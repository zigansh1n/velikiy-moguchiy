//! Which sherpa-onnx recognizer a model needs and which of its files plays
//! which part. Models installed from Hugging Face record this in their
//! manifest, so the provider never has to guess from file names.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Model families sherpa-onnx can run, as this app names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SherpaFamily {
    /// icefall zipformer, conformer or LSTM transducer.
    Transducer,
    /// NeMo transducer: GigaAM, Parakeet, FastConformer.
    NemoTransducer,
    Paraformer,
    NemoCtc,
    /// Offline `zipformer_ctc`, streaming `zipformer2_ctc`.
    ZipformerCtc,
    WenetCtc,
    TelespeechCtc,
    OmnilingualCtc,
    MedasrCtc,
    FireRedAsrCtc,
    /// T-one, streaming CTC for Russian.
    ToneCtc,
    Whisper,
    SenseVoice,
    Moonshine,
    FireRedAsr,
    Dolphin,
    Canary,
    /// A family a newer version of the app wrote.
    #[serde(other)]
    Unknown,
}

/// The part a file plays in a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SherpaRole {
    Encoder,
    Decoder,
    Joiner,
    Model,
    Tokens,
    Preprocessor,
    UncachedDecoder,
    CachedDecoder,
}

/// How to build the recognizer for one model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SherpaLayout {
    pub family: SherpaFamily,
    #[serde(default)]
    pub streaming: bool,
    /// Role -> file path relative to the model directory.
    pub files: BTreeMap<SherpaRole, String>,
    /// sherpa-onnx `model_type`, for families it cannot tell apart by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_type: Option<String>,
    /// Feature sample rate in Hz when the model was trained on other than 16 kHz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<i32>,
    /// Feature dimension when it differs from the family's usual one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_dim: Option<i32>,
}

/// A layout whose files were found inside the model directory.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLayout {
    pub family: SherpaFamily,
    pub streaming: bool,
    pub model_type: Option<String>,
    pub sample_rate: i32,
    pub feature_dim: i32,
    paths: BTreeMap<SherpaRole, PathBuf>,
}

impl SherpaFamily {
    /// Files the recognizer needs, or `None` when sherpa-onnx has no
    /// recognizer of this family for this mode.
    pub fn required_roles(self, streaming: bool) -> Option<&'static [SherpaRole]> {
        use SherpaFamily::*;
        use SherpaRole::*;
        const TRANSDUCER: &[SherpaRole] = &[Encoder, Decoder, Joiner, Tokens];
        const SINGLE: &[SherpaRole] = &[Model, Tokens];
        const ENCODER_DECODER: &[SherpaRole] = &[Encoder, Decoder, Tokens];
        const MOONSHINE: &[SherpaRole] = &[
            Preprocessor,
            Encoder,
            UncachedDecoder,
            CachedDecoder,
            Tokens,
        ];
        match (self, streaming) {
            (Transducer | NemoTransducer, _) => Some(TRANSDUCER),
            (Paraformer, true) => Some(ENCODER_DECODER),
            (Paraformer, false) => Some(SINGLE),
            (NemoCtc | ZipformerCtc, _) => Some(SINGLE),
            (ToneCtc, true) => Some(SINGLE),
            (WenetCtc | TelespeechCtc | OmnilingualCtc | MedasrCtc | FireRedAsrCtc, false) => {
                Some(SINGLE)
            }
            (SenseVoice | Dolphin, false) => Some(SINGLE),
            (Whisper | FireRedAsr | Canary, false) => Some(ENCODER_DECODER),
            (Moonshine, false) => Some(MOONSHINE),
            _ => None,
        }
    }

    /// Feature dimension sherpa-onnx expects unless a model says otherwise.
    fn default_feature_dim(self) -> i32 {
        match self {
            SherpaFamily::TelespeechCtc => 40,
            SherpaFamily::Canary => 128,
            _ => 80,
        }
    }
}

impl SherpaLayout {
    /// What a manifest whose `sherpa` section cannot be read stands for:
    /// a model this version cannot run.
    pub fn unreadable() -> Self {
        Self {
            family: SherpaFamily::Unknown,
            streaming: false,
            files: BTreeMap::new(),
            model_type: None,
            sample_rate: None,
            feature_dim: None,
        }
    }

