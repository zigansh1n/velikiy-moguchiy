use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use crate::config::AsrConfig;
use crate::error::{AsrError, Result};
use crate::event::AsrEvent;

/// Configuration for the sherpa-onnx local streaming ASR provider.
#[derive(Debug, Clone)]
pub struct SherpaOnnxConfig {
    /// Model directory path (e.g. ~/.velikiy-moguchiy/models/sherpa-onnx/<model-name>/)
    pub model_dir: PathBuf,
    /// Number of threads for inference
    pub num_threads: i32,
    /// Hotwords (dictionary terms) for improved recognition
    pub hotwords: Vec<String>,
    /// Score boost for hotwords
    pub hotwords_score: f32,
    /// Endpoint rule 2: trailing silence after speech (seconds)
    pub endpoint_silence: f32,
    /// sherpa-onnx `model_type` of a non-streaming model (e.g. "nemo_transducer");
    /// `None` for streaming zipformer models.
    pub offline_model_type: Option<String>,
}

enum SherpaCmd {
    Audio(Vec<f32>),
    Finish,
    Close,
}

/// Local streaming ASR provider using sherpa-onnx OnlineRecognizer.
///
/// Runs the synchronous sherpa-onnx API on a dedicated worker thread,
/// bridging to async via channels.
pub struct SherpaOnnxProvider {
    config: SherpaOnnxConfig,
    cmd_tx: Option<std_mpsc::Sender<SherpaCmd>>,
    event_rx: Option<tokio::sync::mpsc::Receiver<AsrEvent>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl SherpaOnnxProvider {
    pub fn new(config: SherpaOnnxConfig) -> Self {
        Self {
            config,
            cmd_tx: None,
            event_rx: None,
            worker: None,
        }
    }
}

#[async_trait::async_trait]
impl crate::provider::AsrProvider for SherpaOnnxProvider {
    // Local provider: configuration is passed via `new()`, not through AsrConfig.
    // The `_config` parameter is unused here.
    async fn connect(&mut self, _config: &AsrConfig) -> Result<()> {
        let model_dir = &self.config.model_dir;

        if !model_dir.exists() {
            return Err(AsrError::Connection(format!(
                "model not found: {}",
                model_dir.display()
            )));
        }

        let (encoder, decoder, joiner, tokens) = find_model_files(model_dir)?;

        if let Some(model_type) = self.config.offline_model_type.clone() {
            let files = OfflineModelFiles {
                encoder,
                decoder,
                joiner,
                tokens,
                model_type,
                num_threads: self.config.num_threads,
            };
            let (cmd_tx, cmd_rx) = std_mpsc::channel::<SherpaCmd>();
            let (event_tx, event_rx) = tokio::sync::mpsc::channel::<AsrEvent>(256);
            let worker = std::thread::spawn(move || offline_worker_loop(files, cmd_rx, event_tx));
            self.cmd_tx = Some(cmd_tx);
            self.event_rx = Some(event_rx);
            self.worker = Some(worker);
            return Ok(());
        }

        let num_threads = self.config.num_threads;
        let endpoint_silence = self.config.endpoint_silence;
        let hotwords_score = self.config.hotwords_score;
        let hotwords_buf = if self.config.hotwords.is_empty() {
            None
        } else {
            Some(self.config.hotwords.join("\n").into_bytes())
        };
        let (cmd_tx, cmd_rx) = std_mpsc::channel::<SherpaCmd>();
        let (event_tx, event_rx) = tokio::sync::mpsc::channel::<AsrEvent>(256);

        let worker = std::thread::spawn(move || {
            worker_loop(
                encoder,
                decoder,
                joiner,
                tokens,
                num_threads,
                endpoint_silence,
                hotwords_buf,
                hotwords_score,
                cmd_rx,
                event_tx,
            );
        });

        self.cmd_tx = Some(cmd_tx);
        self.event_rx = Some(event_rx);
        self.worker = Some(worker);

        Ok(())
    }

