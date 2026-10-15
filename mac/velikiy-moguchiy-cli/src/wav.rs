//! Reading the app's own recordings (16 kHz, 16-bit WAV) without ffmpeg.

/// Samples of a 16 kHz 16-bit PCM WAV, channels averaged to mono. `None`
/// for any other WAV, which then goes through ffmpeg.
pub fn pcm16_at_16khz(bytes: &[u8]) -> Option<Vec<i16>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12;
    let mut channels = 0usize;
    let mut format_ok = false;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        let end = body.checked_add(size)?.min(bytes.len());
        match id {
            b"fmt " if size >= 16 && end - body >= 16 => {
                let format = u16::from_le_bytes([bytes[body], bytes[body + 1]]);
                channels = u16::from_le_bytes([bytes[body + 2], bytes[body + 3]]) as usize;
                let rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().ok()?);
                let bits = u16::from_le_bytes([bytes[body + 14], bytes[body + 15]]);
                // 0xFFFE is WAVE_FORMAT_EXTENSIBLE, which AVFoundation writes for PCM too.
                format_ok = (format == 1 || format == 0xFFFE)
                    && rate == 16_000
                    && bits == 16
                    && channels > 0;
            }
            b"data" => {
                if !format_ok {
                    return None;
                }
                let frame = channels * 2;
                let data = &bytes[body..end];
                return Some(
                    data.chunks_exact(frame)
                        .map(|f| {
                            let sum: i32 = f
                                .chunks_exact(2)
                                .map(|s| i16::from_le_bytes([s[0], s[1]]) as i32)
                                .sum();
                            (sum / channels as i32) as i16
                        })
                        .collect(),
                );
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        pos = body.checked_add(size + (size & 1))?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]); // odd chunk plus its pad byte
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn reads_mono_and_downmixes_stereo() {
        assert_eq!(
            pcm16_at_16khz(&wav(16_000, 1, &[1, -2, 3])),
            Some(vec![1, -2, 3])
        );
        assert_eq!(
            pcm16_at_16khz(&wav(16_000, 2, &[10, 20, -4, 0])),
            Some(vec![15, -2])
        );
    }

    #[test]
    fn leaves_other_rates_to_ffmpeg() {
        assert_eq!(pcm16_at_16khz(&wav(44_100, 1, &[1, 2])), None);
        assert_eq!(pcm16_at_16khz(b"OggS"), None);
    }
}
