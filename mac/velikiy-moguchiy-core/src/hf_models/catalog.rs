//! «Рекомендуемые»: the curated models listed in `recommended.json`, each a
//! built-in manifest or a Hugging Face repository pinned to a commit. Editing
//! the list needs no code.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::backend::Backend;
use super::repo::RepoRef;

const CATALOG: &str = include_str!("recommended.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommended {
    pub id: String,
    pub name: String,
    pub description: String,
    pub engine: Backend,
    /// A built-in manifest path such as `sherpa-onnx/ru-gigaam-v3`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// The full commit the repository is installed at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Variant id or label; the repository's suggested variant when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// Bytes, shown before anything is asked from Hugging Face.
    pub size: u64,
    pub quality: String,
    pub speed: String,
    pub streaming: bool,
    pub languages: String,
    pub license: String,
}

#[derive(Debug, Deserialize)]
struct Catalog {
    models: Vec<Recommended>,
}

pub fn recommended() -> Vec<Recommended> {
    serde_json::from_str::<Catalog>(CATALOG)
        .map(|c| c.models)
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize)]
pub struct RecommendedState {
    #[serde(flatten)]
    pub entry: Recommended,
    pub engine_ready: bool,
    pub engine_note: Option<String>,
    /// Where it is installed, relative to the models folder.
    pub model_path: Option<String>,
    /// 0 not downloaded, 1 partly, 2 ready.
    pub status: i32,
}

fn matches_variant(source: &serde_json::Value, wanted: Option<&str>) -> bool {
    let Some(wanted) = wanted else {
        return true;
    };
    let field = |key: &str| source.get(key).and_then(|v| v.as_str()).unwrap_or("");
    field("variant") == wanted || field("label") == wanted
}

/// The models of the catalog with what this Mac has of them.
pub fn recommended_states(models_root: &Path, compiled: &[&str]) -> Vec<RecommendedState> {
    let installed = crate::model_manager::scan_models_in(models_root);
    recommended()
        .into_iter()
        .map(|entry| {
            let engine = entry.engine.status_in(compiled);
            let found = match (&entry.builtin, &entry.repo, &entry.revision) {
                (Some(builtin), _, _) => installed
                    .iter()
                    .find(|m| m.path == models_root.join(builtin)),
                (None, Some(repo), Some(revision)) => installed.iter().find(|m| {
                    crate::model_manager::hf_source(&m.path).is_some_and(|source| {
                        source.get("repo").and_then(|r| r.as_str()) == Some(repo.as_str())
                            && source.get("sha").and_then(|s| s.as_str()) == Some(revision.as_str())
                            && matches_variant(&source, entry.variant.as_deref())
                    })
                }),
                _ => None,
            };
            let model_path = found.map(|m| {
                m.path
                    .strip_prefix(models_root)
                    .unwrap_or(&m.path)
                    .to_string_lossy()
                    .into_owned()
            });
            let status = match found {
                Some(m) if !m.manifest.files.is_empty() || entry.builtin.is_none() => {
                    crate::model_manager::model_status(
                        &m.path,
                        crate::model_manager::VerifyMode::CacheOnly,
                    ) as i32
                }
                _ => 0,
            };
            RecommendedState {
                engine_ready: engine.ready,
                engine_note: engine.note,
                model_path,
                status,
                entry,
            }
        })
        .collect()
}

/// Makes `id` ready to download and returns its model path for
/// `sp_core_download_model`: the built-in manifest, or a Hugging Face
/// repository inspected at its pinned commit.
pub async fn install_recommended(
    client: &reqwest::Client,
    base: &str,
    models_root: &Path,
    id: &str,
    compiled: &[&str],
) -> Result<String, String> {
    let entry = recommended()
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("Такой модели в списке рекомендуемых нет.")?;
    let engine = entry.engine.status_in(compiled);
    if !engine.ready {
        return Err(engine.note.unwrap_or_default());
    }
    if let Some(builtin) = &entry.builtin {
        if crate::model_manager::scan_models_in(models_root)
            .iter()
            .any(|m| m.path == models_root.join(builtin))
        {
            return Ok(builtin.clone());
        }
        return Err("Описание встроенной модели не найдено: перезапустите приложение.".into());
    }
    let (Some(repo), Some(revision)) = (&entry.repo, &entry.revision) else {
        return Err("В списке у модели нет репозитория.".into());
    };
    let (owner, name) = repo
        .split_once('/')
        .ok_or("Неверное имя репозитория в списке.")?;
    let repo_ref = RepoRef {
        owner: owner.into(),
        name: name.into(),
        revision: revision.clone(),
    };
    let inspection = super::inspect_with(client, base, &repo_ref, compiled).await?;
    let variant = match &entry.variant {
        Some(wanted) => inspection
            .variants
            .iter()
            .find(|v| &v.id == wanted || &v.label == wanted),
        None => inspection.variants.first(),
    }
    .ok_or("Модели из списка больше нет в репозитории.")?;
    let variant_id = variant.id.clone();
    super::prepare_install(client, base, models_root, &inspection, &variant_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_catalog_reads_and_every_entry_is_complete() {
        let models = recommended();
        assert!(models.len() >= 5);
        let mut ids = HashSet::new();
        for m in &models {
            assert!(ids.insert(m.id.clone()), "duplicate {}", m.id);
            assert!(!m.name.is_empty() && !m.description.is_empty(), "{}", m.id);
            assert!(m.size > 0, "{}", m.id);
            match (&m.builtin, &m.repo, &m.revision) {
                (Some(builtin), None, None) => {
                    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("src/manifests")
                        .join(format!("{builtin}.json"));
                    assert!(manifest.is_file(), "{} names a missing manifest", m.id);
                    let text = std::fs::read_to_string(manifest).unwrap();
                    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                    assert_eq!(parsed["provider"], m.engine.provider(), "{}", m.id);
                }
                (None, Some(repo), Some(revision)) => {
                    assert!(repo.contains('/'), "{}", m.id);
                    assert!(
                        revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()),
                        "{} must pin a full commit",
                        m.id
                    );
                }
                _ => panic!("{} needs either builtin or repo with revision", m.id),
            }
        }
        assert!(models.iter().any(|m| m.engine == Backend::WhisperCpp));
        assert!(models.iter().any(|m| m.engine == Backend::Mlx));
    }

    #[test]
    fn mlx_entries_say_why_they_cannot_install_in_a_build_without_mlx() {
        let root = std::env::temp_dir().join(format!("vm-catalog-{}", std::process::id()));
        let states = recommended_states(&root, &["sherpa-onnx", "whisper-cpp"]);
        let mlx = states
            .iter()
            .find(|s| s.entry.engine == Backend::Mlx)
            .unwrap();
        assert!(!mlx.engine_ready);
        assert!(mlx.engine_note.as_deref().unwrap().contains("MLX"));
        assert!(states
            .iter()
            .all(|s| s.status == 0 && s.model_path.is_none()));
    }
}