    /// Checks the layout without touching the disk: a known family and mode,
    /// every required file, and only plain relative paths.
    pub fn validate(&self) -> Result<(), String> {
        let required = self.family.required_roles(self.streaming).ok_or_else(|| {
            if self.family == SherpaFamily::Unknown {
                "this model type needs a newer version of the app".to_string()
            } else {
                format!(
                    "sherpa-onnx has no {} recognizer for {:?} models",
                    if self.streaming {
                        "streaming"
                    } else {
                        "offline"
                    },
                    self.family
                )
            }
        })?;
        if let Some(missing) = required.iter().find(|role| !self.files.contains_key(role)) {
            return Err(format!("no {missing:?} file in the model layout"));
        }
        for path in self.files.values() {
            relative_path(path)?;
        }
        if matches!(self.sample_rate, Some(rate) if rate <= 0) {
            return Err("sample rate must be positive".into());
        }
        if matches!(self.feature_dim, Some(dim) if dim <= 0) {
            return Err("feature dimension must be positive".into());
        }
        Ok(())
    }

    /// Validates the layout and finds each file as a regular file inside
    /// `model_dir`, following no symlinks on the way.
    pub fn resolve(&self, model_dir: &Path) -> Result<ResolvedLayout, String> {
        self.validate()?;
        let root = std::fs::canonicalize(model_dir)
            .map_err(|e| format!("model not found: {}: {e}", model_dir.display()))?;
        let mut paths = BTreeMap::new();
        for (role, rel) in &self.files {
            let rel = relative_path(rel)?;
            let mut current = root.clone();
            for component in rel.components() {
                current.push(component);
                let meta = std::fs::symlink_metadata(&current)
                    .map_err(|_| format!("missing {}", rel.display()))?;
                if meta.file_type().is_symlink() {
                    return Err(format!("{} is a symlink", rel.display()));
                }
            }
            if !std::fs::symlink_metadata(&current).is_ok_and(|m| m.is_file()) {
                return Err(format!("{} is not a file", rel.display()));
            }
            paths.insert(*role, current);
        }
        Ok(ResolvedLayout {
            family: self.family,
            streaming: self.streaming,
            model_type: self.model_type.clone(),
            sample_rate: self.sample_rate.unwrap_or(16_000),
            feature_dim: self
                .feature_dim
                .unwrap_or_else(|| self.family.default_feature_dim()),
            paths,
        })
    }
}

impl ResolvedLayout {
    /// The layout of a model directory without a recorded layout: the
    /// built-in transducers, found by name as they always were.
    pub fn legacy_transducer(
        model_dir: &Path,
        offline_model_type: Option<String>,
    ) -> Result<Self, String> {
        let names: Vec<String> = std::fs::read_dir(model_dir)
            .map_err(|e| format!("read model dir: {e}"))?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        let pick = |pattern: &str| -> Result<PathBuf, String> {
            let candidates: Vec<&String> = names
                .iter()
                .filter(|name| name.contains(pattern) && name.ends_with(".onnx"))
                .collect();
            candidates
                .iter()
                .find(|n| n.contains(".int8."))
                .or_else(|| candidates.iter().find(|n| n.contains(".fp16.")))
                .or_else(|| candidates.first())
                .map(|name| model_dir.join(name))
                .ok_or_else(|| format!("missing {pattern}*.onnx in {}", model_dir.display()))
        };
        let mut paths = BTreeMap::new();
        paths.insert(SherpaRole::Encoder, pick("encoder")?);
        paths.insert(SherpaRole::Decoder, pick("decoder")?);
        paths.insert(SherpaRole::Joiner, pick("joiner")?);
        if !names.iter().any(|n| n == "tokens.txt") {
            return Err(format!("missing tokens.txt in {}", model_dir.display()));
        }
        paths.insert(SherpaRole::Tokens, model_dir.join("tokens.txt"));
        let family = match offline_model_type.as_deref() {
            Some("nemo_transducer") => SherpaFamily::NemoTransducer,
            _ => SherpaFamily::Transducer,
        };
        Ok(Self {
            family,
            streaming: offline_model_type.is_none(),
            model_type: offline_model_type,
            sample_rate: 16_000,
            feature_dim: 80,
            paths,
        })
    }

