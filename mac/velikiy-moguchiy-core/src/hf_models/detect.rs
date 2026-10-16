//! Which sherpa-onnx models a repository holds: ONNX files are grouped into
//! candidate models by name, and the metadata inside each candidate's main
//! file decides its family. A candidate without sherpa-onnx metadata is
//! refused: sherpa-onnx exits the process on a model it cannot read.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use velikiy_moguchiy_asr::{SherpaFamily, SherpaLayout, SherpaRole};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepoFile {
    pub path: String,
    pub size: u64,
    /// Git LFS SHA-256; `None` for a file kept in git itself.
    pub sha256: Option<String>,
    /// Git blob id of a file kept in git itself.
    pub git_sha1: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Precision {
    Int8,
    Fp32,
    Fp16,
}

#[derive(Debug, Clone)]
struct OnnxName<'a> {
    path: &'a str,
    dir: &'a str,
    prefix: String,
    suffix: String,
    role: SherpaRole,
    precision: Precision,
}

/// ONNX files that may form one model, before their metadata is read.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub key: String,
    /// Tells candidates of one repository apart; empty for a lone model.
    pub label: String,
    pub roles: BTreeMap<SherpaRole, String>,
    /// External weights (`encoder.weights`, `model.onnx.data`) of chosen files.
    pub extra: Vec<String>,
    /// The file whose metadata decides the family.
    pub primary: String,
    precisions: Vec<Precision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    Incomplete,
    NotSpeechRecognition,
    UnknownType,
    UnsupportedMode,
    NoTokens,
    NoSherpaMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rejection {
    pub label: String,
    pub reason: RejectReason,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Classified {
    pub layout: SherpaLayout,
    /// «GigaAM (трансдьюсер)».
    pub title: String,
}

const SEPARATORS: &[char] = &['-', '_', '.'];

fn split_dir(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

fn strip_precision(stem: &str) -> (&str, Precision) {
    let lower = stem.to_ascii_lowercase();
    for (tag, precision) in [
        ("int8", Precision::Int8),
        ("fp16", Precision::Fp16),
        ("fp32", Precision::Fp32),
    ] {
        for sep in SEPARATORS {
            if lower.ends_with(&format!("{sep}{tag}")) {
                return (&stem[..stem.len() - tag.len() - 1], precision);
            }
        }
    }
    (stem, Precision::Fp32)
}

fn parse_onnx(path: &str) -> Option<OnnxName<'_>> {
    let (dir, file) = split_dir(path);
    if !file.to_ascii_lowercase().ends_with(".onnx") || file.len() <= 5 {
        return None;
    }
    let (stem, precision) = strip_precision(&file[..file.len() - 5]);

    let mut words: Vec<(usize, usize, String)> = Vec::new();
    let mut start = None;
    for (i, ch) in stem
        .char_indices()
        .chain(std::iter::once((stem.len(), '-')))
    {
        if SEPARATORS.contains(&ch) {
            if let Some(s) = start.take() {
                words.push((s, i, stem[s..i].to_ascii_lowercase()));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }

    let mut found = None;
    for (i, (_, _, word)) in words.iter().enumerate() {
        let next = words.get(i + 1).map(|w| w.2.as_str());
        let decode_next = matches!(next, Some("decode" | "decoder"));
        let role = match word.as_str() {
            "uncached" if decode_next => Some((SherpaRole::UncachedDecoder, i + 1)),
            "cached" if decode_next => Some((SherpaRole::CachedDecoder, i + 1)),
            "encoder" | "encode" => Some((SherpaRole::Encoder, i)),
            "decoder" | "decode" => Some((SherpaRole::Decoder, i)),
            "joiner" | "joint" => Some((SherpaRole::Joiner, i)),
            "preprocess" | "preprocessor" => Some((SherpaRole::Preprocessor, i)),
            "model" => Some((SherpaRole::Model, i)),
            _ => None,
        };
        if let Some((role, last)) = role {
            found = Some((role, words[i].0, words[last].1));
            break;
        }
    }
    let (role, prefix, suffix) = match found {
        Some((role, from, to)) => (
            role,
            stem[..from].trim_end_matches(SEPARATORS).to_string(),
            stem[to..].trim_start_matches(SEPARATORS).to_string(),
        ),
        None => (SherpaRole::Model, stem.to_string(), String::new()),
    };
    Some(OnnxName {
        path,
        dir,
        prefix,
        suffix,
        role,
        precision,
    })
}

/// `tokens.txt` or `<prefix>-tokens.txt`: (dir, prefix, path).
fn parse_tokens(path: &str) -> Option<(&str, String, &str)> {
    let (dir, file) = split_dir(path);
    let lower = file.to_ascii_lowercase();
    if lower == "tokens.txt" {
        return Some((dir, String::new(), path));
    }
    let stem = lower.strip_suffix("tokens.txt")?;
    let prefix = stem.strip_suffix(SEPARATORS)?;
    (!prefix.is_empty()).then(|| (dir, prefix.to_string(), path))
}

fn pick<'n, 'a>(names: &[&'n OnnxName<'a>], order: &[Precision]) -> &'n OnnxName<'a> {
    order
        .iter()
        .find_map(|p| names.iter().find(|n| n.precision == *p))
        .unwrap_or(&names[0])
}

/// Candidate models in `files`, in the order the repository lists them.
pub fn candidates(files: &[RepoFile]) -> Vec<Candidate> {
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let onnx: Vec<OnnxName> = paths.iter().filter_map(|p| parse_onnx(p)).collect();
    let tokens: Vec<(&str, String, &str)> = paths.iter().filter_map(|p| parse_tokens(p)).collect();

    let mut order: Vec<(String, String, String)> = Vec::new();
    let mut groups: HashMap<(String, String, String), Vec<&OnnxName>> = HashMap::new();
    for name in &onnx {
        let key = (
            name.dir.to_string(),
            name.prefix.to_ascii_lowercase(),
            name.suffix.to_ascii_lowercase(),
        );
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(name);
    }

    let unique = |matches: Vec<&&str>| (matches.len() == 1).then(|| matches[0].to_string());
    let mut out = Vec::new();
    for key in order {
        let names = &groups[&key];
        let (dir, prefix, suffix) = &key;
        let mut by_role: BTreeMap<SherpaRole, Vec<&OnnxName>> = BTreeMap::new();
        for name in names {
            by_role.entry(name.role).or_default().push(name);
        }
        let transducer = by_role.contains_key(&SherpaRole::Joiner);
        let mut roles = BTreeMap::new();
        let mut precisions = Vec::new();
        for (role, names) in &by_role {
            // A transducer's decoder is a few megabytes and loses accuracy
            // when quantized; everything else is smaller and faster in int8.
            let order: &[Precision] = if transducer && *role == SherpaRole::Decoder {
                &[Precision::Fp32, Precision::Int8, Precision::Fp16]
            } else {
                &[Precision::Int8, Precision::Fp32, Precision::Fp16]
            };
            let chosen = pick(names, order);
            roles.insert(*role, chosen.path.to_string());
            precisions.push(chosen.precision);
        }

        let token = unique(
            tokens
                .iter()
                .filter(|(d, p, _)| d == dir && p == prefix)
                .map(|(_, _, path)| path)
                .collect(),
        )
        .or_else(|| {
            unique(
                tokens
                    .iter()
                    .filter(|(d, p, _)| d == dir && p.is_empty())
                    .map(|(_, _, path)| path)
                    .collect(),
            )
        })
        .or_else(|| {
            unique(
                tokens
                    .iter()
                    .filter(|(_, p, _)| p == prefix)
                    .map(|(_, _, path)| path)
                    .collect(),
            )
        })
        .or_else(|| {
            unique(
                tokens
                    .iter()
                    .filter(|(_, p, _)| p.is_empty())
                    .map(|(_, _, path)| path)
                    .collect(),
            )
        });
        if let Some(token) = token {
            roles.insert(SherpaRole::Tokens, token);
        }

        let mut extra = Vec::new();
        for path in roles
            .values()
            .filter(|p| p.to_ascii_lowercase().ends_with(".onnx"))
        {
            let stem = &path[..path.len() - 5];
            for sidecar in [
                format!("{stem}.weights"),
                format!("{path}.data"),
                format!("{path}_data"),
                format!("{stem}.data"),
            ] {
                if paths.contains(&sidecar.as_str()) && !extra.contains(&sidecar) {
                    extra.push(sidecar);
                }
            }
        }

        let primary = roles
            .get(&SherpaRole::Encoder)
            .or_else(|| roles.get(&SherpaRole::Model))
            .or_else(|| roles.values().next())
            .cloned()
            .unwrap_or_default();
        let shown_prefix = names[0].prefix.clone();
        let shown_suffix = names[0].suffix.clone();
        let label = [dir.as_str(), shown_prefix.as_str(), shown_suffix.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ");
        out.push(Candidate {
            key: format!("{dir}|{prefix}|{suffix}"),
            label,
            roles,
            extra,
            primary,
            precisions,
        });
    }
    out
}

impl Candidate {
    /// «int8», «fp32» or «int8 и fp32».
    pub fn precision(&self) -> String {
        let mut words = Vec::new();
        for (p, word) in [
            (Precision::Int8, "int8"),
            (Precision::Fp32, "fp32"),
            (Precision::Fp16, "fp16"),
        ] {
            if self.precisions.contains(&p) {
                words.push(word);
            }
        }
        words.join(" и ")
    }

    fn reject(&self, reason: RejectReason, message: impl Into<String>) -> Rejection {
        Rejection {
            label: self.label.clone(),
            reason,
            message: message.into(),
        }
    }
}

fn is_streaming_comment(comment: Option<&String>) -> bool {
    comment.is_some_and(|c| {
        let c = c.to_ascii_lowercase();
        c.contains("streaming") && !c.contains("non-streaming")
    })
}

fn parse_positive(value: Option<&String>) -> Option<i32> {
    value
        .and_then(|v| v.trim().parse::<i32>().ok())
        .filter(|v| *v > 0)
}

/// Decides what `candidate` is from the metadata of its main file.
pub fn classify(
    candidate: &Candidate,
    meta: &BTreeMap<String, String>,
) -> Result<Classified, Rejection> {
    use SherpaRole::*;
    let has = |role| candidate.roles.contains_key(&role);
    if !has(Tokens) {
        return Err(candidate.reject(
            RejectReason::NoTokens,
            "ONNX-файлы есть, но не в формате sherpa-onnx: нет tokens.txt.",
        ));
    }

    let moonshine = has(Preprocessor) && has(Encoder) && has(UncachedDecoder) && has(CachedDecoder);
    let transducer = has(Encoder) && has(Decoder) && has(Joiner);
    let encoder_decoder = has(Encoder) && has(Decoder) && !has(Joiner);
    let single = has(Model) && !has(Encoder) && !has(Decoder);
    if !(moonshine || transducer || encoder_decoder || single) {
        return Err(candidate.reject(
            RejectReason::Incomplete,
            "Набор файлов неполный: не удалось собрать из него модель.",
        ));
    }

    let model_type = meta
        .get("model_type")
        .map(|t| t.trim().to_string())
        .unwrap_or_default();
    let mt = model_type.to_ascii_lowercase();
    let sherpa_keys = meta.keys().filter(|k| k.as_str() != "onnx.infer").count();
    if !moonshine && sherpa_keys == 0 {
        return Err(candidate.reject(
            RejectReason::NoSherpaMetadata,
            "В ONNX-файлах нет метаданных sherpa-onnx: модель экспортирована для другой программы.",
        ));
    }

    let nemo_streaming =
        meta.contains_key("window_size") || meta.contains_key("cache_last_channel_dim1");
    let feature_dim = parse_positive(meta.get("feat_dim").or_else(|| meta.get("feature_dim")));
    let giga_am = meta.get("is_giga_am").is_some_and(|v| v == "1");
    let url = meta
        .get("url")
        .map(|u| u.to_ascii_lowercase())
        .unwrap_or_default();

    let mut layout_files: BTreeMap<SherpaRole, String> = BTreeMap::new();
    let mut take = |roles: &[SherpaRole]| {
        for role in roles {
            if let Some(path) = candidate.roles.get(role) {
                layout_files.insert(*role, path.clone());
            }
        }
    };

    let (family, streaming, title) = if moonshine {
        take(&[
            Preprocessor,
            Encoder,
            UncachedDecoder,
            CachedDecoder,
            Tokens,
        ]);
        (SherpaFamily::Moonshine, false, "Moonshine".to_string())
    } else if transducer {
        take(&[Encoder, Decoder, Joiner, Tokens]);
        if mt.starts_with("encdec") {
            let title = if giga_am {
                "GigaAM (трансдьюсер)"
            } else if url.contains("parakeet") {
                "Parakeet (NeMo-трансдьюсер)"
            } else {
                "NeMo-трансдьюсер"
            };
            (
                SherpaFamily::NemoTransducer,
                nemo_streaming,
                title.to_string(),
            )
        } else if matches!(
            mt.as_str(),
            "zipformer" | "zipformer2" | "lstm" | "conformer"
        ) || meta.contains_key("decode_chunk_len")
        {
            let streaming =
                meta.contains_key("decode_chunk_len") || is_streaming_comment(meta.get("comment"));
            let title = match mt.as_str() {
                "lstm" => "LSTM-трансдьюсер",
                "conformer" => "Conformer-трансдьюсер",
                _ => "Zipformer-трансдьюсер",
            };
            (SherpaFamily::Transducer, streaming, title.to_string())
        } else {
            return Err(unknown_type(candidate, &model_type));
        }
    } else if encoder_decoder {
        take(&[Encoder, Decoder, Tokens]);
        if mt.starts_with("whisper") || meta.contains_key("n_mels") {
            let size = mt.strip_prefix("whisper-").unwrap_or("");
            let title = if size.is_empty() {
                "Whisper".to_string()
            } else {
                format!("Whisper {size}")
            };
            (SherpaFamily::Whisper, false, title)
        } else if mt == "encdecmultitaskmodel" {
            (SherpaFamily::Canary, false, "NeMo Canary".into())
        } else if mt.starts_with("fire-red-asr") || mt.starts_with("fire_red_asr") {
            (SherpaFamily::FireRedAsr, false, "FireRedASR".into())
        } else if mt == "paraformer" {
            (SherpaFamily::Paraformer, true, "Paraformer".into())
        } else {
            return Err(unknown_type(candidate, &model_type));
        }
    } else {
        take(&[Model, Tokens]);
        let ctc_streaming =
            meta.contains_key("decode_chunk_len") || is_streaming_comment(meta.get("comment"));
        let nemo_ctc = (mt.starts_with("encdec") || mt == "ctcmodel")
            && (mt.contains("ctc") || !meta.contains_key("pred_hidden"));
        match mt.as_str() {
            _ if nemo_ctc => (
                SherpaFamily::NemoCtc,
                nemo_streaming,
                if giga_am { "GigaAM (CTC)" } else { "NeMo CTC" }.to_string(),
            ),
            "zipformer2_ctc" => (SherpaFamily::ZipformerCtc, false, "Zipformer CTC".into()),
            "zipformer2" | "zipformer" if ctc_streaming => {
                (SherpaFamily::ZipformerCtc, true, "Zipformer CTC".into())
            }
            "wenet_ctc" => (SherpaFamily::WenetCtc, ctc_streaming, "WeNet CTC".into()),
            "telespeech_ctc" => (SherpaFamily::TelespeechCtc, false, "TeleSpeech CTC".into()),
            "medasr_ctc" => (SherpaFamily::MedasrCtc, false, "MedASR".into()),
            "t-one" => (SherpaFamily::ToneCtc, true, "T-one".into()),
            "paraformer" => (SherpaFamily::Paraformer, false, "Paraformer".into()),
            t if t.starts_with("dolphin") => (SherpaFamily::Dolphin, false, "Dolphin".into()),
            t if t.starts_with("omnilingual") => (
                SherpaFamily::OmnilingualCtc,
                false,
                "Omnilingual ASR".into(),
            ),
            t if t.starts_with("sense_voice") || t.starts_with("sensevoice") => {
                (SherpaFamily::SenseVoice, false, "SenseVoice".into())
            }
            t if (t.starts_with("fire-red-asr") || t.starts_with("fire_red_asr"))
                && t.contains("ctc") =>
            {
                (SherpaFamily::FireRedAsrCtc, false, "FireRedASR CTC".into())
            }
            "tdnn" => {
                return Err(candidate.reject(
                    RejectReason::NotSpeechRecognition,
                    "Это учебная модель sherpa-onnx (yes/no), для диктовки она не подходит.",
                ))
            }
            t if ["vits", "matcha", "kokoro", "kitten", "piper", "zipvoice"]
                .iter()
                .any(|tts| t.starts_with(tts)) =>
            {
                return Err(candidate.reject(
                    RejectReason::NotSpeechRecognition,
                    "Это модель синтеза речи (TTS), а не распознавания.",
                ))
            }
            _ => return Err(unknown_type(candidate, &model_type)),
        }
    };

    if let Some(missing) = required_metadata(family, streaming)
        .iter()
        .find(|key| !meta.contains_key(**key))
    {
        return Err(candidate.reject(
            RejectReason::NoSherpaMetadata,
            format!("В метаданных модели нет поля «{missing}», без которого sherpa-onnx её не запустит."),
        ));
    }

    let mut layout = SherpaLayout {
        family,
        streaming,
        files: layout_files,
        model_type: None,
        sample_rate: None,
        feature_dim,
    };
    match family {
        SherpaFamily::NemoTransducer if !streaming => {
            layout.model_type = Some("nemo_transducer".into())
        }
        SherpaFamily::ToneCtc => {
            layout.sample_rate = parse_positive(meta.get("sample_rate")).filter(|r| *r != 16_000)
        }
        _ => {}
    }
    if family.required_roles(streaming).is_none() {
        let mode = if streaming {
            "Потоковые"
        } else {
            "Не потоковые"
        };
        return Err(candidate.reject(
            RejectReason::UnsupportedMode,
            format!("{mode} модели {title} sherpa-onnx не запускает."),
        ));
    }
    if let Err(e) = layout.validate() {
        return Err(candidate.reject(
            RejectReason::Incomplete,
            format!("Набор файлов неполный: {e}."),
        ));
    }
    Ok(Classified { layout, title })
}

/// Metadata sherpa-onnx reads without a default for these families; a model
/// without it stops the process instead of failing to load.
fn required_metadata(family: SherpaFamily, streaming: bool) -> &'static [&'static str] {
    match (family, streaming) {
        (SherpaFamily::NemoTransducer, _) => &["vocab_size", "pred_rnn_layers", "pred_hidden"],
        (SherpaFamily::NemoCtc, _) => &["vocab_size", "subsampling_factor"],
        (SherpaFamily::Transducer, true) => &["decode_chunk_len", "T"],
        (SherpaFamily::Whisper, _) => &["n_mels", "n_text_layer", "sot_sequence"],
        (SherpaFamily::Paraformer, _) | (SherpaFamily::SenseVoice, _) => {
            &["lfr_window_size", "neg_mean", "inv_stddev"]
        }
        (SherpaFamily::ToneCtc, _) => &["state_dim", "frame_length_ms"],
        _ => &[],
    }
}

fn unknown_type(candidate: &Candidate, model_type: &str) -> Rejection {
    let message = if model_type.is_empty() {
        "Не удалось понять, что это за модель: в её метаданных нет типа.".to_string()
    } else {
        format!("Тип модели «{model_type}» эта версия приложения не знает.")
    };
    candidate.reject(RejectReason::UnknownType, message)
}

/// Why a repository has nothing to run, when no candidate passed: (code, message).
pub fn explain(
    files: &[RepoFile],
    pipeline_tag: Option<&str>,
    rejections: &[Rejection],
) -> (String, String) {
    let has_ext = |exts: &[&str]| {
        files.iter().any(|f| {
            let lower = f.path.to_ascii_lowercase();
            exts.iter().any(|e| lower.ends_with(e))
        })
    };
    if let Some(tag) =
        pipeline_tag.filter(|t| !t.is_empty() && *t != "automatic-speech-recognition")
    {
        return (
            "not_asr".into(),
            format!("Это не модель распознавания речи: на Hugging Face она отмечена как «{tag}»."),
        );
    }

    if !has_ext(&[".onnx"]) {
        let model_files = files.iter().any(|f| {
            let lower = f.path.to_ascii_lowercase();
            let name = split_dir(&lower).1.to_string();
            !(name.starts_with('.')
                || name.starts_with("readme")
                || name.starts_with("license")
                || name.ends_with(".md"))
        });
        return if has_ext(&[
            ".nemo",
            ".safetensors",
            ".bin",
            ".pt",
            ".pth",
            ".ckpt",
            ".msgpack",
            ".h5",
        ]) {
            (
                "pytorch".into(),
                "В репозитории нет ONNX-файлов — эта модель только для PyTorch.".into(),
            )
        } else if has_ext(&[".gguf", ".ggml"]) {
            (
                "ggml".into(),
                "В репозитории нет ONNX-файлов — это модель для whisper.cpp или llama.cpp.".into(),
            )
        } else if has_ext(&[".ort"]) {
            (
                "ort".into(),
                "Файлы модели в формате ORT (.ort), а приложение читает только .onnx.".into(),
            )
        } else if model_files {
            ("no_onnx".into(), "В репозитории нет ONNX-файлов.".into())
        } else {
            ("empty".into(), "В репозитории нет файлов модели.".into())
        };
    }

    let stems: Vec<String> = files
        .iter()
        .filter_map(|f| parse_onnx(&f.path))
        .map(|n| format!("{}_{}", n.prefix, n.suffix).to_ascii_lowercase())
        .collect();
    let unsupported = if files.iter().any(|f| {
        split_dir(&f.path)
            .1
            .to_ascii_lowercase()
            .starts_with("encoder_adaptor")
    }) {
        Some("FunASR-nano")
    } else if stems.iter().any(|s| s.contains("conv_frontend")) {
        Some("Qwen3-ASR")
    } else {
        None
    };
    if let Some(kind) = unsupported {
        return (
            "unsupported_family".into(),
            format!(
                "Это {kind}: sherpa-onnx умеет такие модели, а эта версия приложения — пока нет."
            ),
        );
    }

    let worst = rejections.iter().max_by_key(|r| r.reason);
    match worst.map(|r| r.reason) {
        Some(RejectReason::NoTokens) | None => {
            let flavour = if has_ext(&["tokenizer.json", "preprocessor_config.json"]) {
                " Похоже на экспорт для Transformers.js или Optimum."
            } else if has_ext(&["vocab.txt", ".yaml"]) {
                " Похоже на экспорт для onnx-asr."
            } else {
                ""
            };
            (
                "no_tokens".into(),
                format!("ONNX-файлы есть, но не в формате sherpa-onnx: нет tokens.txt.{flavour}"),
            )
        }
        Some(reason) => {
            let code = serde_json::to_value(reason)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            (code, worst.unwrap().message.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> RepoFile {
        RepoFile {
            path: path.into(),
            size: 1,
            sha256: None,
            git_sha1: None,
        }
    }

    fn files(paths: &[&str]) -> Vec<RepoFile> {
        paths.iter().map(|p| file(p)).collect()
    }

    #[test]
    fn names_are_split_into_prefix_role_and_suffix() {
        let n = parse_onnx("encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx").unwrap();
        assert_eq!(
            (n.role, n.prefix.as_str(), n.suffix.as_str()),
            (SherpaRole::Encoder, "", "epoch-99-avg-1-chunk-16-left-128")
        );
        assert_eq!(n.precision, Precision::Int8);
        let n = parse_onnx("distil-large-v3.5-decoder.onnx").unwrap();
        assert_eq!(
            (n.role, n.prefix.as_str()),
            (SherpaRole::Decoder, "distil-large-v3.5")
        );
        let n = parse_onnx("gigaam_v3_e2e_rnnt_encoder_int8.onnx").unwrap();
        assert_eq!(
            (n.role, n.prefix.as_str(), n.precision),
            (SherpaRole::Encoder, "gigaam_v3_e2e_rnnt", Precision::Int8)
        );
        let n = parse_onnx("gigaam_v3_e2e_rnnt_joint.onnx").unwrap();
        assert_eq!(n.role, SherpaRole::Joiner);
        let n = parse_onnx("uncached_decode.int8.onnx").unwrap();
        assert_eq!(
            (n.role, n.prefix.as_str(), n.suffix.as_str()),
            (SherpaRole::UncachedDecoder, "", "")
        );
        let n = parse_onnx("am-onnx/joiner.int8.onnx").unwrap();
        assert_eq!((n.dir, n.role), ("am-onnx", SherpaRole::Joiner));
        let n = parse_onnx("GigaAMv2_ctc_public.onnx").unwrap();
        assert_eq!(
            (n.role, n.prefix.as_str()),
            (SherpaRole::Model, "GigaAMv2_ctc_public")
        );
        assert!(parse_onnx("model.onnx.data").is_none());
        assert_eq!(parse_tokens("tiny-tokens.txt").unwrap().1, "tiny");
        assert_eq!(parse_tokens("lang/tokens.txt").unwrap().0, "lang");
        assert!(parse_tokens("bpe.vocab").is_none());
    }

    #[test]
    fn decoder_of_a_transducer_stays_fp32_and_the_rest_goes_int8() {
        let c = candidates(&files(&[
            "decoder.int8.onnx",
            "decoder.onnx",
            "encoder.int8.onnx",
            "encoder.onnx",
            "joiner.int8.onnx",
            "joiner.onnx",
            "tokens.txt",
        ]));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].roles[&SherpaRole::Decoder], "decoder.onnx");
        assert_eq!(c[0].roles[&SherpaRole::Encoder], "encoder.int8.onnx");
        assert_eq!(c[0].roles[&SherpaRole::Joiner], "joiner.int8.onnx");
        assert_eq!(c[0].precision(), "int8 и fp32");

        let c = candidates(&files(&[
            "turbo-decoder.int8.onnx",
            "turbo-decoder.onnx",
            "turbo-encoder.onnx",
            "turbo-encoder.weights",
            "turbo-tokens.txt",
        ]));
        assert_eq!(c[0].roles[&SherpaRole::Decoder], "turbo-decoder.int8.onnx");
        assert_eq!(c[0].extra, vec!["turbo-encoder.weights".to_string()]);
    }

    #[test]
    fn tokens_are_matched_by_prefix_then_directory_then_uniqueness() {
        let c = candidates(&files(&[
            "gigaam_v3_ctc.onnx",
            "gigaam_v3_ctc_int8.onnx",
            "gigaam_v3_ctc_tokens.txt",
            "gigaam_v3_rnnt_encoder.onnx",
            "gigaam_v3_rnnt_decoder.onnx",
            "gigaam_v3_rnnt_joint.onnx",
            "gigaam_v3_rnnt_tokens.txt",
        ]));
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].roles[&SherpaRole::Model], "gigaam_v3_ctc_int8.onnx");
        assert_eq!(c[0].roles[&SherpaRole::Tokens], "gigaam_v3_ctc_tokens.txt");
        assert_eq!(c[1].roles[&SherpaRole::Tokens], "gigaam_v3_rnnt_tokens.txt");

        let c = candidates(&files(&[
            "am-onnx/encoder.onnx",
            "am-onnx/decoder.onnx",
            "am-onnx/joiner.onnx",
            "lang/tokens.txt",
        ]));
        assert_eq!(c[0].roles[&SherpaRole::Tokens], "lang/tokens.txt");

        let c = candidates(&files(&[
            "a/model.onnx",
            "a/tokens.txt",
            "b/model.onnx",
            "b/tokens.txt",
            "c/model.onnx",
        ]));
        assert_eq!(c[0].roles[&SherpaRole::Tokens], "a/tokens.txt");
        assert!(
            !c[2].roles.contains_key(&SherpaRole::Tokens),
            "c has two equally good tokens files"
        );
    }

    fn meta(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn metadata_decides_streaming_and_nemo() {
        let c = &candidates(&files(&[
            "encoder.int8.onnx",
            "decoder.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ]))[0];
        let online = classify(
            c,
            &meta(&[
                ("model_type", "zipformer2"),
                ("decode_chunk_len", "64"),
                ("T", "77"),
            ]),
        )
        .unwrap();
        let missing = classify(
            c,
            &meta(&[("model_type", "zipformer2"), ("decode_chunk_len", "64")]),
        )
        .unwrap_err();
        assert!(missing.message.contains("«T»"), "{}", missing.message);
        assert_eq!(
            (online.layout.family, online.layout.streaming),
            (SherpaFamily::Transducer, true)
        );
        let offline = classify(
            c,
            &meta(&[
                ("model_type", "zipformer2"),
                ("comment", "non-streaming zipformer2"),
            ]),
        )
        .unwrap();
        assert!(!offline.layout.streaming);
        let nemo = [
            ("vocab_size", "1024"),
            ("pred_rnn_layers", "1"),
            ("pred_hidden", "320"),
        ];
        let gigaam = classify(
            c,
            &meta(
                &[
                    &nemo[..],
                    &[("model_type", "EncDecRNNTBPEModel"), ("is_giga_am", "1")],
                ]
                .concat(),
            ),
        )
        .unwrap();
        assert_eq!(gigaam.layout.family, SherpaFamily::NemoTransducer);
        assert_eq!(gigaam.layout.model_type.as_deref(), Some("nemo_transducer"));
        assert_eq!(gigaam.title, "GigaAM (трансдьюсер)");
        let nemo_online = classify(
            c,
            &meta(
                &[
                    &nemo[..],
                    &[
                        ("model_type", "EncDecHybridRNNTCTCBPEModel"),
                        ("window_size", "25"),
                    ],
                ]
                .concat(),
            ),
        )
        .unwrap();
        assert!(nemo_online.layout.streaming);
        assert!(nemo_online.layout.model_type.is_none());
    }

    #[test]
    fn exports_without_sherpa_metadata_are_refused() {
        let c = &candidates(&files(&[
            "encoder.int8.onnx",
            "decoder.onnx",
            "joiner.onnx",
            "tokens.txt",
        ]))[0];
        let r = classify(c, &meta(&[("onnx.infer", "onnxruntime.quant")])).unwrap_err();
        assert_eq!(r.reason, RejectReason::NoSherpaMetadata);
        let moonshine = &candidates(&files(&[
            "preprocess.onnx",
            "encode.int8.onnx",
            "uncached_decode.int8.onnx",
            "cached_decode.int8.onnx",
            "tokens.txt",
        ]))[0];
        assert_eq!(
            classify(moonshine, &BTreeMap::new()).unwrap().layout.family,
            SherpaFamily::Moonshine
        );
    }

    #[test]
    fn unsupported_modes_and_other_kinds_of_models_are_named() {
        let wenet = candidates(&files(&[
            "model-streaming.int8.onnx",
            "model.int8.onnx",
            "tokens.txt",
        ]));
        let streaming = classify(
            &wenet[0],
            &meta(&[("model_type", "wenet_ctc"), ("comment", "streaming")]),
        )
        .unwrap_err();
        assert_eq!(streaming.reason, RejectReason::UnsupportedMode);
        let offline = classify(
            &wenet[1],
            &meta(&[("model_type", "wenet_ctc"), ("comment", "non-streaming")]),
        )
        .unwrap();
        assert_eq!(offline.layout.family, SherpaFamily::WenetCtc);

        let tts = &candidates(&files(&["model.onnx", "tokens.txt", "lexicon.txt"]))[0];
        let r = classify(
            tts,
            &meta(&[("model_type", "vits"), ("language", "Chinese")]),
        )
        .unwrap_err();
        assert_eq!(r.reason, RejectReason::NotSpeechRecognition);
        let r = classify(tts, &meta(&[("model_type", "qwen9-asr"), ("version", "1")])).unwrap_err();
        assert!(r.message.contains("qwen9-asr"));
    }

    #[test]
    fn explain_names_the_most_telling_reason() {
        let pytorch = files(&[
            ".gitattributes",
            "README.md",
            "model.safetensors",
            "parakeet.nemo",
        ]);
        assert_eq!(
            explain(&pytorch, Some("automatic-speech-recognition"), &[]).0,
            "pytorch"
        );
        assert!(explain(&pytorch, None, &[])
            .1
            .contains("только для PyTorch"));
        assert_eq!(explain(&files(&[".gitattributes"]), None, &[]).0, "empty");
        assert_eq!(
            explain(&files(&["model.safetensors"]), Some("text-generation"), &[]).0,
            "not_asr"
        );
        let onnx_asr = files(&["v3_ctc.onnx", "v3_vocab.txt", "v3_ctc.yaml"]);
        let (code, message) = explain(&onnx_asr, None, &[]);
        assert_eq!(code, "no_tokens");
        assert!(message.contains("onnx-asr"), "{message}");
    }
}
