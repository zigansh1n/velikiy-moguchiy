//! Local non-streaming ASR with whisper.cpp ggml models, on Metal where the
//! Mac has it.

use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::config::AsrConfig;
use crate::error::{AsrError, Result};
use crate::event::AsrEvent;

const SAMPLE_RATE: u32 = 16_000;
/// Whisper hears 30-second windows; pieces cut at pauses keep words whole
/// and let long dictation show text before it ends.
const PIECE_MIN_SECS: f32 = 10.0;
const PIECE_MAX_SECS: f32 = 28.0;
/// whisper.cpp returns nothing for less than a second of audio.
const MIN_PIECE_SAMPLES: usize = SAMPLE_RATE as usize + SAMPLE_RATE as usize / 10;
/// The prompt is cut at 224 tokens; this many characters of terms stay under it.
const MAX_PROMPT_CHARS: usize = 400;
const MODEL_IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone)]
pub struct WhisperCppConfig {
    /// The ggml model file.
    pub model_path: PathBuf,
    /// ISO 639-1 code such as "ru", or "auto" to let the model tell.
    pub language: String,
    pub num_threads: i32,
    /// Dictionary terms, given to the model as its prompt.
    pub hotwords: Vec<String>,
}

enum Cmd {
    Audio(Vec<f32>),
    Finish,
    Close,
}

pub struct WhisperCppProvider {
    config: WhisperCppConfig,
    cmd_tx: Option<std_mpsc::Sender<Cmd>>,
    event_rx: Option<tokio::sync::mpsc::Receiver<AsrEvent>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl WhisperCppProvider {
    pub fn new(config: WhisperCppConfig) -> Self {
        Self {
            config,
            cmd_tx: None,
            event_rx: None,
            worker: None,
        }
    }
}

#[async_trait::async_trait]
impl crate::provider::AsrProvider for WhisperCppProvider {
    async fn connect(&mut self, _config: &AsrConfig) -> Result<()> {
        let path = &self.config.model_path;
        if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
            return Err(AsrError::Connection(format!(
                "model not found: {}",
                path.display()
            )));
        }
        let (cmd_tx, cmd_rx) = std_mpsc::channel::<Cmd>();
        let (event_tx, event_rx) = tokio::sync::mpsc::channel::<AsrEvent>(256);
        let config = self.config.clone();
        self.worker = Some(std::thread::spawn(move || {
            worker_loop(config, cmd_rx, event_tx)
        }));
        self.cmd_tx = Some(cmd_tx);
        self.event_rx = Some(event_rx);
        Ok(())
    }

    async fn send_audio(&mut self, frame: &[u8]) -> Result<()> {
        let samples: Vec<f32> = frame
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect();
        if let Some(ref tx) = self.cmd_tx {
            tx.send(Cmd::Audio(samples))
                .map_err(|_| AsrError::Connection("worker thread closed".into()))?;
        }
        Ok(())
    }

    async fn finish_input(&mut self) -> Result<()> {
        if let Some(ref tx) = self.cmd_tx {
            let _ = tx.send(Cmd::Finish);
        }
        Ok(())
    }

    async fn next_event(&mut self) -> Result<AsrEvent> {
        match self.event_rx {
            Some(ref mut rx) => rx
                .recv()
                .await
                .ok_or(AsrError::Connection("event channel closed".into())),
            None => Err(AsrError::Connection("not connected".into())),
        }
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(Cmd::Close);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.event_rx = None;
        Ok(())
    }
}

struct CachedContext {
    path: PathBuf,
    context: Arc<WhisperContext>,
    last_used: Instant,
}

/// Loading large-v3 takes seconds, too slow for every hotkey press.
static CACHE: Mutex<Option<CachedContext>> = Mutex::new(None);

fn context(path: &PathBuf) -> std::result::Result<Arc<WhisperContext>, String> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = cache.as_mut().filter(|c| &c.path == path) {
        cached.last_used = Instant::now();
        return Ok(cached.context.clone());
    }
    // Two models at once would double the memory peak.
    *cache = None;
    whisper_rs::install_logging_hooks();
    let context = Arc::new(
        WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .map_err(|e| format!("whisper.cpp could not load {}: {e}", path.display()))?,
    );
    *cache = Some(CachedContext {
        path: path.clone(),
        context: context.clone(),
        last_used: Instant::now(),
    });
    start_cache_reaper();
    Ok(context)
}

fn start_cache_reaper() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_secs(60));
            let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            let idle = cache.as_ref().is_some_and(|c| {
                c.last_used.elapsed() >= MODEL_IDLE_UNLOAD && Arc::strong_count(&c.context) == 1
            });
            if idle {
                log::info!("whisper.cpp model unloaded after idle");
                *cache = None;
            }
        });
    });
}

/// The dictionary as a prompt: Whisper spells the words it has just "seen".
pub(crate) fn prompt_from(hotwords: &[String]) -> String {
    let mut prompt = String::new();
    for term in hotwords.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        let extra = if prompt.is_empty() { 0 } else { 2 };
        if prompt.chars().count() + extra + term.chars().count() > MAX_PROMPT_CHARS {
            break;
        }
        if !prompt.is_empty() {
            prompt.push_str(", ");
        }
        prompt.push_str(term);
    }
    prompt
}

