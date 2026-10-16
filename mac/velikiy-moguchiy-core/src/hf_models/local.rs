//! «На этом Mac»: speech models already on disk — the Hugging Face cache,
//! whisper.cpp folders, sherpa-onnx downloads, a folder the user picks — used
//! where they are. Scanning only reads; adding one writes a small manifest
//! under `models/external/` that points at the folder.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::analysis::{self, Problem, RepoHints, Variant};
use super::card::{self, Languages, LicenseInfo};
use super::detect::RepoFile;
use super::onnx_meta;

/// Added models live under this folder of the models directory.
pub const EXTERNAL_DIR: &str = "external";
const MANIFEST_FILE: &str = ".koe-manifest.json";
const MAX_FILES: usize = 600;
const MAX_DEPTH: usize = 3;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct LocalFind {
    /// Absolute folder the model is in.
    pub location: String,
    /// «aystream/GigaAM-v3-e2e-rnnt-mlx» for the Hugging Face cache, else the folder name.
    pub name: String,
    /// "hf_cache" or "folder".
    pub source: &'static str,
    pub variants: Vec<Variant>,
    pub problem: Option<Problem>,
    pub languages: Languages,
    pub license: LicenseInfo,
    pub warnings: Vec<String>,
    /// A model of «Рекомендуемые» that runs the same thing on an engine this
    /// build has, when this one cannot run.
    pub alternative: Option<String>,
    /// Model paths already in the list for this folder, by variant id.
    pub added: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    /// `hub/models--owner--name/snapshots/<commit>/`.
    HfCache,
    /// Folders with models in them, or a model itself.
    Folders,
}

/// Where models usually are. The Downloads, Documents and Desktop folders are
/// left out: macOS asks the user for permission before anyone reads them.
pub fn default_roots(home: &Path) -> Vec<(PathBuf, RootKind)> {
    let hub = std::env::var_os("HF_HUB_CACHE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HF_HOME").map(|h| PathBuf::from(h).join("hub")))
        .unwrap_or_else(|| home.join(".cache/huggingface/hub"));
    let mut roots = vec![(hub, RootKind::HfCache)];
    for folder in [
        home.join("whisper.cpp/models"),
        home.join(".cache/whisper.cpp"),
        PathBuf::from("/opt/homebrew/share/whisper-cpp"),
        PathBuf::from("/usr/local/share/whisper-cpp"),
        home.join("models"),
        home.join("Models"),
    ] {
        roots.push((folder, RootKind::Folders));
    }
    if let Ok(entries) = std::fs::read_dir(home) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let dir = entry.file_type().is_ok_and(|t| t.is_dir());
            if dir && name.starts_with("sherpa-onnx") {
                roots.push((entry.path(), RootKind::Folders));
            }
        }
    }
    roots
}

/// Files under `dir`, relative to it, following file links (the Hugging
/// Face cache links its files to blobs) but not folder links.
fn list_files(dir: &Path) -> Vec<RepoFile> {
    fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<RepoFile>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if out.len() >= MAX_FILES {
                return;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth < MAX_DEPTH {
                    walk(root, &path, depth + 1, out);
                }
                continue;
            }
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(RepoFile {
                    path: rel.to_string_lossy().replace('\\', "/"),
                    size: meta.len(),
                    sha256: None,
                    git_sha1: None,
                });
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, dir, 1, &mut files);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn read_tail(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(onnx_meta::TAIL_BYTES)))
        .map_err(|e| e.to_string())?;
    let mut tail = Vec::new();
    file.take(onnx_meta::TAIL_BYTES)
        .read_to_end(&mut tail)
        .map_err(|e| e.to_string())?;
    Ok(tail)
}

/// whisper.cpp files start with the magic `ggml` as a little-endian word.
fn is_ggml(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && magic == *b"lmgg"
}

