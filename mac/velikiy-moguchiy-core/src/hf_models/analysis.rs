//! What a set of model files is and which engine runs it: sherpa-onnx
//! exports (see [`super::detect`]), whisper.cpp `ggml-*.bin` files and MLX
//! folders, and the formats this app recognizes but cannot run. The same
//! code reads a Hugging Face repository and a folder on disk.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use velikiy_moguchiy_asr::{SherpaFamily, SherpaLayout, SherpaRole};

use super::backend::Backend;
use super::detect::{self, Candidate, Rejection, RepoFile};

/// Sherpa-onnx candidates whose metadata is read; a repository with more is
/// a collection, and its author lists the main ones first.
pub const MAX_CANDIDATES: usize = 12;

/// The ONNX metadata of a file, or why it could not be read.
pub type MetadataRead = Result<BTreeMap<String, String>, String>;

/// What is known about where the files come from.
#[derive(Debug, Clone, Default)]
pub struct RepoHints {
    /// Repository or folder name.
    pub name: String,
    pub library: Option<String>,
    pub tags: Vec<String>,
    pub pipeline_tag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MlxKind {
    Qwen3Asr,
    Whisper,
    GigaAm,
    Other,
}

/// How the files of a variant are used, recorded in its manifest.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelLayout {
    Sherpa(SherpaLayout),
    WhisperCpp { model: String },
    Mlx { kind: MlxKind },
}

#[derive(Debug, Clone, Serialize)]
pub struct Variant {
    pub id: String,
    /// What tells it apart from the other variants of the repository.
    pub label: String,
    /// «GigaAM (трансдьюсер)».
    pub title: String,
    /// «потоковая: текст появляется во время речи».
    pub mode: String,
    pub backend: Backend,
    /// Whether this build can run it; `engine_note` says why not.
    pub engine_ready: bool,
    pub engine_note: Option<String>,
    pub family: Option<SherpaFamily>,
    pub streaming: bool,
    /// «int8», «fp32», «q5_0» or «int8 и fp32».
    pub precision: String,
    pub total_size: u64,
    pub files: Vec<RepoFile>,
    #[serde(skip)]
    pub layout: ModelLayout,
    /// The file whose metadata decided the type, if any.
    #[serde(skip)]
    pub primary: String,
}

impl Variant {
    pub fn sherpa_layout(&self) -> Option<&SherpaLayout> {
        match &self.layout {
            ModelLayout::Sherpa(layout) => Some(layout),
            _ => None,
        }
    }