    async fn send_audio(&mut self, frame: &[u8]) -> Result<()> {
        let samples: Vec<f32> = frame
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect();

        if let Some(ref tx) = self.cmd_tx {
            tx.send(SherpaCmd::Audio(samples))
                .map_err(|_| AsrError::Connection("worker thread closed".into()))?;
        }
        Ok(())
    }

    async fn finish_input(&mut self) -> Result<()> {
        if let Some(ref tx) = self.cmd_tx {
            let _ = tx.send(SherpaCmd::Finish);
        }
        Ok(())
    }

    async fn next_event(&mut self) -> Result<AsrEvent> {
        if let Some(ref mut rx) = self.event_rx {
            rx.recv()
                .await
                .ok_or(AsrError::Connection("event channel closed".into()))
        } else {
            Err(AsrError::Connection("not connected".into()))
        }
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(SherpaCmd::Close);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.event_rx = None;
        Ok(())
    }
}

/// Find encoder/decoder/joiner/tokens files in the model directory.
/// Prefers int8 quantized models when available (smaller, faster on CPU).
fn find_model_files(model_dir: &std::path::Path) -> Result<(String, String, String, String)> {
    let entries: Vec<_> = std::fs::read_dir(model_dir)
        .map_err(|e| AsrError::Connection(format!("read model dir: {e}")))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    // Prefer int8 > fp16 > fp32
    let find = |pattern: &str| -> Result<String> {
        let candidates: Vec<_> = entries
            .iter()
            .filter(|name| name.contains(pattern) && name.ends_with(".onnx"))
            .collect();
        let chosen = candidates
            .iter()
            .find(|n| n.contains(".int8."))
            .or_else(|| candidates.iter().find(|n| n.contains(".fp16.")))
            .or_else(|| candidates.first());
        chosen
            .map(|name| model_dir.join(name).to_string_lossy().to_string())
            .ok_or_else(|| {
                AsrError::Connection(format!(
                    "missing {pattern}*.onnx in {}",
                    model_dir.display()
                ))
            })
    };

    let encoder = find("encoder")?;
    let decoder = find("decoder")?;
    let joiner = find("joiner")?;

    let tokens = entries
        .iter()
        .find(|name| *name == "tokens.txt")
        .map(|name| model_dir.join(name).to_string_lossy().to_string())
        .ok_or_else(|| {
            AsrError::Connection(format!("missing tokens.txt in {}", model_dir.display()))
        })?;

    log::info!("sherpa model files: encoder={encoder}, decoder={decoder}, joiner={joiner}");
    Ok((encoder, decoder, joiner, tokens))
}

/// Worker thread: runs sherpa-onnx OnlineRecognizer synchronously.
#[allow(clippy::too_many_arguments)]
fn worker_loop(
    encoder: String,
    decoder: String,
    joiner: String,
    tokens: String,
    num_threads: i32,
    endpoint_silence: f32,
    hotwords_buf: Option<Vec<u8>>,
    hotwords_score: f32,
    cmd_rx: std_mpsc::Receiver<SherpaCmd>,
    event_tx: tokio::sync::mpsc::Sender<AsrEvent>,
) {
    use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig};

    let mut config = OnlineRecognizerConfig::default();
    config.model_config.transducer.encoder = Some(encoder);
    config.model_config.transducer.decoder = Some(decoder);
    config.model_config.transducer.joiner = Some(joiner);
    config.model_config.tokens = Some(tokens);
    config.model_config.num_threads = num_threads;
    config.decoding_method = Some("greedy_search".into());
    config.enable_endpoint = true;
    config.rule1_min_trailing_silence = 2.4;
    config.rule2_min_trailing_silence = endpoint_silence;
    config.rule3_min_utterance_length = 300.0; // effectively disabled
    config.hotwords_buf = hotwords_buf;
    config.hotwords_score = hotwords_score;