/// The YAML front matter of a model card, as the Hugging Face API returns it
/// in `cardData`.
fn card_from_readme(dir: &Path, files: &[RepoFile]) -> Option<serde_json::Value> {
    let readme = files
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case("README.md") && f.size <= MAX_CONFIG_BYTES)?;
    let text = std::fs::read_to_string(dir.join(&readme.path)).ok()?;
    let rest = text.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&rest[..end]).ok()?;
    serde_json::to_value(yaml).ok().filter(|v| v.is_object())
}

/// What the folder holds, read from the disk alone.
pub fn analyze_folder(
    dir: &Path,
    name: &str,
    source: &'static str,
    compiled: &[&str],
) -> LocalFind {
    let mut files = list_files(dir);
    files.retain(|f| {
        let lower = f.path.to_ascii_lowercase();
        !(lower.starts_with("ggml-") && lower.ends_with(".bin")) || is_ggml(&dir.join(&f.path))
    });
    let card = card_from_readme(dir, &files);
    let text_list = |key: &str| -> Vec<String> {
        card.as_ref()
            .and_then(|c| c.get(key))
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let text = |key: &str| {
        card.as_ref()
            .and_then(|c| c.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let hints = RepoHints {
        name: name.rsplit('/').next().unwrap_or(name).to_string(),
        library: text("library_name"),
        tags: text_list("tags"),
        pipeline_tag: text("pipeline_tag"),
    };

    let mut metadata: HashMap<String, analysis::MetadataRead> = HashMap::new();
    for path in analysis::metadata_wanted(&files) {
        let read = read_tail(&dir.join(&path)).map(|tail| onnx_meta::metadata_from_tail(&tail));
        metadata.insert(path, read);
    }
    let config = analysis::config_wanted(&files, &hints)
        .map(|p| dir.join(p))
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() <= MAX_CONFIG_BYTES))
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let found = analysis::analyze(&files, &metadata, config.as_ref(), &hints, compiled);

    let read: Vec<&BTreeMap<String, String>> = if found.variants.is_empty() {
        metadata.values().filter_map(|r| r.as_ref().ok()).collect()
    } else {
        found
            .variants
            .iter()
            .filter_map(|v| metadata.get(&v.primary)?.as_ref().ok())
            .collect()
    };
    let mut languages = card::languages(card.as_ref(), &read, &hints.name);
    if languages.has_russian.is_none() && found.variants.iter().any(|v| v.is_multilingual_whisper())
    {
        languages = Languages::whisper();
    }
    let license = card::license_from_card(card.as_ref(), &hints.tags)
        .or_else(|| card::license_from_file_names(files.iter().map(|f| f.path.as_str())))
        .or_else(|| {
            files
                .iter()
                .find(|f| {
                    analysis::is_root_license(&f.path) && f.size <= analysis::MAX_LICENSE_BYTES
                })
                .and_then(|f| std::fs::read(dir.join(&f.path)).ok())
                .and_then(|bytes| card::license_from_text(&String::from_utf8_lossy(&bytes)))
        })
        .or_else(|| read.iter().find_map(|m| card::license_from_onnx(m)))
        .unwrap_or_else(LicenseInfo::unknown);
    let warnings = if found.variants.is_empty() {
        Vec::new()
    } else {
        license.warning().into_iter().collect()
    };
    let alternative = alternative(&found.variants);
    LocalFind {
        location: std::fs::canonicalize(dir)
            .unwrap_or_else(|_| dir.to_path_buf())
            .to_string_lossy()
            .into_owned(),
        name: name.to_string(),
        source,
        variants: found.variants,
        problem: found.problem,
        languages,
        license,
        alternative,
        warnings,
        added: BTreeMap::new(),
    }
}

fn alternative(variants: &[Variant]) -> Option<String> {
    if variants.iter().any(|v| v.engine_ready) {
        return None;
    }
    let id = variants.iter().find_map(|v| match v.layout {
        analysis::ModelLayout::Mlx {
            kind: analysis::MlxKind::GigaAm,
        } => Some("gigaam-v3-rnnt-punct"),
        analysis::ModelLayout::Mlx {
            kind: analysis::MlxKind::Whisper,
        } => Some("whisper-large-v3-turbo-q5"),
        _ => None,
    })?;
    super::catalog::recommended()
        .iter()
        .any(|r| r.id == id)
        .then(|| id.to_string())
}

/// A speech model, or a speech format this app names but cannot run.
fn worth_showing(find: &LocalFind) -> bool {
    !find.variants.is_empty()
        || find
            .problem
            .as_ref()
            .is_some_and(|p| matches!(p.code.as_str(), "faster_whisper" | "unsupported_family"))
}

fn looks_like_model_dir(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        name.ends_with(".onnx")
            || (name.starts_with("ggml-") && name.ends_with(".bin"))
            || name.ends_with(".safetensors")
            || name == "model.bin"
    })
}

