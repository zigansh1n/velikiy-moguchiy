use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use crate::config::AsrConfig;
use crate::error::{AsrError, Result};
use crate::event::AsrEvent;
use crate::sherpa_layout::{ResolvedLayout, SherpaFamily, SherpaLayout, SherpaRole};

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
    /// `None` for streaming zipformer models. Ignored when `layout` is set.
    pub offline_model_type: Option<String>,
    /// The model's recorded layout; `None` for the built-in transducers,
    /// whose files are found by name.
    pub layout: Option<SherpaLayout>,
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

        let layout = match &self.config.layout {
            Some(layout) => layout.resolve(model_dir),
            None => {
                ResolvedLayout::legacy_transducer(model_dir, self.config.offline_model_type.clone())
            }
        }
        .map_err(AsrError::Connection)?;
        log::info!(
            "sherpa model: {:?}, streaming: {}",
            layout.family,
            layout.streaming
        );

        let (cmd_tx, cmd_rx) = std_mpsc::channel::<SherpaCmd>();
        let (event_tx, event_rx) = tokio::sync::mpsc::channel::<AsrEvent>(256);
        let worker = if layout.streaming {
            let config = online_config(&layout, &self.config);
            std::thread::spawn(move || worker_loop(config, cmd_rx, event_tx))
        } else {
            let key = layout.cache_key(self.config.num_threads);
            let config = offline_config(&layout, self.config.num_threads);
            std::thread::spawn(move || offline_worker_loop(key, config, cmd_rx, event_tx))
        };

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

/// Recognizer settings for a streaming model.
fn online_config(
    layout: &ResolvedLayout,
    settings: &SherpaOnnxConfig,
) -> sherpa_onnx::OnlineRecognizerConfig {
    let mut config = sherpa_onnx::OnlineRecognizerConfig::default();
    config.feat_config.sample_rate = layout.sample_rate;
    config.feat_config.feature_dim = layout.feature_dim;
    let model = &mut config.model_config;
    match layout.family {
        SherpaFamily::Paraformer => {
            model.paraformer.encoder = layout.path(SherpaRole::Encoder);
            model.paraformer.decoder = layout.path(SherpaRole::Decoder);
        }
        SherpaFamily::NemoCtc => model.nemo_ctc.model = layout.path(SherpaRole::Model),
        SherpaFamily::ZipformerCtc => model.zipformer2_ctc.model = layout.path(SherpaRole::Model),
        SherpaFamily::ToneCtc => model.t_one_ctc.model = layout.path(SherpaRole::Model),
        _ => {
            model.transducer.encoder = layout.path(SherpaRole::Encoder);
            model.transducer.decoder = layout.path(SherpaRole::Decoder);
            model.transducer.joiner = layout.path(SherpaRole::Joiner);
        }
    }
    model.tokens = layout.path(SherpaRole::Tokens);
    model.num_threads = settings.num_threads;
    model.model_type = layout.model_type.clone();
    config.decoding_method = Some("greedy_search".into());
    config.enable_endpoint = true;
    config.rule1_min_trailing_silence = 2.4;
    config.rule2_min_trailing_silence = settings.endpoint_silence;
    config.rule3_min_utterance_length = 300.0; // effectively disabled

    // Hotwords stay with the transducers that always received them.
    if layout.family == SherpaFamily::Transducer && !settings.hotwords.is_empty() {
        config.hotwords_buf = Some(settings.hotwords.join("\n").into_bytes());
        config.hotwords_score = settings.hotwords_score;
    }
    config
}