    /// Absolute path of a role's file, as sherpa-onnx takes it.
    pub fn path(&self, role: SherpaRole) -> Option<String> {
        self.paths
            .get(&role)
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// Identifies the loaded model for the offline recognizer cache.
    pub fn cache_key(&self, num_threads: i32) -> String {
        let files: Vec<String> = self
            .paths
            .values()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        format!("{:?}|{}|{}", self.family, files.join("|"), num_threads)
    }
}

fn relative_path(path: &str) -> Result<&Path, String> {
    let path = Path::new(path);
    let plain = path.components().count() > 0
        && path.components().all(|c| matches!(c, Component::Normal(_)));
    if plain {
        Ok(path)
    } else {
        Err(format!(
            "unsafe file name in model layout: {}",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sherpa-layout-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn layout(family: SherpaFamily, streaming: bool, files: &[(SherpaRole, &str)]) -> SherpaLayout {
        SherpaLayout {
            family,
            streaming,
            files: files.iter().map(|(r, p)| (*r, p.to_string())).collect(),
            model_type: None,
            sample_rate: None,
            feature_dim: None,
        }
    }

    #[test]
    fn every_family_states_its_files_and_unsupported_modes_are_refused() {
        use SherpaFamily::*;
        for family in [
            Transducer,
            NemoTransducer,
            Paraformer,
            NemoCtc,
            ZipformerCtc,
            WenetCtc,
            TelespeechCtc,
            OmnilingualCtc,
            MedasrCtc,
            FireRedAsrCtc,
            Whisper,
            SenseVoice,
            Moonshine,
            FireRedAsr,
            Dolphin,
            Canary,
        ] {
            assert!(family.required_roles(false).is_some(), "{family:?} offline");
        }
        for family in [
            Transducer,
            NemoTransducer,
            Paraformer,
            NemoCtc,
            ZipformerCtc,
            ToneCtc,
        ] {
            assert!(
                family.required_roles(true).is_some(),
                "{family:?} streaming"
            );
        }
        assert!(Whisper.required_roles(true).is_none());
        assert!(ToneCtc.required_roles(false).is_none());
        assert!(Unknown.required_roles(false).is_none());
    }

    #[test]
    fn validate_names_the_missing_file() {
        let whisper = layout(
            SherpaFamily::Whisper,
            false,
            &[
                (SherpaRole::Encoder, "tiny-encoder.int8.onnx"),
                (SherpaRole::Tokens, "tiny-tokens.txt"),
            ],
        );
        let err = whisper.validate().unwrap_err();
        assert!(err.contains("Decoder"), "{err}");
    }

    #[test]
    fn validate_refuses_paths_that_leave_the_model_dir() {
        for bad in [
            "../tokens.txt",
            "/etc/passwd",
            "./model.onnx",
            "a/../../b",
            "",
        ] {
            let l = layout(
                SherpaFamily::SenseVoice,
                false,
                &[
                    (SherpaRole::Model, "model.int8.onnx"),
                    (SherpaRole::Tokens, bad),
                ],
            );
            assert!(l.validate().is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn unknown_family_from_a_newer_app_is_reported_not_guessed() {
        let json = r#"{"family":"qwen9_asr","streaming":false,"files":{"model":"m.onnx"}}"#;
        let l: SherpaLayout = serde_json::from_str(json).unwrap();
        assert_eq!(l.family, SherpaFamily::Unknown);
        assert!(l.validate().unwrap_err().contains("newer version"));
        assert!(SherpaLayout::unreadable().validate().is_err());
    }

    #[test]
    fn layout_round_trips_through_json() {
        let mut l = layout(
            SherpaFamily::ToneCtc,
            true,
            &[
                (SherpaRole::Model, "model.onnx"),
                (SherpaRole::Tokens, "tokens.txt"),
            ],
        );
        l.sample_rate = Some(8000);
        let json = serde_json::to_string(&l).unwrap();
        assert!(json.contains(r#""family":"tone_ctc""#), "{json}");
        assert!(!json.contains("feature_dim"), "{json}");
        assert_eq!(serde_json::from_str::<SherpaLayout>(&json).unwrap(), l);
    }

    #[test]
    fn resolve_finds_files_in_subdirectories_and_applies_family_defaults() {
        let dir = temp_dir("resolve");
        fs::create_dir_all(dir.join("am-onnx")).unwrap();
        fs::create_dir_all(dir.join("lang")).unwrap();
        for f in [
            "am-onnx/encoder.int8.onnx",
            "am-onnx/decoder.onnx",
            "am-onnx/joiner.int8.onnx",
            "lang/tokens.txt",
        ] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let l = layout(
            SherpaFamily::Transducer,
            false,
            &[
                (SherpaRole::Encoder, "am-onnx/encoder.int8.onnx"),
                (SherpaRole::Decoder, "am-onnx/decoder.onnx"),
                (SherpaRole::Joiner, "am-onnx/joiner.int8.onnx"),
                (SherpaRole::Tokens, "lang/tokens.txt"),
            ],
        );
        let resolved = l.resolve(&dir).unwrap();
        let root = fs::canonicalize(&dir).unwrap();
        assert_eq!(
            resolved.path(SherpaRole::Tokens).unwrap(),
            root.join("lang/tokens.txt").to_string_lossy()
        );
        assert_eq!((resolved.sample_rate, resolved.feature_dim), (16_000, 80));

        let mut telespeech = layout(
            SherpaFamily::TelespeechCtc,
            false,
            &[
                (SherpaRole::Model, "am-onnx/encoder.int8.onnx"),
                (SherpaRole::Tokens, "lang/tokens.txt"),
            ],
        );
        assert_eq!(telespeech.resolve(&dir).unwrap().feature_dim, 40);
        telespeech.feature_dim = Some(64);
        assert_eq!(telespeech.resolve(&dir).unwrap().feature_dim, 64);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_reports_a_missing_file() {
        let dir = temp_dir("missing");
        fs::write(dir.join("model.int8.onnx"), b"x").unwrap();
        let l = layout(
            SherpaFamily::Dolphin,
            false,
            &[
                (SherpaRole::Model, "model.int8.onnx"),
                (SherpaRole::Tokens, "tokens.txt"),
            ],
        );
        assert!(l.resolve(&dir).unwrap_err().contains("tokens.txt"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn resolve_refuses_symlinks() {
        let dir = temp_dir("symlink");
        let outside = temp_dir("outside");
        fs::write(outside.join("tokens.txt"), b"x").unwrap();
        fs::write(dir.join("model.onnx"), b"x").unwrap();
        std::os::unix::fs::symlink(outside.join("tokens.txt"), dir.join("tokens.txt")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("sub")).unwrap();
        for tokens in ["tokens.txt", "sub/tokens.txt"] {
            let l = layout(
                SherpaFamily::NemoCtc,
                false,
                &[
                    (SherpaRole::Model, "model.onnx"),
                    (SherpaRole::Tokens, tokens),
                ],
            );
            assert!(l.resolve(&dir).unwrap_err().contains("symlink"), "{tokens}");
        }
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn legacy_transducer_keeps_the_old_file_choice() {
        let dir = temp_dir("legacy");
        for f in [
            "encoder-epoch-99-avg-1.onnx",
            "encoder-epoch-99-avg-1.int8.onnx",
            "decoder-epoch-99-avg-1.onnx",
            "joiner-epoch-99-avg-1.int8.onnx",
            "tokens.txt",
        ] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let online = ResolvedLayout::legacy_transducer(&dir, None).unwrap();
        assert!(online.streaming);
        assert_eq!(online.family, SherpaFamily::Transducer);
        assert!(online
            .path(SherpaRole::Encoder)
            .unwrap()
            .ends_with("encoder-epoch-99-avg-1.int8.onnx"));
        let gigaam =
            ResolvedLayout::legacy_transducer(&dir, Some("nemo_transducer".into())).unwrap();
        assert!(!gigaam.streaming);
        assert_eq!(gigaam.family, SherpaFamily::NemoTransducer);
        fs::remove_file(dir.join("tokens.txt")).unwrap();
        assert!(ResolvedLayout::legacy_transducer(&dir, None).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