/// Model folders at or below `dir`, a few levels deep.
fn model_dirs(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if out.len() >= 200 {
        return;
    }
    if looks_like_model_dir(dir) {
        out.push(dir.to_path_buf());
        return;
    }
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir())
            && !entry.file_name().to_string_lossy().starts_with('.')
        {
            model_dirs(&entry.path(), depth + 1, out);
        }
    }
}

/// The snapshot a cached repository uses: the one `refs/main` names, else
/// the newest.
fn cached_snapshot(repo_dir: &Path) -> Option<PathBuf> {
    let snapshots = repo_dir.join("snapshots");
    if let Ok(commit) = std::fs::read_to_string(repo_dir.join("refs/main")) {
        let dir = snapshots.join(commit.trim());
        if dir.is_dir() {
            return Some(dir);
        }
    }
    std::fs::read_dir(&snapshots)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

/// Every speech model found under `roots`, skipping the app's own models.
pub fn scan(
    roots: &[(PathBuf, RootKind)],
    models_root: &Path,
    compiled: &[&str],
) -> Vec<LocalFind> {
    let own = std::fs::canonicalize(models_root).ok();
    let mut finds = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (root, kind) in roots {
        match kind {
            RootKind::HfCache => {
                let Ok(entries) = std::fs::read_dir(root) else {
                    continue;
                };
                let mut repos: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with("models--"))
                    })
                    .collect();
                repos.sort();
                for repo_dir in repos {
                    let folder = repo_dir.file_name().unwrap().to_string_lossy().into_owned();
                    let name = folder.trim_start_matches("models--").replacen("--", "/", 1);
                    let Some(snapshot) = cached_snapshot(&repo_dir) else {
                        continue;
                    };
                    let find = analyze_folder(&snapshot, &name, "hf_cache", compiled);
                    if worth_showing(&find) && seen.insert(find.location.clone()) {
                        finds.push(find);
                    }
                }
            }
            RootKind::Folders => {
                let mut dirs = Vec::new();
                model_dirs(root, 1, &mut dirs);
                for dir in dirs {
                    let canonical = std::fs::canonicalize(&dir).ok();
                    if canonical.is_some() && canonical == own
                        || own
                            .as_ref()
                            .zip(canonical.as_ref())
                            .is_some_and(|(o, c)| c.starts_with(o))
                    {
                        continue;
                    }
                    let name = dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let find = analyze_folder(&dir, &name, "folder", compiled);
                    if worth_showing(&find) && seen.insert(find.location.clone()) {
                        finds.push(find);
                    }
                }
            }
        }
    }
    mark_added(&mut finds, models_root);
    finds
}

/// What a folder the user picked holds: the folder itself or the model
/// folders inside it. Unlike [`scan`], says why when there is nothing.
pub fn inspect_folder(dir: &Path, models_root: &Path, compiled: &[&str]) -> Vec<LocalFind> {
    let mut dirs = Vec::new();
    model_dirs(dir, 1, &mut dirs);
    if dirs.is_empty() {
        dirs.push(dir.to_path_buf());
    }
    let mut finds: Vec<LocalFind> = dirs
        .iter()
        .map(|d| {
            let name = d
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            analyze_folder(d, &name, "folder", compiled)
        })
        .collect();
    if finds.len() > 1 {
        finds.retain(worth_showing);
    }
    mark_added(&mut finds, models_root);
    finds
}

