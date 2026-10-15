//! Transcription of whole audio files (voice messages, recordings, video
//! soundtracks) with the configured provider, for the CLI's `transcribe` and
//! the app's «Расшифровать файл…».
//!
//! The audio is cut at pauses into pieces of at most [`CHUNK_MAX_SECS`], each
//! sent as its own provider session: cloud services cap the length of one
//! request, local offline models degrade on long input, and the piece
//! boundaries double as subtitle timestamps.

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use futures_util::stream::{self, StreamExt};
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use velikiy_moguchiy_asr::audio_split::{is_silent, split_at_quiet};
use velikiy_moguchiy_asr::{AsrEvent, TranscriptAggregator};

use crate::config::Config;

pub const SAMPLE_RATE: u32 = 16_000;
const CHUNK_MIN_SECS: f32 = 3.0;
pub const CHUNK_MAX_SECS: f32 = 20.0;
/// A piece is at most 20 s of audio; a provider that has not answered in
/// two minutes is stuck.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(120);
/// Cloud requests run a few at a time; local engines share one CPU-bound model.
const CLOUD_PARALLEL_CHUNKS: usize = 3;
const LOCAL_PROVIDERS: &[&str] = &["mlx", "sherpa-onnx", "apple-speech", "wetype"];
/// Subtitle cues longer than this are split at sentence ends.
const MAX_CUE_SECS: f64 = 7.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Segment {
    /// Seconds from the start of the file.
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileTranscript {
    pub provider: String,
    pub duration: f64,
    pub text: String,
    pub segments: Vec<Segment>,
    pub srt: String,
}

impl FileTranscript {
    pub fn new(provider: &str, sample_count: usize, segments: Vec<Segment>) -> Self {
        Self {
            provider: provider.to_string(),
            duration: sample_count as f64 / SAMPLE_RATE as f64,
            text: join_segments(&segments),
            srt: to_srt(&segments),
            segments,
        }
    }
}

/// Converts 16-bit little-endian PCM bytes to samples.
pub fn pcm_bytes_to_samples(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Transcribes 16 kHz mono PCM with `provider_name`. `progress(done, total)`
/// is called after each piece with speech; pieces of silence are skipped.
/// `voice_commands` asks Qwen Omni to mark spoken formatting commands, for
/// re-transcribing dictations whose text then goes through `commands`.
pub async fn transcribe_pcm(
    cfg: &Config,
    provider_name: &str,
    app: Option<&str>,
    voice_commands: bool,
    pcm: &[i16],
    cancel: &CancellationToken,
    progress: &(dyn Fn(usize, usize) + Send + Sync),
) -> Result<Vec<Segment>, String> {
    let samples: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32768.0).collect();
    let pieces: Vec<_> = split_at_quiet(&samples, SAMPLE_RATE, CHUNK_MIN_SECS, CHUNK_MAX_SECS)
        .into_iter()
        .filter(|r| !is_silent(&samples[r.clone()], SAMPLE_RATE))
        .collect();
    let total = pieces.len();
    progress(0, total);
    if total == 0 {
        return Ok(Vec::new());
    }

    let user_dictionary =
        crate::dictionary::load_entries(&crate::config::resolve_dictionary_path(cfg))
            .unwrap_or_default();
    let terms: Vec<String> = user_dictionary.iter().map(|e| e.term.clone()).collect();
    let instructions = omni_instructions(cfg, provider_name, app, voice_commands, &user_dictionary);

    let parallel = if LOCAL_PROVIDERS.contains(&provider_name) {
        1
    } else {
        CLOUD_PARALLEL_CHUNKS
    };
    let done = AtomicUsize::new(0);
    let results: Vec<Result<Option<Segment>, String>> = stream::iter(pieces)
        .map(|range| {
            let bytes: Vec<u8> = pcm[range.clone()]
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect();
            let terms = &terms;
            let instructions = &instructions;
            let done = &done;
            async move {
                if cancel.is_cancelled() {
                    return Err("cancelled".to_string());
                }
                let mut text = None;
                let mut last_error = String::new();
                for _attempt in 0..2 {
                    let run = transcribe_piece(cfg, provider_name, terms, instructions, &bytes);
                    let outcome = tokio::select! {
                        _ = cancel.cancelled() => return Err("cancelled".to_string()),
                        r = tokio::time::timeout(CHUNK_TIMEOUT, run) => r,
                    };
                    match outcome {
                        Ok(Ok(t)) => {
                            text = Some(t);
                            break;
                        }
                        Ok(Err(e)) => last_error = e,
                        Err(_) => last_error = "provider did not answer in time".into(),
                    }
                    log::warn!("file transcription piece failed, retrying: {last_error}");
                }
                let text = text.ok_or(last_error)?;
                progress(done.fetch_add(1, Ordering::SeqCst) + 1, total);
                Ok((!text.trim().is_empty()).then(|| Segment {
                    start: range.start as f64 / SAMPLE_RATE as f64,
                    end: range.end as f64 / SAMPLE_RATE as f64,
                    text: text.trim().to_string(),
                }))
            }
        })
        .buffered(parallel)
        .collect()
        .await;

    let mut segments = Vec::new();
    for result in results {
        if let Some(segment) = result? {
            segments.push(segment);
        }
    }
    Ok(segments)
}

/// Qwen Omni takes dictation instructions: the output mode of `app` (none:
/// the default one).
fn omni_instructions(
    cfg: &Config,
    provider_name: &str,
    app: Option<&str>,
    voice_commands: bool,
    user_dictionary: &[crate::dictionary::DictEntry],
) -> Option<String> {
    if provider_name != "qwen"
        || !velikiy_moguchiy_asr::QwenAsrProvider::is_omni_model(&cfg.asr.qwen.model)
    {
        return None;
    }
    let starter = crate::dictionary::load_starter_sections(
        &crate::config::config_dir().join(crate::dictionary::STARTER_FILE_NAME),
    );
    Some(crate::modes::dictation_instructions_for(
        &cfg.asr.qwen,
        &crate::modes::DictationEnvironment {
            bundle_id: app,
            user_dictionary,
            starter_dictionary: &starter,
            field: Default::default(),
            voice_commands,
        },
    ))
}

async fn transcribe_piece(
    cfg: &Config,
    provider_name: &str,
    terms: &[String],
    instructions: &Option<String>,
    pcm: &[u8],
) -> Result<String, String> {
    let (mut config, mut asr) = crate::asr_factory::create_asr_provider(cfg, provider_name, terms);
    if instructions.is_some() {
        config.instructions = instructions.clone();
    }
    asr.connect(&config)
        .await
        .map_err(|e| format!("connect: {e}"))?;

    // 20 ms frames, sent as fast as the provider takes them.
    for frame in pcm.chunks(640) {
        if let Err(e) = asr.send_audio(frame).await {
            asr.close().await.ok();
            return Err(format!("send_audio: {e}"));
        }
    }
    if let Err(e) = asr.finish_input().await {
        asr.close().await.ok();
        return Err(format!("finish_input: {e}"));
    }

    let mut aggregator = TranscriptAggregator::new();
    let outcome = loop {
        match asr.next_event().await {
            Ok(AsrEvent::Interim(text)) => aggregator.update_interim(&text),
            Ok(AsrEvent::Definite(text)) => aggregator.update_definite(&text),
            Ok(AsrEvent::Final(text)) => {
                aggregator.update_final(&text);
                break Ok(());
            }
            Ok(AsrEvent::Closed(_)) => break Ok(()),
            Ok(AsrEvent::Error(msg)) => break Err(format!("ASR error: {msg}")),
            Ok(_) => {}
            Err(e) => break Err(format!("event: {e}")),
        }
    };
    asr.close().await.ok();
    outcome.map(|()| aggregator.best_text())
}

/// The transcript as plain text: pieces joined by spaces, with a paragraph
/// break wherever a stretch of silence was skipped.
pub fn join_segments(segments: &[Segment]) -> String {
    let mut text = String::new();
    let mut previous_end = None;
    for segment in segments {
        if let Some(end) = previous_end {
            if segment.start - end > 0.01 {
                text.push_str("\n\n");
            }
        }
        velikiy_moguchiy_asr::append_segment(&mut text, &segment.text);
        previous_end = Some(segment.end);
    }
    text
}

/// SubRip subtitles. Pieces are up to 20 s long, so a long piece is split at
/// sentence ends with time shared out by sentence length.
pub fn to_srt(segments: &[Segment]) -> String {
    let mut cues: Vec<Segment> = Vec::new();
    for segment in segments {
        let sentences = split_sentences(&segment.text);
        if segment.end - segment.start <= MAX_CUE_SECS || sentences.len() < 2 {
            cues.push(segment.clone());
            continue;
        }
        let total_chars: usize = sentences.iter().map(|s| s.chars().count()).sum();
        let span = segment.end - segment.start;
        let mut start = segment.start;
        let mut chars_before = 0;
        for (i, sentence) in sentences.iter().enumerate() {
            chars_before += sentence.chars().count();
            let end = if i == sentences.len() - 1 {
                segment.end
            } else {
                segment.start + span * chars_before as f64 / total_chars as f64
            };
            cues.push(Segment {
                start,
                end,
                text: sentence.clone(),
            });
            start = end;
        }
    }

    let mut srt = String::new();
    for (i, cue) in cues.iter().enumerate() {
        srt.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            srt_time(cue.start),
            srt_time(cue.end),
            cue.text
        ));
    }
    srt
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        let ends_sentence = matches!(c, '.' | '!' | '?' | '…')
            && chars.peek().is_some_and(|next| next.is_whitespace());
        if ends_sentence {
            sentences.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }
    sentences
}