/// Worker thread: runs sherpa-onnx OnlineRecognizer synchronously.
fn worker_loop(
    config: sherpa_onnx::OnlineRecognizerConfig,
    cmd_rx: std_mpsc::Receiver<SherpaCmd>,
    event_tx: tokio::sync::mpsc::Sender<AsrEvent>,
) {
    use sherpa_onnx::OnlineRecognizer;

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

/// Recognizer settings for a non-streaming model.
fn offline_config(
    layout: &ResolvedLayout,
    num_threads: i32,
) -> sherpa_onnx::OfflineRecognizerConfig {
    let mut config = sherpa_onnx::OfflineRecognizerConfig::default();
    config.feat_config.sample_rate = layout.sample_rate;
    config.feat_config.feature_dim = layout.feature_dim;
    let model = &mut config.model_config;
    let main = layout.path(SherpaRole::Model);
    let encoder = layout.path(SherpaRole::Encoder);
    let decoder = layout.path(SherpaRole::Decoder);
    // sherpa-onnx picks the implementation by model_type when it is set;
    // these are the values its own Python constructors pass.
    let mut model_type = layout.model_type.clone();
    match layout.family {
        SherpaFamily::Transducer | SherpaFamily::NemoTransducer => {
            model.transducer.encoder = encoder;
            model.transducer.decoder = decoder;
            model.transducer.joiner = layout.path(SherpaRole::Joiner);
            if model_type.is_none() {
                model_type = Some(
                    if layout.family == SherpaFamily::NemoTransducer {
                        "nemo_transducer"
                    } else {
                        "transducer"
                    }
                    .into(),
                );
            }
        }
        SherpaFamily::Paraformer => {
            model.paraformer.model = main;
            model_type.get_or_insert_with(|| "paraformer".into());
        }
        SherpaFamily::NemoCtc => {
            model.nemo_ctc.model = main;
            model_type.get_or_insert_with(|| "nemo_ctc".into());
        }
        SherpaFamily::ZipformerCtc => model.zipformer_ctc.model = main,
        SherpaFamily::WenetCtc => {
            model.wenet_ctc.model = main;
            model_type.get_or_insert_with(|| "wenet_ctc".into());
        }
        SherpaFamily::TelespeechCtc => model.telespeech_ctc = main,
        SherpaFamily::OmnilingualCtc => model.omnilingual.model = main,
        SherpaFamily::MedasrCtc => model.medasr.model = main,
        SherpaFamily::FireRedAsrCtc => model.fire_red_asr_ctc.model = main,
        SherpaFamily::Whisper => {
            model.whisper.encoder = encoder;
            model.whisper.decoder = decoder;
            model.whisper.task = Some("transcribe".into());
            model.whisper.tail_paddings = -1;
            model_type.get_or_insert_with(|| "whisper".into());
        }
        SherpaFamily::SenseVoice => {
            model.sense_voice.model = main;
            model.sense_voice.use_itn = true;
        }
        SherpaFamily::Moonshine => {
            model.moonshine.preprocessor = layout.path(SherpaRole::Preprocessor);
            model.moonshine.encoder = encoder;
            model.moonshine.uncached_decoder = layout.path(SherpaRole::UncachedDecoder);
            model.moonshine.cached_decoder = layout.path(SherpaRole::CachedDecoder);
        }
        SherpaFamily::FireRedAsr => {
            model.fire_red_asr.encoder = encoder;
            model.fire_red_asr.decoder = decoder;
        }
        SherpaFamily::Dolphin => model.dolphin.model = main,
        SherpaFamily::Canary => {
            model.canary.encoder = encoder;
            model.canary.decoder = decoder;
            model.canary.use_pnc = true;
        }
        SherpaFamily::ToneCtc | SherpaFamily::Unknown => {}
    }
    model.tokens = layout.path(SherpaRole::Tokens);
    model.num_threads = num_threads;
    model.model_type = model_type;
    config.decoding_method = Some("greedy_search".into());
    config
}

fn offline_recognizer(
    key: String,
    config: &sherpa_onnx::OfflineRecognizerConfig,
) -> Option<std::sync::Arc<sherpa_onnx::OfflineRecognizer>> {
    use sherpa_onnx::OfflineRecognizer;

    let mut cache = OFFLINE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = cache.as_mut().filter(|c| c.key == key) {
        cached.last_used = std::time::Instant::now();
        return Some(cached.recognizer.clone());
    }
    // Drop the previous model before loading the next one: two of them
    // would double the memory peak.
    *cache = None;

    let recognizer = std::sync::Arc::new(OfflineRecognizer::create(config)?);
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
    key: String,
    config: sherpa_onnx::OfflineRecognizerConfig,
    cmd_rx: std_mpsc::Receiver<SherpaCmd>,
    event_tx: tokio::sync::mpsc::Sender<AsrEvent>,
) {
    use crate::audio_split::split_at_quiet;
    use crate::transcript::append_segment;

    let Some(recognizer) = offline_recognizer(key, &config) else {
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