fn mark_added(finds: &mut [LocalFind], models_root: &Path) {
    let added: Vec<(String, String, String)> =
        crate::model_manager::scan_models_in(&models_root.join(EXTERNAL_DIR))
            .iter()
            .filter_map(|m| {
                let external = crate::model_manager::external_source(&m.path)?;
                let rel = m
                    .path
                    .strip_prefix(models_root)
                    .ok()?
                    .to_string_lossy()
                    .into_owned();
                Some((
                    external.location.to_string_lossy().into_owned(),
                    external.variant,
                    rel,
                ))
            })
            .collect();
    for find in finds {
        for (location, variant, rel) in &added {
            if *location == find.location {
                find.added.insert(variant.clone(), rel.clone());
            }
        }
    }
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches(|c| c == '-' || c == '.')
        .chars()
        .take(48)
        .collect()
}

/// Adds variant `variant_id` of the model in `location` to the model list
/// without copying it, and returns its model path. Nothing in `location`
/// is written.
pub fn add_external(
    models_root: &Path,
    location: &Path,
    variant_id: &str,
    compiled: &[&str],
) -> Result<String, String> {
    let location =
        std::fs::canonicalize(location).map_err(|_| "Папка модели не найдена.".to_string())?;
    if !location.is_dir() {
        return Err("Выберите папку, в которой лежит модель.".into());
    }
    let name = location
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cache_name = location.ancestors().find_map(|a| {
        a.file_name()?
            .to_str()?
            .strip_prefix("models--")
            .map(|n| n.replacen("--", "/", 1))
    });
    let shown = cache_name.clone().unwrap_or(name.clone());
    let find = analyze_folder(&location, &shown, "folder", compiled);
    let variant = find
        .variants
        .iter()
        .find(|v| v.id == variant_id)
        .ok_or("В папке нет такой модели.")?;
    if !variant.engine_ready {
        return Err(variant.engine_note.clone().unwrap_or_default());
    }

    let digest = super::sha256_hex(format!("{}\n{variant_id}", location.display()).as_bytes());
    let dir_name = format!("{}-{}", slug(&shown), &digest[..8]);
    let rel = format!("{EXTERNAL_DIR}/{dir_name}");
    let stub = super::create_contained_dir(models_root, &rel)?;
    let mut title = variant.title.clone();
    if !variant.label.is_empty() && find.variants.len() > 1 {
        title = format!("{title} · {}", variant.label);
    }
    let added_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut manifest = serde_json::json!({
        "provider": variant.backend.provider(),
        "mode": "asr",
        "description": format!("{shown} · {title} — на этом Mac"),
        "repo": shown,
        "files": [],
        "external": {
            "location": location.to_string_lossy(),
            "variant": variant.id,
            "title": variant.title,
            "streaming": variant.streaming,
            "size": variant.total_size,
            "license": find.license,
            "languages": find.languages.codes,
            "added_at": added_at,
        },
    });
    super::add_layout_section(&mut manifest, &variant.layout);
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    super::write_atomically(&stub.join(MANIFEST_FILE), text.as_bytes())?;
    Ok(rel)
}

