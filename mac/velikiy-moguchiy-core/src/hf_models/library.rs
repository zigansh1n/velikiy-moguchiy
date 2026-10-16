//! «Мои модели»: the speech models this Mac has — downloaded built-ins,
//! models from Hugging Face and folders added in place — and how each one
//! leaves the list.

use std::path::Path;

use serde::Serialize;

use super::backend::Backend;
use crate::model_manager::{self, DiscoveredModel, VerifyMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Builtin,
    HuggingFace,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Removal {
    /// «Удалить»: deletes what was downloaded. A built-in keeps its
    /// description and can be downloaded again.
    Delete,
    /// «Убрать из списка»: the model's own folder stays as it is.
    Unlist,
}

#[derive(Debug, Clone, Serialize)]
pub struct MyModel {
    pub model_path: String,
    pub provider: String,
    pub engine_name: String,
    pub engine_ready: bool,
    pub engine_note: Option<String>,
    pub title: String,
    pub description: String,
    pub origin: Origin,
    pub removal: Removal,
    /// 1 partly downloaded, 2 ready.
    pub status: i32,
    pub size: u64,
    /// The model `asr.provider` and its `model` key name.
    pub primary: bool,
    /// Where an added model lives.
    pub location: Option<String>,
    /// An added model whose folder is gone.
    pub missing: bool,
    pub streaming: Option<bool>,
    pub license: Option<serde_json::Value>,
    pub languages: Vec<String>,
}

fn section(model: &DiscoveredModel, key: &str) -> Option<serde_json::Value> {
    model_manager::manifest_section(&model.path, key)
}

fn text(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Every speech model that is downloaded, partly downloaded or added, the
/// primary first. `primary` is `(asr.provider, asr.<provider>.model)`.
pub fn my_models(
    models_root: &Path,
    compiled: &[&str],
    primary: Option<(&str, &str)>,
) -> Vec<MyModel> {
    let mut list: Vec<MyModel> = model_manager::scan_models_in(models_root)
        .into_iter()
        .filter(|m| m.manifest.mode.as_deref().is_none_or(|mode| mode == "asr"))
        .filter_map(|m| describe(models_root, compiled, primary, &m))
        .collect();
    list.sort_by(|a, b| {
        b.primary
            .cmp(&a.primary)
            .then_with(|| a.title.cmp(&b.title))
    });
    list
}

fn describe(
    models_root: &Path,
    compiled: &[&str],
    primary: Option<(&str, &str)>,
    model: &DiscoveredModel,
) -> Option<MyModel> {
    let model_path = model
        .path
        .strip_prefix(models_root)
        .ok()?
        .to_string_lossy()
        .into_owned();
    let external = model_manager::external_source(&model.path);
    let status = model_manager::model_status(&model.path, VerifyMode::CacheOnly) as i32;
    if status == 0 && external.is_none() {
        return None;
    }
    let provider = model.manifest.provider.clone();
    let backend = Backend::from_provider(&provider);
    let engine = match backend {
        Some(backend) => backend.status_in(compiled),
        None => super::backend::EngineStatus {
            ready: compiled.contains(&provider.as_str()),
            note: None,
        },
    };
    let (origin, details) = if let Some(details) = section(model, "external") {
        (Origin::External, Some(details))
    } else if let Some(details) = section(model, "huggingface") {
        (Origin::HuggingFace, Some(details))
    } else {
        (Origin::Builtin, None)
    };
    let detail = |key: &str| details.as_ref().and_then(|d| text(d, key));
    let description = model.manifest.description.clone();
    let title = match origin {
        Origin::Builtin => description
            .split(" — ")
            .next()
            .unwrap_or(&description)
            .to_string(),
        _ => description
            .rsplit_once(" — ")
            .map(|(head, _)| head.to_string())
            .unwrap_or_else(|| detail("title").unwrap_or_else(|| description.clone())),
    };
    let size = match origin {
        Origin::External => details
            .as_ref()
            .and_then(|d| d.get("size"))
            .and_then(|s| s.as_u64())
            .unwrap_or(0),
        _ => model.manifest.files.iter().map(|f| f.size).sum(),
    };
    let missing = external.as_ref().is_some_and(|e| !e.location.is_dir());
    Some(MyModel {
        primary: primary
            .is_some_and(|(p, m)| p == provider && m.trim_end_matches('/') == model_path),
        engine_name: backend
            .map(|b| b.display_name().to_string())
            .unwrap_or_else(|| provider.clone()),
        engine_ready: engine.ready,
        engine_note: engine.note,
        provider,
        title,
        description,
        removal: if origin == Origin::External {
            Removal::Unlist
        } else {
            Removal::Delete
        },
        origin,
        status,
        size,
        location: external.map(|e| e.location.to_string_lossy().into_owned()),
        missing,
        streaming: details
            .as_ref()
            .and_then(|d| d.get("streaming"))
            .and_then(|s| s.as_bool()),
        license: details.as_ref().and_then(|d| d.get("license")).cloned(),
        languages: details
            .as_ref()
            .and_then(|d| d.get("languages"))
            .and_then(|l| l.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        model_path,
    })
}

/// Deletes a downloaded model or takes an added one out of the list, as
/// [`MyModel::removal`] says. Returns that removal.
pub fn remove(models_root: &Path, model_path: &str) -> Result<Removal, String> {
    let rel = Path::new(model_path);
    if model_path.is_empty()
        || rel.is_absolute()
        || rel
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("Такой модели нет.".into());
    }
    let model_dir = models_root.join(rel);
    if model_manager::external_source(&model_dir).is_some() {
        super::local::remove_external(models_root, model_path)?;
        return Ok(Removal::Unlist);
    }
    if model_manager::hf_source(&model_dir).is_some() {
        super::remove_user_model(models_root, model_path)?;
        return Ok(Removal::Delete);
    }
    model_manager::remove_model_files(&model_dir).map_err(|e| e.to_string())?;
    Ok(Removal::Delete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vm-library-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest(dir: &Path, json: serde_json::Value) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(".koe-manifest.json"), json.to_string()).unwrap();
    }

    #[test]
    fn the_list_has_what_is_downloaded_or_added_and_says_how_it_leaves() {
        let root = temp("list");
        let file = |name: &str, data: &[u8]| serde_json::json!({"name": name, "size": data.len(), "sha256": "", "url": "https://example.invalid/x"});
        manifest(
            &root.join("sherpa-onnx/ru-small"),
            serde_json::json!({
                "provider": "sherpa-onnx", "mode": "asr", "description": "Vosk small — русская потоковая",
                "repo": "x", "files": [file("model.onnx", b"abc")],
            }),
        );
        fs::write(root.join("sherpa-onnx/ru-small/model.onnx"), b"abc").unwrap();
        manifest(
            &root.join("sherpa-onnx/not-downloaded"),
            serde_json::json!({
                "provider": "sherpa-onnx", "mode": "asr", "description": "Другая", "repo": "x",
                "files": [file("model.onnx", b"abc")],
            }),
        );
        manifest(
            &root.join("mlx/llm"),
            serde_json::json!({
                "provider": "mlx", "mode": "llm", "description": "LLM", "repo": "x", "files": [],
            }),
        );
        let gone = root.join("gone");
        manifest(
            &root.join("external/gone-1234"),
            serde_json::json!({
                "provider": "whisper-cpp", "mode": "asr", "description": "ggml-base · base — на этом Mac",
                "repo": "ggml-base", "files": [],
                "external": {"location": gone.to_string_lossy(), "variant": "ggml-base.bin", "size": 147951465u64},
            }),
        );

        let list = my_models(
            &root,
            &["sherpa-onnx"],
            Some(("sherpa-onnx", "sherpa-onnx/ru-small/")),
        );
        let paths: Vec<&str> = list.iter().map(|m| m.model_path.as_str()).collect();
        assert_eq!(paths, vec!["sherpa-onnx/ru-small", "external/gone-1234"]);
        let builtin = &list[0];
        assert!(builtin.primary);
        assert_eq!(
            (builtin.origin, builtin.removal, builtin.status),
            (Origin::Builtin, Removal::Delete, 2)
        );
        assert_eq!(builtin.title, "Vosk small");
        let external = &list[1];
        assert_eq!(
            (external.origin, external.removal),
            (Origin::External, Removal::Unlist)
        );
        assert!(external.missing && !external.engine_ready);
        assert_eq!(external.title, "ggml-base · base");
        assert_eq!(external.size, 147951465);
        assert_eq!(external.engine_name, "whisper.cpp");

        assert_eq!(
            remove(&root, "external/gone-1234").unwrap(),
            Removal::Unlist
        );
        assert_eq!(
            remove(&root, "sherpa-onnx/ru-small").unwrap(),
            Removal::Delete
        );
        assert!(
            root.join("sherpa-onnx/ru-small/.koe-manifest.json")
                .exists(),
            "a built-in can come back"
        );
        assert!(!root.join("sherpa-onnx/ru-small/model.onnx").exists());
        assert!(my_models(&root, &["sherpa-onnx"], None).is_empty());
        for outside in ["/etc", "../x", "sherpa-onnx/../mlx/llm", ""] {
            assert!(remove(&root, outside).is_err(), "{outside}");
        }
        let _ = fs::remove_dir_all(&root);
    }
}