fn srt_time(seconds: f64) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

// ─── FFI ────────────────────────────────────────────────────────────

/// `done` of `total` pieces with speech are transcribed.
pub type FileProgressCallback = extern "C" fn(ctx: *mut c_void, done: u32, total: u32);
/// Called once per job with a [`FileTranscript`] as JSON, or `{"error": "…"}`,
/// or `{"cancelled": true}`.
pub type FileDoneCallback = extern "C" fn(ctx: *mut c_void, json: *const c_char);

struct FileCallbacks {
    ctx: *mut c_void,
    progress: FileProgressCallback,
    done: FileDoneCallback,
}
// The app's callbacks hop to the main queue themselves.
unsafe impl Send for FileCallbacks {}
unsafe impl Sync for FileCallbacks {}

impl FileCallbacks {
    fn finish(&self, json: &serde_json::Value) {
        if let Ok(cstr) = CString::new(json.to_string()) {
            (self.done)(self.ctx, cstr.as_ptr());
        }
    }
}

static FILE_JOBS: Mutex<Option<HashMap<u64, CancellationToken>>> = Mutex::new(None);
static NEXT_FILE_JOB: AtomicU64 = AtomicU64::new(1);

/// File jobs run apart from dictation, on their own small runtime, so a long
/// file never delays a session.
fn file_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("file-transcription")
            .enable_all()
            .build()
            .expect("file transcription runtime")
    })
}