/// Takes an added model out of the list. Its folder is not touched.
pub fn remove_external(models_root: &Path, model_path: &str) -> Result<(), String> {
    let root = std::fs::canonicalize(models_root.join(EXTERNAL_DIR))
        .map_err(|_| "Такой модели нет.".to_string())?;
    let stub = std::fs::canonicalize(models_root.join(model_path))
        .map_err(|_| "Такой модели нет.".to_string())?;
    if !stub.starts_with(&root) || stub == root || stub.parent() != Some(root.as_path()) {
        return Err("Убрать из списка можно только добавленную модель.".into());
    }
    if crate::model_manager::external_source(&stub).is_none() {
        return Err("Убрать из списка можно только добавленную модель.".into());
    }
    std::fs::remove_file(stub.join(MANIFEST_FILE)).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(stub.join(".DS_Store"));
    std::fs::remove_dir(&stub).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const ENGINES: &[&str] = &["sherpa-onnx", "whisper-cpp", "mlx"];

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vm-local-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn onnx_with(meta: &[(&str, &str)]) -> Vec<u8> {
        let mut body = vec![7u8; 5000];
        body.extend(onnx_meta::encode_entries(meta));
        body
    }

    const GIGAAM_CTC: &[(&str, &str)] = &[
        ("model_type", "EncDecCTCModel"),
        ("vocab_size", "257"),
        ("subsampling_factor", "4"),
        ("is_giga_am", "1"),
        ("language", "Russian"),
    ];

    /// The layout of the Hugging Face cache: snapshot files linked to blobs.
    fn cached_repo(hub: &Path, owner: &str, name: &str, files: &[(&str, Vec<u8>)]) -> PathBuf {
        let repo = hub.join(format!("models--{owner}--{name}"));
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let snapshot = repo.join("snapshots").join(commit);
        fs::create_dir_all(repo.join("blobs")).unwrap();
        fs::create_dir_all(repo.join("refs")).unwrap();
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(repo.join("refs/main"), commit).unwrap();
        for (i, (path, data)) in files.iter().enumerate() {
            let blob = repo.join("blobs").join(format!("blob{i}"));
            fs::write(&blob, data).unwrap();
            let link = snapshot.join(path);
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&blob, &link).unwrap();
            #[cfg(not(unix))]
            fs::copy(&blob, &link).unwrap();
        }
        snapshot
    }

    #[test]
    fn the_hf_cache_is_read_through_its_links_and_only_speech_models_show() {
        let home = temp("cache");
        let hub = home.join(".cache/huggingface/hub");
        cached_repo(
            &hub,
            "someone",
            "gigaam-ctc-sherpa",
            &[
                ("model.int8.onnx", onnx_with(GIGAAM_CTC)),
                ("tokens.txt", b"<blk> 0\n".to_vec()),
            ],
        );
        cached_repo(
            &hub,
            "aystream",
            "GigaAM-v3-e2e-rnnt-mlx",
            &[
                (
                    "config.json",
                    br#"{"model_type": "gigaam", "framework": "mlx"}"#.to_vec(),
                ),
                ("weights.safetensors", vec![0u8; 100]),
                ("tokenizer.model", vec![1u8; 10]),
            ],
        );
        cached_repo(
            &hub,
            "cointegrated",
            "rubert-base-cased-nli-threeway",
            &[
                ("config.json", br#"{"model_type": "bert"}"#.to_vec()),
                ("model.safetensors", vec![0u8; 100]),
            ],
        );
        let models_root = home.join(".velikiy-moguchiy/models");
        let finds = scan(&[(hub.clone(), RootKind::HfCache)], &models_root, ENGINES);
        let names: Vec<&str> = finds.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "aystream/GigaAM-v3-e2e-rnnt-mlx",
                "someone/gigaam-ctc-sherpa"
            ]
        );
        let mlx = &finds[0];
        assert!(!mlx.variants[0].engine_ready);
        assert_eq!(mlx.alternative.as_deref(), Some("gigaam-v3-rnnt-punct"));
        assert!(mlx.variants[0]
            .engine_note
            .as_deref()
            .unwrap()
            .contains("только Qwen3-ASR"));
        let sherpa = &finds[1];
        assert_eq!(sherpa.source, "hf_cache");
        assert!(sherpa.variants[0].engine_ready);
        assert_eq!(sherpa.languages.codes, vec!["ru"]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn whisper_cpp_folders_are_checked_by_magic_not_by_name() {
        let home = temp("ggml");
        let models = home.join("whisper.cpp/models");
        fs::create_dir_all(&models).unwrap();
        let mut ggml = b"lmgg".to_vec();
        ggml.extend(vec![0u8; 100]);
        fs::write(models.join("ggml-large-v3-turbo-q5_0.bin"), &ggml).unwrap();
        fs::write(models.join("ggml-base.bin"), b"<html>not a model</html>").unwrap();
        let finds = scan(
            &[(models.clone(), RootKind::Folders)],
            &home.join("app-models"),
            ENGINES,
        );
        assert_eq!(finds.len(), 1);
        let labels: Vec<&str> = finds[0].variants.iter().map(|v| v.label.as_str()).collect();
        assert_eq!(labels, vec!["large-v3-turbo-q5_0"]);
        assert_eq!(finds[0].languages.has_russian, Some(true));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_found_model_is_added_in_place_and_removed_without_touching_it() {
        let home = temp("add");
        let hub = home.join("hub");
        let snapshot = cached_repo(
            &hub,
            "someone",
            "gigaam-ctc-sherpa",
            &[
                ("model.int8.onnx", onnx_with(GIGAAM_CTC)),
                ("tokens.txt", b"<blk> 0\n".to_vec()),
            ],
        );
        let models_root = home.join("models");
        let find = &scan(&[(hub.clone(), RootKind::HfCache)], &models_root, ENGINES)[0];
        let variant = find.variants[0].id.clone();
        let rel = add_external(&models_root, Path::new(&find.location), &variant, ENGINES).unwrap();
        assert!(
            rel.starts_with("external/someone-gigaam-ctc-sherpa-"),
            "{rel}"
        );

        let stub = models_root.join(&rel);
        let source = crate::model_manager::external_source(&stub).unwrap();
        assert_eq!(source.location, fs::canonicalize(&snapshot).unwrap());
        let layout = crate::model_manager::sherpa_layout(&stub).unwrap();
        let resolved = layout.resolve_following_links(&source.location).unwrap();
        assert_eq!(resolved.family, velikiy_moguchiy_asr::SherpaFamily::NemoCtc);
        assert!(
            layout.resolve(&source.location).is_err(),
            "cache files are links"
        );

        let again = scan(&[(hub.clone(), RootKind::HfCache)], &models_root, ENGINES);
        assert_eq!(again[0].added.get(&variant), Some(&rel));

        remove_external(&models_root, &rel).unwrap();
        assert!(!stub.exists());
        assert!(
            snapshot.join("model.int8.onnx").exists(),
            "the model itself stays"
        );
        assert!(remove_external(&models_root, "external").is_err());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_picked_folder_without_a_model_says_why() {
        let dir = temp("empty-pick");
        fs::write(dir.join("model.safetensors"), b"x").unwrap();
        fs::write(dir.join("config.json"), br#"{"model_type": "wav2vec2"}"#).unwrap();
        let finds = inspect_folder(&dir, &dir.join("models"), ENGINES);
        assert_eq!(finds.len(), 1);
        assert!(finds[0].variants.is_empty());
        assert_eq!(finds[0].problem.as_ref().unwrap().code, "pytorch");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_roots_skip_folders_macos_guards() {
        let home = temp("roots");
        for dir in ["sherpa-onnx-zipformer-ru", "Downloads", "Documents", "Desktop"] {
            fs::create_dir_all(home.join(dir)).unwrap();
        }
        let roots = default_roots(&home);
        assert_eq!(roots[0].1, RootKind::HfCache);
        assert!(roots.contains(&(home.join("sherpa-onnx-zipformer-ru"), RootKind::Folders)));
        for guarded in ["Downloads", "Documents", "Desktop"] {
            assert!(!roots.iter().any(|(r, _)| r.starts_with(home.join(guarded))));
        }
        let _ = fs::remove_dir_all(&home);
    }
}