    let recognizer = match OnlineRecognizer::create(&config) {
        Some(r) => r,
        None => {
            let _ = event_tx.blocking_send(AsrEvent::Error(
                "failed to create sherpa-onnx recognizer".into(),
            ));
            return;
        }
    };

    let stream = recognizer.create_stream();
    let _ = event_tx.blocking_send(AsrEvent::Connected);

    let mut accumulated_text = String::new();
    let mut last_text = String::new();
    let decode_timeout = Duration::from_millis(200);

    loop {
        match cmd_rx.recv_timeout(decode_timeout) {
            Ok(SherpaCmd::Audio(samples)) => {
                stream.accept_waveform(16000, &samples);
            }
            Ok(SherpaCmd::Finish) => {
                stream.input_finished();
                // Final decode pass
                while recognizer.is_ready(&stream) {
                    recognizer.decode(&stream);
                }
                let result_text = recognizer
                    .get_result(&stream)
                    .map(|r| r.text)
                    .unwrap_or_default();
                let mut final_text = accumulated_text.clone();
                crate::transcript::append_segment(&mut final_text, &result_text);
                let _ = event_tx.blocking_send(AsrEvent::Final(final_text));
                break;
            }
            Ok(SherpaCmd::Close) => {
                break;
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                // Periodic decode — fall through to decode loop below
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }

        // Decode available frames
        while recognizer.is_ready(&stream) {
            recognizer.decode(&stream);
        }

        let result_text = recognizer
            .get_result(&stream)
            .map(|r| r.text)
            .unwrap_or_default();

        // Check endpoint (sentence boundary detected)
        if recognizer.is_endpoint(&stream) {
            if !result_text.is_empty() {
                crate::transcript::append_segment(&mut accumulated_text, &result_text);
                let _ = event_tx.blocking_send(AsrEvent::Definite(accumulated_text.clone()));
            }
            recognizer.reset(&stream);
            last_text = String::new();
        } else {
            // Emit Interim if text changed
            let mut current = accumulated_text.clone();
            crate::transcript::append_segment(&mut current, &result_text);
            if current != last_text && !current.is_empty() {
                let _ = event_tx.blocking_send(AsrEvent::Interim(current.clone()));
                last_text = current;
            }
        }
    }

    let _ = event_tx.blocking_send(AsrEvent::Closed(None));
}

struct OfflineModelFiles {
    encoder: String,
    decoder: String,
    joiner: String,
    tokens: String,
    model_type: String,
    num_threads: i32,
}

/// GigaAM-style offline models are trained on utterances up to ~25 s and
/// degrade on longer input, so audio is decoded in pieces cut at pauses.
const OFFLINE_PIECE_MIN_SECS: f32 = 5.0;
const OFFLINE_PIECE_MAX_SECS: f32 = 20.0;
const SAMPLE_RATE: u32 = 16_000;

/// How long a loaded offline model stays in memory after its last use.
/// Loading a ~230 MB model takes about a second, too slow for every hotkey press.
const OFFLINE_MODEL_IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);

struct CachedRecognizer {
    key: String,
    recognizer: std::sync::Arc<sherpa_onnx::OfflineRecognizer>,
    last_used: std::time::Instant,
}

static OFFLINE_CACHE: std::sync::Mutex<Option<CachedRecognizer>> = std::sync::Mutex::new(None);

