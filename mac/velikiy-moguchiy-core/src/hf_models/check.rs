//! «Проверить»: a short recording through a local sherpa-onnx model, to show
//! the user what it hears. The app runs this in a child process, because
//! sherpa-onnx exits the process when a model file is not what it expects.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

const SAMPLE_RATE: u32 = 16_000;
/// Loading the largest Whisper exports takes tens of seconds.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckOutcome {
    pub ok: bool,
    pub text: String,
    /// From loading the model to the final text.
    pub seconds: f64,
    pub error: Option<String>,
}

/// Samples of a 16 kHz mono 16-bit PCM WAV file.
pub fn pcm_from_wav(bytes: &[u8]) -> Result<Vec<i16>, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let mut at = 12;
    let mut format_ok = false;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = at + 8;
        let end = body.checked_add(len).ok_or("bad WAV chunk")?;
        match id {
            b"fmt " => {
                if len < 16 || end > bytes.len() {
                    return Err("bad WAV format chunk".into());
                }
                let word = |o: usize| u16::from_le_bytes([bytes[body + o], bytes[body + o + 1]]);
                let rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().unwrap());
                if word(0) != 1 || word(2) != 1 || rate != SAMPLE_RATE || word(14) != 16 {
                    return Err("the clip must be 16 kHz mono 16-bit PCM".into());
                }
                format_ok = true;
            }
            b"data" => {
                if !format_ok {
                    return Err("WAV data before its format".into());
                }
                let data = &bytes[body..end.min(bytes.len())];
                return Ok(data
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect());
            }
            _ => {}
        }
        at = end + (len & 1);
    }
    Err("no audio in the WAV file".into())
}

/// Recognizes `pcm` (16 kHz mono) with the model in `model_dir`. Blocks.
pub fn check_model(model_dir: &Path, pcm: &[i16]) -> CheckOutcome {
    let started = Instant::now();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())
        .and_then(|runtime| {
            runtime.block_on(async {
                match tokio::time::timeout(CHECK_TIMEOUT, recognize(model_dir, pcm)).await {
                    Ok(result) => result,
                    Err(_) => Err("модель не ответила за три минуты".into()),
                }
            })
        });
    let seconds = (started.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    match result {
        Ok(text) => CheckOutcome {
            ok: true,
            text,
            seconds,
            error: None,
        },
        Err(error) => CheckOutcome {
            ok: false,
            text: String::new(),
            seconds,
            error: Some(error),
        },
    }
}

#[cfg(feature = "sherpa-onnx")]
async fn recognize(model_dir: &Path, pcm: &[i16]) -> Result<String, String> {
    use velikiy_moguchiy_asr::{
        AsrConfig, AsrEvent, AsrProvider, SherpaOnnxConfig, SherpaOnnxProvider,
    };

    let defaults = crate::config::SherpaOnnxAsrConfig::default();
    let mut provider = SherpaOnnxProvider::new(SherpaOnnxConfig {
        model_dir: crate::model_manager::files_dir(model_dir),
        num_threads: defaults.num_threads,
        hotwords: Vec::new(),
        hotwords_score: defaults.hotwords_score,
        endpoint_silence: defaults.endpoint_silence,
        offline_model_type: crate::model_manager::sherpa_offline_model_type(model_dir),
        layout: crate::model_manager::sherpa_layout(model_dir),
        follow_links: crate::model_manager::external_source(model_dir).is_some(),
    });
    provider
        .connect(&AsrConfig::default())
        .await
        .map_err(|e| e.to_string())?;

    // Half a second of silence after the speech, as after a real dictation,
    // lets a streaming model finish the last word.
    let tail = vec![0i16; SAMPLE_RATE as usize / 2];
    for chunk in pcm
        .chunks(SAMPLE_RATE as usize / 10)
        .chain(tail.chunks(SAMPLE_RATE as usize / 10))
    {
        let bytes: Vec<u8> = chunk.iter().flat_map(|s| s.to_le_bytes()).collect();
        provider
            .send_audio(&bytes)
            .await
            .map_err(|e| e.to_string())?;
    }
    provider.finish_input().await.map_err(|e| e.to_string())?;

    let mut text = String::new();
    let outcome = loop {
        match provider.next_event().await {
            Ok(AsrEvent::Final(t)) => {
                text = t;
                break Ok(());
            }
            Ok(AsrEvent::Interim(t) | AsrEvent::Definite(t)) => text = t,
            Ok(AsrEvent::Error(e)) => break Err(e),
            Ok(AsrEvent::Closed(_)) => break Ok(()),
            Ok(_) => {}
            Err(e) => break Err(e.to_string()),
        }
    };
    let _ = provider.close().await;
    outcome.map(|()| text.trim().to_string())
}

#[cfg(not(feature = "sherpa-onnx"))]
async fn recognize(_model_dir: &Path, _pcm: &[i16]) -> Result<String, String> {
    Err("в этой сборке нет локального распознавания sherpa-onnx".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, bits: u16, samples: &[i16], extra_chunk: bool) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
        out.extend(b"fmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * u32::from(channels) * u32::from(bits) / 8).to_le_bytes());
        out.extend((channels * bits / 8).to_le_bytes());
        out.extend(bits.to_le_bytes());
        if extra_chunk {
            out.extend(b"LIST");
            out.extend(3u32.to_le_bytes());
            out.extend(b"abc\0");
        }
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    #[test]
    fn reads_16k_mono_pcm_and_skips_other_chunks() {
        let samples = [0i16, 1, -1, i16::MAX, i16::MIN];
        assert_eq!(
            pcm_from_wav(&wav(16_000, 1, 16, &samples, false)).unwrap(),
            samples
        );
        assert_eq!(
            pcm_from_wav(&wav(16_000, 1, 16, &samples, true)).unwrap(),
            samples
        );
    }

    #[test]
    fn refuses_other_formats() {
        assert!(pcm_from_wav(&wav(44_100, 1, 16, &[0], false)).is_err());
        assert!(pcm_from_wav(&wav(16_000, 2, 16, &[0, 0], false)).is_err());
        assert!(pcm_from_wav(b"OggS").is_err());
        assert!(pcm_from_wav(&wav(16_000, 1, 16, &[0], false)[..20]).is_err());
    }

    #[test]
    fn a_missing_model_fails_without_panicking() {
        let outcome = check_model(Path::new("/nonexistent/velikiy-moguchiy-model"), &[0; 1600]);
        assert!(!outcome.ok);
        assert!(outcome.error.is_some());
    }
}