/// Whisper's answer to silence or noise, learned from subtitled videos.
pub(crate) fn is_known_hallucination(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    let lower =
        lower.trim_matches(|c: char| c.is_ascii_punctuation() || c == '…' || c.is_whitespace());
    [
        "продолжение следует",
        "субтитры сделал dimatorzok",
        "субтитры создавал dimatorzok",
        "редактор субтитров а.семкин корректор а.егорова",
        "thank you for watching",
    ]
    .contains(&lower)
}

fn decode(
    context: &WhisperContext,
    config: &WhisperCppConfig,
    prompt: &str,
    samples: &[f32],
) -> std::result::Result<String, String> {
    if crate::audio_split::is_silent(samples, SAMPLE_RATE) {
        return Ok(String::new());
    }
    let mut padded;
    let samples = if samples.len() < MIN_PIECE_SAMPLES {
        padded = samples.to_vec();
        padded.resize(MIN_PIECE_SAMPLES, 0.0);
        &padded[..]
    } else {
        samples
    };
    let mut state = context.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let language = config.language.trim();
    params.set_language(Some(if language.is_empty() {
        "auto"
    } else {
        language
    }));
    params.set_n_threads(config.num_threads.max(1));
    params.set_translate(false);
    params.set_no_timestamps(true);
    params.set_suppress_nst(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    if !prompt.is_empty() {
        params.set_initial_prompt(prompt);
    }
    state.full(params, samples).map_err(|e| e.to_string())?;
    let mut text = String::new();
    for segment in state.as_iter() {
        let piece = segment.to_str_lossy().map_err(|e| e.to_string())?;
        if !is_known_hallucination(&piece) {
            crate::transcript::append_segment(&mut text, piece.trim());
        }
    }
    Ok(text)
}

fn worker_loop(
    config: WhisperCppConfig,
    cmd_rx: std_mpsc::Receiver<Cmd>,
    event_tx: tokio::sync::mpsc::Sender<AsrEvent>,
) {
    use crate::audio_split::split_at_quiet;
    use crate::transcript::append_segment;

    let context = match context(&config.model_path) {
        Ok(context) => context,
        Err(error) => {
            let _ = event_tx.blocking_send(AsrEvent::Error(error));
            return;
        }
    };
    let _ = event_tx.blocking_send(AsrEvent::Connected);
    let prompt = prompt_from(&config.hotwords);
    let max_piece = (PIECE_MAX_SECS * SAMPLE_RATE as f32) as usize;
    let mut pending: Vec<f32> = Vec::new();
    let mut text = String::new();
    let piece_text = |text: &mut String, piece: &[f32]| -> bool {
        match decode(&context, &config, &prompt, piece) {
            Ok(recognized) => {
                append_segment(text, &recognized);
                true
            }
            Err(error) => {
                let _ = event_tx.blocking_send(AsrEvent::Error(error));
                false
            }
        }
    };

    loop {
        match cmd_rx.recv() {
            Ok(Cmd::Audio(samples)) => {
                pending.extend_from_slice(&samples);
                if pending.len() > max_piece {
                    let first =
                        split_at_quiet(&pending, SAMPLE_RATE, PIECE_MIN_SECS, PIECE_MAX_SECS)
                            .remove(0);
                    let piece: Vec<f32> = pending.drain(first).collect();
                    if !piece_text(&mut text, &piece) {
                        break;
                    }
                    if !text.is_empty() {
                        let _ = event_tx.blocking_send(AsrEvent::Definite(text.clone()));
                    }
                }
            }
            Ok(Cmd::Finish) => {
                let mut ok = true;
                for range in split_at_quiet(&pending, SAMPLE_RATE, PIECE_MIN_SECS, PIECE_MAX_SECS) {
                    ok = piece_text(&mut text, &pending[range]);
                    if !ok {
                        break;
                    }
                }
                if ok {
                    let _ = event_tx.blocking_send(AsrEvent::Final(text));
                }
                break;
            }
            Ok(Cmd::Close) | Err(_) => break,
        }
    }

    let _ = event_tx.blocking_send(AsrEvent::Closed(None));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_keeps_whole_terms_within_the_limit() {
        let terms: Vec<String> = (0..200).map(|i| format!("термин{i}")).collect();
        let prompt = prompt_from(&terms);
        assert!(prompt.chars().count() <= MAX_PROMPT_CHARS);
        assert!(prompt.starts_with("термин0, термин1, "));
        assert!(prompt.split(", ").all(|t| t.starts_with("термин")));
        assert_eq!(
            prompt_from(&["  ".into(), "Kubernetes".into()]),
            "Kubernetes"
        );
    }

    #[test]
    fn only_a_whole_known_phrase_counts_as_a_hallucination() {
        assert!(is_known_hallucination(" Продолжение следует..."));
        assert!(is_known_hallucination("Субтитры сделал DimaTorzok"));
        assert!(!is_known_hallucination("Продолжение следует в понедельник"));
        assert!(!is_known_hallucination("Спасибо"));
    }
}
