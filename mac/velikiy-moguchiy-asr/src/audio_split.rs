//! Cutting long audio into pieces at its quietest moments, for engines and
//! services that only accept a bounded length per request.

use std::ops::Range;

/// Length of the window whose energy decides where a cut goes: long enough
/// to land between words rather than inside a stop consonant.
const QUIET_WINDOW_SECS: f32 = 0.3;
const FRAME_SECS: f32 = 0.01;

/// Splits `samples` into consecutive ranges of at most `max_secs` that cover
/// all of it. Each cut lies at the quietest stretch between `min_secs` and
/// `max_secs` after the previous one, so pieces end in pauses wherever the
/// speaker paused.
pub fn split_at_quiet(
    samples: &[f32],
    sample_rate: u32,
    min_secs: f32,
    max_secs: f32,
) -> Vec<Range<usize>> {
    let max_len = ((max_secs * sample_rate as f32) as usize).max(1);
    let min_len = ((min_secs * sample_rate as f32) as usize).min(max_len);
    let frame = ((FRAME_SECS * sample_rate as f32) as usize).max(1);
    let window_frames = ((QUIET_WINDOW_SECS / FRAME_SECS) as usize).max(1);

    let energy: Vec<f32> = samples
        .chunks(frame)
        .map(|c| c.iter().map(|s| s * s).sum::<f32>())
        .collect();
    // window_energy[i] = energy of frames i..i+window_frames
    let mut prefix = Vec::with_capacity(energy.len() + 1);
    prefix.push(0.0f64);
    for e in &energy {
        prefix.push(prefix.last().unwrap() + *e as f64);
    }
    let window_energy = |first: usize| {
        let last = (first + window_frames).min(energy.len());
        prefix[last] - prefix[first]
    };

    let mut ranges = Vec::new();
    let mut start = 0;
    while samples.len() - start > max_len {
        let first = (start + min_len) / frame;
        let last = ((start + max_len) / frame)
            .saturating_sub(window_frames)
            .max(first);
        let quietest = (first..=last)
            .min_by(|&a, &b| window_energy(a).total_cmp(&window_energy(b)))
            .unwrap_or(first);
        let cut = ((quietest + window_frames / 2) * frame).clamp(start + 1, start + max_len);
        ranges.push(start..cut);
        start = cut;
    }
    if start < samples.len() {
        ranges.push(start..samples.len());
    }
    ranges
}

/// Whether `samples` hold nothing louder than background noise: no 30 ms
/// stretch reaches −46 dBFS.
pub fn is_silent(samples: &[f32], sample_rate: u32) -> bool {
    const SPEECH_RMS: f32 = 0.005;
    let frame = ((0.03 * sample_rate as f32) as usize).max(1);
    samples.chunks(frame).all(|c| {
        let mean_square = c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32;
        mean_square.sqrt() < SPEECH_RMS
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    fn tone(secs: f32) -> Vec<f32> {
        (0..(secs * SR as f32) as usize)
            .map(|i| 0.3 * (i as f32 * 0.05).sin())
            .collect()
    }

    fn silence(secs: f32) -> Vec<f32> {
        vec![0.0; (secs * SR as f32) as usize]
    }

    #[test]
    fn short_audio_is_one_piece() {
        let audio = tone(5.0);
        assert_eq!(split_at_quiet(&audio, SR, 4.0, 20.0), vec![0..audio.len()]);
        assert!(split_at_quiet(&[], SR, 4.0, 20.0).is_empty());
    }

    #[test]
    fn cuts_in_the_pause() {
        let mut audio = tone(12.0);
        audio.extend(silence(1.0));
        audio.extend(tone(12.0));
        let pieces = split_at_quiet(&audio, SR, 4.0, 20.0);
        assert_eq!(pieces.len(), 2);
        let cut_secs = pieces[0].end as f32 / SR as f32;
        assert!((12.0..13.0).contains(&cut_secs), "cut at {cut_secs}");
        assert_eq!(pieces[1].end, audio.len());
    }

    #[test]
    fn pieces_never_exceed_the_limit_and_cover_everything() {
        let audio = tone(95.0);
        let pieces = split_at_quiet(&audio, SR, 4.0, 20.0);
        let max = 20 * SR as usize;
        assert!(pieces.iter().all(|r| r.len() <= max && !r.is_empty()));
        assert_eq!(pieces.first().unwrap().start, 0);
        assert_eq!(pieces.last().unwrap().end, audio.len());
        assert!(pieces.windows(2).all(|w| w[0].end == w[1].start));
    }

    #[test]
    fn detects_silence() {
        assert!(is_silent(&silence(2.0), SR));
        assert!(!is_silent(&tone(0.5), SR));
        let mut mostly_quiet = silence(3.0);
        mostly_quiet.extend(tone(0.2));
        assert!(!is_silent(&mostly_quiet, SR));
    }
}
