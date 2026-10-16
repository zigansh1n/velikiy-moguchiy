//! The engines a downloaded or found model can run on, and whether this
//! build has them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    SherpaOnnx,
    WhisperCpp,
    Mlx,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineStatus {
    pub ready: bool,
    /// Why the model cannot run here, when it cannot.
    pub note: Option<String>,
}

impl Backend {
    /// The `asr.provider` value that runs this engine.
    pub fn provider(self) -> &'static str {
        match self {
            Backend::SherpaOnnx => "sherpa-onnx",
            Backend::WhisperCpp => "whisper-cpp",
            Backend::Mlx => "mlx",
        }
    }

    pub fn from_provider(provider: &str) -> Option<Self> {
        match provider {
            "sherpa-onnx" => Some(Backend::SherpaOnnx),
            "whisper-cpp" => Some(Backend::WhisperCpp),
            "mlx" => Some(Backend::Mlx),
            _ => None,
        }
    }

    /// The badge the window shows.
    pub fn display_name(self) -> &'static str {
        match self {
            Backend::SherpaOnnx => "sherpa-onnx",
            Backend::WhisperCpp => "whisper.cpp",
            Backend::Mlx => "MLX",
        }
    }

    pub fn status(self) -> EngineStatus {
        self.status_in(crate::model_manager::supported_providers())
    }

    pub(crate) fn status_in(self, compiled: &[&str]) -> EngineStatus {
        if compiled.contains(&self.provider()) {
            return EngineStatus {
                ready: true,
                note: None,
            };
        }
        let note = match self {
            Backend::SherpaOnnx => "Движка sherpa-onnx в этой сборке нет.",
            Backend::WhisperCpp => "Движка whisper.cpp в этой сборке нет.",
            Backend::Mlx => {
                "Нужен движок MLX, а в этой сборке его нет: MLX пока не собирается в Xcode 26.3. Модель найдена, но запустить её приложение не может."
            }
        };
        EngineStatus {
            ready: false,
            note: Some(note.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_round_trip_and_missing_engines_say_so() {
        for backend in [Backend::SherpaOnnx, Backend::WhisperCpp, Backend::Mlx] {
            assert_eq!(Backend::from_provider(backend.provider()), Some(backend));
        }
        assert!(Backend::SherpaOnnx.status_in(&["sherpa-onnx"]).ready);
        let mlx = Backend::Mlx.status_in(&["sherpa-onnx", "apple-speech"]);
        assert!(!mlx.ready);
        assert!(mlx.note.unwrap().contains("MLX"));
        assert_eq!(
            serde_json::to_string(&Backend::WhisperCpp).unwrap(),
            r#""whisper-cpp""#
        );
    }
}