    /// English-only Whisper sizes are named `*.en`.
    pub fn is_multilingual_whisper(&self) -> bool {
        let whisper = matches!(self.layout, ModelLayout::WhisperCpp { .. })
            || self.family == Some(SherpaFamily::Whisper)
            || matches!(
                self.layout,
                ModelLayout::Mlx {
                    kind: MlxKind::Whisper
                }
            );
        whisper && !self.label.to_ascii_lowercase().contains(".en") && !self.title.ends_with(".en")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct Analysis {
    /// The suggested variant first.
    pub variants: Vec<Variant>,
    pub rejected: Vec<Rejection>,
    /// Why there is nothing to install, when `variants` is empty.
    pub problem: Option<Problem>,
}

/// A verdict from file names alone, for search results.
#[derive(Debug, Clone)]
pub enum QuickLook {
    Runs(Backend),
    NeedsEngine(Backend, String),
    No(Problem),
}

fn engine_look(backend: Backend, compiled: &[&str]) -> QuickLook {
    let status = backend.status_in(compiled);
    match status.note {
        Some(note) if !status.ready => QuickLook::NeedsEngine(backend, note),
        _ => QuickLook::Runs(backend),
    }
}

/// What [`analyze`] would most likely find, without reading any file.
pub fn quick_look(files: &[RepoFile], hints: &RepoHints, compiled: &[&str]) -> QuickLook {
    let speech = hints
        .pipeline_tag
        .as_deref()
        .is_none_or(|t| t == "automatic-speech-recognition");
    if detect::candidates(files)
        .iter()
        .take(MAX_CANDIDATES)
        .any(detect::shape_is_complete)
    {
        return engine_look(Backend::SherpaOnnx, compiled);
    }
    if !whisper_cpp_files(files).is_empty() {
        return engine_look(Backend::WhisperCpp, compiled);
    }
    let has_config = files.iter().any(|f| f.path == "config.json");
    if speech && has_config && has_weights(files) && looks_mlx(hints) {
        if let Ok(kind) = mlx_kind(None, hints) {
            return match mlx_note(kind, compiled) {
                Some(note) => QuickLook::NeedsEngine(Backend::Mlx, note),
                None => QuickLook::Runs(Backend::Mlx),
            };
        }
    }
    QuickLook::No(explain_other(files, hints).unwrap_or_else(|| {
        let (code, message) = detect::explain(files, hints.pipeline_tag.as_deref(), &[]);
        Problem { code, message }
    }))
}

pub fn mode_text(streaming: bool) -> &'static str {
    if streaming {
        "потоковая: текст появляется во время речи"
    } else {
        "текст появляется после паузы"
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Files whose last bytes are needed: the main file of every sherpa-onnx
/// candidate that has a tokens file.
pub fn metadata_wanted(files: &[RepoFile]) -> Vec<String> {
    let mut wanted: Vec<String> = Vec::new();
    for candidate in detect::candidates(files).iter().take(MAX_CANDIDATES) {
        if candidate.roles.contains_key(&SherpaRole::Tokens) && !wanted.contains(&candidate.primary)
        {
            wanted.push(candidate.primary.clone());
        }
    }
    wanted
}

fn has_weights(files: &[RepoFile]) -> bool {
    files.iter().any(|f| {
        let name = basename(&f.path).to_ascii_lowercase();
        name.ends_with(".safetensors") || name == "weights.npz"
    })
}

fn looks_mlx(hints: &RepoHints) -> bool {
    hints.library.as_deref() == Some("mlx")
        || hints.tags.iter().any(|t| t == "mlx")
        || hints.name.to_ascii_lowercase().contains("mlx")
}

/// The `config.json` to read when the files may be an MLX model.
pub fn config_wanted(files: &[RepoFile], hints: &RepoHints) -> Option<String> {
    let config = files.iter().find(|f| f.path == "config.json")?;
    (has_weights(files) && (looks_mlx(hints) || config.size < 64 * 1024))
        .then(|| config.path.clone())
}

struct WhisperFile<'a> {
    file: &'a RepoFile,
    label: String,
}

fn whisper_cpp_files(files: &[RepoFile]) -> Vec<WhisperFile<'_>> {
    files
        .iter()
        .filter_map(|file| {
            let name = basename(&file.path);
            let lower = name.to_ascii_lowercase();
            let stem = lower.strip_prefix("ggml-")?.strip_suffix(".bin")?;
            if stem.is_empty()
                || ["gpt", "llama", "for-tests"]
                    .iter()
                    .any(|bad| lower.contains(bad))
            {
                return None;
            }
            Some(WhisperFile {
                file,
                label: name[5..name.len() - 4].to_string(),
            })
        })
        .collect()
}

/// Splits «large-v3-turbo-q5_0» into its size and quantization.
fn whisper_size_and_precision(label: &str) -> (String, String) {
    let lower = label.to_ascii_lowercase();
    for quant in ["q4_0", "q4_1", "q5_0", "q5_1", "q8_0", "f16", "f32"] {
        if let Some(size) = lower.strip_suffix(&format!("-{quant}")) {
            return (label[..size.len()].to_string(), quant.to_string());
        }
    }
    (label.to_string(), "f16".to_string())
}

/// Turbo first, then the larger sizes, the smaller quantization of each;
/// English-only and diarizing builds last.
fn whisper_preference(label: &str) -> i32 {
    let lower = label.to_ascii_lowercase();
    let size = if lower.contains("large-v3-turbo") || lower.contains("turbo") {
        100
    } else if lower.contains("large-v3") {
        80
    } else if lower.contains("large") {
        60
    } else if lower.contains("medium") {
        50
    } else if lower.contains("small") {
        40
    } else if lower.contains("base") {
        30
    } else if lower.contains("tiny") {
        20
    } else {
        10
    };
    let (_, quant) = whisper_size_and_precision(label);
    let quant = match quant.as_str() {
        "q5_0" | "q5_1" => 5,
        "q8_0" => 4,
        _ => 3,
    };
    let mut score = size * 10 + quant;
    if lower.contains(".en") {
        score -= 2000;
    }
    if lower.contains("tdrz") {
        score -= 1000;
    }
    score
}

fn mlx_kind(config: Option<&serde_json::Value>, hints: &RepoHints) -> Result<MlxKind, String> {
    let model_type = config
        .and_then(|c| c.get("model_type"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = hints.name.to_ascii_lowercase();
    let kind = match model_type.as_str() {
        "qwen3_asr" => MlxKind::Qwen3Asr,
        "whisper" => MlxKind::Whisper,
        "gigaam" => MlxKind::GigaAm,
        "" if name.contains("gigaam") => MlxKind::GigaAm,
        "" if name.contains("whisper") => MlxKind::Whisper,
        "" if name.contains("qwen3-asr") => MlxKind::Qwen3Asr,
        "" if hints.pipeline_tag.as_deref() == Some("automatic-speech-recognition") => {
            MlxKind::Other
        }
        "" => return Err("Это MLX-модель, но не понять, что она делает.".into()),
        other => {
            let speech = [
                "asr",
                "speech",
                "wav2vec",
                "parakeet",
                "conformer",
                "canary",
            ]
            .iter()
            .any(|w| other.contains(w));
            if !speech {
                return Err(format!(
                    "Это не модель распознавания речи: MLX-модель типа «{other}»."
                ));
            }
            MlxKind::Other
        }
    };
    Ok(kind)
}

fn mlx_note(kind: MlxKind, compiled: &[&str]) -> Option<String> {
    let engine = Backend::Mlx.status_in(compiled);
    let what = match kind {
        MlxKind::Qwen3Asr => return engine.note,
        MlxKind::Whisper => "Whisper",
        MlxKind::GigaAm => "GigaAM",
        MlxKind::Other => "модели этого типа",
    };
    let missing = if engine.ready {
        "".to_string()
    } else {
        " К тому же в этой сборке нет самого движка MLX: он пока не собирается в Xcode 26.3."
            .to_string()
    };
    Some(format!(
        "Это MLX-модель ({what}). MLX-движок приложения умеет только Qwen3-ASR, {what} для MLX он пока не запускает.{missing}"
    ))
}

fn sherpa_variant(
    candidate: &Candidate,
    classified: detect::Classified,
    files: &[RepoFile],
    compiled: &[&str],
) -> Variant {
    let mut wanted: Vec<String> = classified.layout.files.values().cloned().collect();
    for extra in &candidate.extra {
        if !wanted.contains(extra) {
            wanted.push(extra.clone());
        }
    }
    if let Some(licence) = files
        .iter()
        .find(|f| is_root_license(&f.path) && f.size <= MAX_LICENSE_BYTES)
    {
        wanted.push(licence.path.clone());
    }
    let chosen: Vec<RepoFile> = files
        .iter()
        .filter(|f| wanted.contains(&f.path))
        .cloned()
        .collect();
    let engine = Backend::SherpaOnnx.status_in(compiled);
    Variant {
        id: candidate.key.clone(),
        label: candidate.label.clone(),
        title: classified.title,
        mode: mode_text(classified.layout.streaming).into(),
        backend: Backend::SherpaOnnx,
        engine_ready: engine.ready,
        engine_note: engine.note,
        family: Some(classified.layout.family),
        streaming: classified.layout.streaming,
        precision: candidate.precision(),
        total_size: chosen.iter().map(|f| f.size).sum(),
        files: chosen,
        layout: ModelLayout::Sherpa(classified.layout),
        primary: candidate.primary.clone(),
    }
}

pub const MAX_LICENSE_BYTES: u64 = 64 * 1024;

pub fn is_root_license(path: &str) -> bool {
    !path.contains('/')
        && matches!(
            path.to_ascii_lowercase().as_str(),
            "license" | "license.txt" | "license.md" | "licence"
        )
}

/// Every model in `files`. `metadata` holds what [`metadata_wanted`] asked
/// for, `config` the parsed [`config_wanted`] file; `compiled` lists the
/// providers of this build.
pub fn analyze(
    files: &[RepoFile],
    metadata: &HashMap<String, MetadataRead>,
    config: Option<&serde_json::Value>,
    hints: &RepoHints,
    compiled: &[&str],
) -> Analysis {
    let mut analysis = Analysis::default();

    let empty = BTreeMap::new();
    let mut sherpa = Vec::new();
    for candidate in detect::candidates(files).iter().take(MAX_CANDIDATES) {
        let meta = match metadata.get(&candidate.primary) {
            Some(Ok(meta)) => meta,
            Some(Err(error)) => {
                analysis.rejected.push(Rejection {
                    label: candidate.label.clone(),
                    reason: detect::RejectReason::Unreadable,
                    message: format!("Не удалось прочитать {}: {error}", candidate.primary),
                });
                continue;
            }
            None => &empty,
        };
        match detect::classify(candidate, meta) {
            Ok(classified) => sherpa.push(sherpa_variant(candidate, classified, files, compiled)),
            Err(rejection) => analysis.rejected.push(rejection),
        }
    }
    sherpa.sort_by_key(|v| -sherpa_preference(v));
    analysis.variants.extend(sherpa);

    let mut whisper: Vec<WhisperFile> = whisper_cpp_files(files);
    whisper.sort_by_key(|w| -whisper_preference(&w.label));
    let engine = Backend::WhisperCpp.status_in(compiled);
    for w in whisper {
        let (size, precision) = whisper_size_and_precision(&w.label);
        analysis.variants.push(Variant {
            id: w.file.path.clone(),
            label: w.label.clone(),
            title: format!("Whisper {size}"),
            mode: mode_text(false).into(),
            backend: Backend::WhisperCpp,
            engine_ready: engine.ready,
            engine_note: engine.note.clone(),
            family: None,
            streaming: false,
            precision,
            total_size: w.file.size,
            files: vec![w.file.clone()],
            layout: ModelLayout::WhisperCpp {
                model: w.file.path.clone(),
            },
            primary: w.file.path.clone(),
        });
    }

    if analysis.variants.is_empty()
        && has_weights(files)
        && files.iter().any(|f| f.path == "config.json")
    {
        let mlx = looks_mlx(hints)
            || config
                .and_then(|c| c.get("framework"))
                .and_then(|f| f.as_str())
                .is_some_and(|f| f.eq_ignore_ascii_case("mlx"));
        if mlx {
            match mlx_kind(config, hints) {
                Ok(kind) => {
                    let note = mlx_note(kind, compiled);
                    let title = match kind {
                        MlxKind::Qwen3Asr => "Qwen3-ASR (MLX)",
                        MlxKind::Whisper => "Whisper (MLX)",
                        MlxKind::GigaAm => "GigaAM (MLX)",
                        MlxKind::Other => "Модель MLX",
                    };
                    let chosen: Vec<RepoFile> = files
                        .iter()
                        .filter(|f| {
                            let name = basename(&f.path).to_ascii_lowercase();
                            !name.starts_with('.') && !name.ends_with(".md")
                        })
                        .cloned()
                        .collect();
                    analysis.variants.push(Variant {
                        id: "mlx".into(),
                        label: String::new(),
                        title: title.into(),
                        mode: mode_text(false).into(),
                        backend: Backend::Mlx,
                        engine_ready: note.is_none(),
                        engine_note: note,
                        family: None,
                        streaming: false,
                        precision: String::new(),
                        total_size: chosen.iter().map(|f| f.size).sum(),
                        files: chosen,
                        layout: ModelLayout::Mlx { kind },
                        primary: "config.json".into(),
                    });
                }
                Err(message) => {
                    analysis.problem = Some(Problem {
                        code: "not_asr".into(),
                        message,
                    })
                }
            }
        }
    }

    if analysis.variants.is_empty() && analysis.problem.is_none() {
        analysis.problem = Some(explain_other(files, hints).unwrap_or_else(|| {
            let (code, message) =
                detect::explain(files, hints.pipeline_tag.as_deref(), &analysis.rejected);
            Problem { code, message }
        }));
    }
    analysis
}

fn sherpa_preference(variant: &Variant) -> i32 {
    let label = variant.label.to_ascii_lowercase();
    let mut score = 0;
    if label.contains("e2e") || label.contains("punct") {
        score += 2;
    }
    if matches!(
        variant.family,
        Some(SherpaFamily::Transducer | SherpaFamily::NemoTransducer)
    ) {
        score += 1;
    }
    score
}

/// Formats recognized by their files that no engine of this app runs.
fn explain_other(files: &[RepoFile], hints: &RepoHints) -> Option<Problem> {
    let names: Vec<String> = files
        .iter()
        .map(|f| basename(&f.path).to_ascii_lowercase())
        .collect();
    let has = |n: &str| names.iter().any(|x| x == n);
    let name = hints.name.to_ascii_lowercase();
    let ct2 = hints.library.as_deref() == Some("ctranslate2")
        || hints.tags.iter().any(|t| t == "ctranslate2")
        || name.contains("faster-whisper")
        || name.contains("ct2");
    if has("model.bin")
        && has("config.json")
        && (has("vocabulary.json") || has("vocabulary.txt") || ct2)
        && ct2
    {
        return Some(Problem {
            code: "faster_whisper".into(),
            message: "Формат faster-whisper (CTranslate2) не поддерживается: он работает только из Python. Подойдёт версия той же модели для whisper.cpp (файл ggml-*.bin).".into(),
        });
    }
    let pytorch = names.iter().any(|n| {
        [".nemo", ".safetensors", ".pt", ".pth", ".ckpt"]
            .iter()
            .any(|ext| n.ends_with(ext))
            || n == "pytorch_model.bin"
    });
    if names.iter().any(|n| n.ends_with(".gguf"))
        && !pytorch
        && !names.iter().any(|n| n.ends_with(".onnx"))
    {
        return Some(Problem {
            code: "gguf".into(),
            message: "Файлы GGUF — это формат для transcribe.cpp и llama.cpp, приложение его не запускает. Для Whisper подойдут файлы ggml-*.bin от whisper.cpp.".into(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, u64)]) -> Vec<RepoFile> {
        list.iter()
            .map(|(p, s)| RepoFile {
                path: p.to_string(),
                size: *s,
                sha256: Some("0".repeat(64)),
                git_sha1: None,
            })
            .collect()
    }

    const ALL: &[&str] = &["sherpa-onnx", "whisper-cpp", "mlx"];
    const RELEASE: &[&str] = &["sherpa-onnx", "whisper-cpp", "apple-speech"];

    #[test]
    fn whisper_cpp_files_become_variants_turbo_q5_first() {
        let repo = files(&[
            ("ggml-tiny.en.bin", 77),
            ("ggml-large-v3.bin", 3095),
            ("ggml-large-v3-turbo.bin", 1624),
            ("ggml-large-v3-turbo-q8_0.bin", 874),
            ("ggml-large-v3-turbo-q5_0.bin", 574),
            ("ggml-base-encoder.mlmodelc.zip", 30),
            ("for-tests-ggml-tiny.bin", 1),
        ]);
        let a = analyze(&repo, &HashMap::new(), None, &RepoHints::default(), RELEASE);
        let labels: Vec<&str> = a.variants.iter().map(|v| v.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "large-v3-turbo-q5_0",
                "large-v3-turbo-q8_0",
                "large-v3-turbo",
                "large-v3",
                "tiny.en"
            ]
        );
        let first = &a.variants[0];
        assert_eq!(
            (first.title.as_str(), first.precision.as_str()),
            ("Whisper large-v3-turbo", "q5_0")
        );
        assert_eq!(
            first.layout,
            ModelLayout::WhisperCpp {
                model: "ggml-large-v3-turbo-q5_0.bin".into()
            }
        );
        assert!(first.engine_ready && first.is_multilingual_whisper());
        assert!(!a.variants[4].is_multilingual_whisper());
        let without = analyze(
            &repo,
            &HashMap::new(),
            None,
            &RepoHints::default(),
            &["sherpa-onnx"],
        );
        assert!(!without.variants[0].engine_ready);
        assert!(without.variants[0]
            .engine_note
            .as_deref()
            .unwrap()
            .contains("whisper.cpp"));
    }

    #[test]
    fn mlx_models_are_found_and_say_what_is_missing() {
        let repo = files(&[
            ("README.md", 1),
            ("config.json", 2),
            ("tokenizer.model", 3),
            ("weights.safetensors", 900),
        ]);
        let hints = RepoHints {
            name: "GigaAM-v3-e2e-rnnt-mlx".into(),
            library: Some("mlx".into()),
            ..Default::default()
        };
        let config = serde_json::json!({"model_type": "gigaam", "framework": "mlx"});
        let a = analyze(&repo, &HashMap::new(), Some(&config), &hints, RELEASE);
        let v = &a.variants[0];
        assert_eq!(
            (v.backend, v.title.as_str(), v.engine_ready),
            (Backend::Mlx, "GigaAM (MLX)", false)
        );
        let note = v.engine_note.as_deref().unwrap();
        assert!(
            note.contains("только Qwen3-ASR") && note.contains("Xcode"),
            "{note}"
        );
        assert_eq!(v.files.len(), 3, "README is not part of the model");

        let qwen = serde_json::json!({"model_type": "qwen3_asr"});
        let a = analyze(&repo, &HashMap::new(), Some(&qwen), &hints, ALL);
        assert!(a.variants[0].engine_ready);
        let a = analyze(&repo, &HashMap::new(), Some(&qwen), &hints, RELEASE);
        assert!(a.variants[0]
            .engine_note
            .as_deref()
            .unwrap()
            .contains("Xcode"));

        let llm = serde_json::json!({"model_type": "qwen3"});
        let a = analyze(&repo, &HashMap::new(), Some(&llm), &hints, ALL);
        assert!(a.variants.is_empty());
        assert_eq!(a.problem.unwrap().code, "not_asr");
    }

    #[test]
    fn faster_whisper_and_gguf_are_named() {
        let ct2 = files(&[
            ("config.json", 2),
            ("model.bin", 1600),
            ("tokenizer.json", 2),
            ("vocabulary.json", 1),
        ]);
        let hints = RepoHints {
            name: "faster-whisper-large-v3-turbo".into(),
            ..Default::default()
        };
        let a = analyze(&ct2, &HashMap::new(), None, &hints, ALL);
        assert_eq!(a.problem.unwrap().code, "faster_whisper");
        let gguf = files(&[("whisper-large-v3-turbo-Q5_K_M.gguf", 600)]);
        let a = analyze(&gguf, &HashMap::new(), None, &RepoHints::default(), ALL);
        assert_eq!(a.problem.unwrap().code, "gguf");
    }
}