/// Reads an Ogg Opus file (Telegram and WhatsApp voice messages) to PCM.
/// Other formats are decoded by the caller.
pub fn decode_ogg_opus_file(path: &std::path::Path) -> Result<Vec<i16>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    if !velikiy_moguchiy_asr::ogg_opus::is_ogg(&bytes) {
        return Err("unsupported audio format".into());
    }
    velikiy_moguchiy_asr::ogg_opus::decode_ogg_opus(&bytes)
}

/// Starts transcribing a file with the provider from config.yaml. Pass the
/// audio as 16 kHz mono PCM in `pcm`/`pcm_len` (samples), or a null `pcm`
/// and the `path` of an Ogg Opus file. Returns a job id for
/// [`sp_file_transcribe_cancel`], or 0 when nothing started. `done_cb` is
/// called exactly once for every started job, on a background thread.
///
/// # Safety
/// `path` must be null or a valid C string; `pcm` must be null or point to
/// `pcm_len` readable samples. Both are copied before returning.
#[no_mangle]
pub unsafe extern "C" fn sp_file_transcribe_start(
    path: *const c_char,
    pcm: *const i16,
    pcm_len: usize,
    progress_cb: FileProgressCallback,
    done_cb: FileDoneCallback,
    ctx: *mut c_void,
) -> u64 {
    let audio = if !pcm.is_null() && pcm_len > 0 {
        Some(unsafe { std::slice::from_raw_parts(pcm, pcm_len) }.to_vec())
    } else {
        None
    };
    let path = if path.is_null() {
        None
    } else {
        unsafe { CStr::from_ptr(path) }
            .to_str()
            .ok()
            .map(std::path::PathBuf::from)
    };
    if audio.is_none() && path.is_none() {
        return 0;
    }

    let job = NEXT_FILE_JOB.fetch_add(1, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    FILE_JOBS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(job, cancel.clone());
    let callbacks = std::sync::Arc::new(FileCallbacks {
        ctx,
        progress: progress_cb,
        done: done_cb,
    });

    file_runtime().spawn(async move {
        let result = run_file_job(audio, path, &cancel, &callbacks).await;
        FILE_JOBS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_with(HashMap::new)
            .remove(&job);
        let json = match result {
            Ok(transcript) => serde_json::to_value(&transcript).unwrap_or_default(),
            Err(_) if cancel.is_cancelled() => serde_json::json!({ "cancelled": true }),
            Err(e) => serde_json::json!({ "error": e }),
        };
        callbacks.finish(&json);
    });
    job
}

async fn run_file_job(
    audio: Option<Vec<i16>>,
    path: Option<std::path::PathBuf>,
    cancel: &CancellationToken,
    callbacks: &std::sync::Arc<FileCallbacks>,
) -> Result<FileTranscript, String> {
    let pcm = match (audio, path) {
        (Some(pcm), _) => pcm,
        (None, Some(path)) => tokio::task::spawn_blocking(move || decode_ogg_opus_file(&path))
            .await
            .map_err(|e| format!("decode: {e}"))??,
        (None, None) => unreachable!("checked before spawning"),
    };
    let cfg = crate::config::load_config().map_err(|e| format!("load config: {e}"))?;
    let provider = cfg.asr.provider.clone();
    if !crate::asr_factory::supported_providers().contains(&provider.as_str()) {
        return Err(format!("unsupported provider '{provider}'"));
    }
    let progress_callbacks = callbacks.clone();
    let progress = move |done: usize, total: usize| {
        (progress_callbacks.progress)(progress_callbacks.ctx, done as u32, total as u32);
    };
    let segments = transcribe_pcm(&cfg, &provider, None, false, &pcm, cancel, &progress).await?;
    Ok(FileTranscript::new(&provider, pcm.len(), segments))
}

/// Stops a job started by [`sp_file_transcribe_start`]; its `done_cb` then
/// reports `{"cancelled": true}`. Returns 1 when the job was running.
#[no_mangle]
pub extern "C" fn sp_file_transcribe_cancel(job: u64) -> i32 {
    let jobs = FILE_JOBS.lock().unwrap_or_else(|e| e.into_inner());
    match jobs.as_ref().and_then(|map| map.get(&job)) {
        Some(token) => {
            token.cancel();
            1
        }
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64, text: &str) -> Segment {
        Segment {
            start,
            end,
            text: text.into(),
        }
    }

    #[test]
    fn srt_formats_times_and_numbers_cues() {
        let srt = to_srt(&[seg(0.0, 4.5, "Привет."), seg(4.5, 3725.25, "Пока")]);
        assert_eq!(
            srt,
            "1\n00:00:00,000 --> 00:00:04,500\nПривет.\n\n\
             2\n00:00:04,500 --> 01:02:05,250\nПока\n\n"
        );
    }

    #[test]
    fn long_pieces_split_into_sentence_cues() {
        let srt = to_srt(&[seg(
            10.0,
            30.0,
            "Раз два. Три четыре пять шесть семь восемь?",
        )]);
        let cues: Vec<&str> = srt.split("\n\n").filter(|c| !c.is_empty()).collect();
        assert_eq!(cues.len(), 2);
        assert!(cues[0].ends_with("Раз два."));
        assert!(cues[1].contains("--> 00:00:30,000"));
        // The short first sentence gets the smaller share of the 20 s.
        assert!(cues[0].contains("00:00:10,000 --> 00:00:13,8"));
    }

    #[test]
    fn short_pieces_stay_whole() {
        let srt = to_srt(&[seg(0.0, 6.0, "Да. Нет.")]);
        assert_eq!(srt.matches("-->").count(), 1);
    }

    #[test]
    fn skipped_silence_starts_a_paragraph() {
        let text = join_segments(&[
            seg(0.0, 10.0, "Первая часть"),
            seg(10.0, 20.0, "вторая."),
            seg(35.0, 40.0, "После паузы."),
        ]);
        assert_eq!(text, "Первая часть вторая.\n\nПосле паузы.");
    }

    #[test]
    fn pcm_bytes_round_trip() {
        assert_eq!(pcm_bytes_to_samples(&[1, 0, 0xff, 0xff, 9]), vec![1, -1]);
    }

    #[tokio::test]
    async fn silence_needs_no_provider() {
        let calls = AtomicUsize::new(0);
        let segments = transcribe_pcm(
            &Config::default(),
            "qwen",
            None,
            false,
            &vec![0i16; SAMPLE_RATE as usize * 30],
            &CancellationToken::new(),
            &|_, _| {
                calls.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await
        .unwrap();
        assert!(segments.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