fn offline_recognizer(
    files: &OfflineModelFiles,
) -> Option<std::sync::Arc<sherpa_onnx::OfflineRecognizer>> {
    use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig};

    let key = format!(
        "{}|{}|{}",
        files.encoder, files.model_type, files.num_threads
    );
    let mut cache = OFFLINE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = cache.as_mut().filter(|c| c.key == key) {
        cached.last_used = std::time::Instant::now();
        return Some(cached.recognizer.clone());
    }
    // Drop the previous model before loading the next one: two of them
    // would double the memory peak.
    *cache = None;

    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer.encoder = Some(files.encoder.clone());
    config.model_config.transducer.decoder = Some(files.decoder.clone());
    config.model_config.transducer.joiner = Some(files.joiner.clone());
    config.model_config.tokens = Some(files.tokens.clone());
    config.model_config.model_type = Some(files.model_type.clone());
    config.model_config.num_threads = files.num_threads;
    config.decoding_method = Some("greedy_search".into());

    let recognizer = std::sync::Arc::new(OfflineRecognizer::create(&config)?);
    *cache = Some(CachedRecognizer {
        key,
        recognizer: recognizer.clone(),
        last_used: std::time::Instant::now(),
    });
    start_offline_cache_reaper();
    Some(recognizer)
}

fn start_offline_cache_reaper() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_secs(60));
            let mut cache = OFFLINE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            let idle = cache.as_ref().is_some_and(|c| {
                c.last_used.elapsed() >= OFFLINE_MODEL_IDLE_UNLOAD
                    && std::sync::Arc::strong_count(&c.recognizer) == 1
            });
            if idle {
                log::info!("sherpa offline model unloaded after idle");
                *cache = None;
            }
        });
    });
}

fn decode_offline(recognizer: &sherpa_onnx::OfflineRecognizer, samples: &[f32]) -> String {
    if crate::audio_split::is_silent(samples, SAMPLE_RATE) {
        return String::new();
    }
    let stream = recognizer.create_stream();
    stream.accept_waveform(SAMPLE_RATE as i32, samples);
    recognizer.decode(&stream);
    stream.get_result().map(|r| r.text).unwrap_or_default()
}

/// Worker thread for non-streaming models: buffers audio, decodes a piece
/// whenever the buffer outgrows the length limit, and the rest when input
/// finishes.
fn offline_worker_loop(
    files: OfflineModelFiles,
    cmd_rx: std_mpsc::Receiver<SherpaCmd>,
    event_tx: tokio::sync::mpsc::Sender<AsrEvent>,
) {
    use crate::audio_split::split_at_quiet;
    use crate::transcript::append_segment;

    let Some(recognizer) = offline_recognizer(&files) else {
        let _ = event_tx.blocking_send(AsrEvent::Error(
            "failed to create sherpa-onnx offline recognizer".into(),
        ));
        return;
    };
    let _ = event_tx.blocking_send(AsrEvent::Connected);

    let max_piece = (OFFLINE_PIECE_MAX_SECS * SAMPLE_RATE as f32) as usize;
    let mut pending: Vec<f32> = Vec::new();
    let mut text = String::new();

    loop {
        match cmd_rx.recv() {
            Ok(SherpaCmd::Audio(samples)) => {
                pending.extend_from_slice(&samples);
                if pending.len() > max_piece {
                    let first = split_at_quiet(
                        &pending,
                        SAMPLE_RATE,
                        OFFLINE_PIECE_MIN_SECS,
                        OFFLINE_PIECE_MAX_SECS,
                    )
                    .remove(0);
                    let piece: Vec<f32> = pending.drain(first).collect();
                    append_segment(&mut text, &decode_offline(&recognizer, &piece));
                    if !text.is_empty() {
                        let _ = event_tx.blocking_send(AsrEvent::Definite(text.clone()));
                    }
                }
            }
            Ok(SherpaCmd::Finish) => {
                for range in split_at_quiet(
                    &pending,
                    SAMPLE_RATE,
                    OFFLINE_PIECE_MIN_SECS,
                    OFFLINE_PIECE_MAX_SECS,
                ) {
                    append_segment(&mut text, &decode_offline(&recognizer, &pending[range]));
                }
                let _ = event_tx.blocking_send(AsrEvent::Final(text));
                break;
            }
            Ok(SherpaCmd::Close) | Err(_) => break,
        }
    }

    let _ = event_tx.blocking_send(AsrEvent::Closed(None));
}
