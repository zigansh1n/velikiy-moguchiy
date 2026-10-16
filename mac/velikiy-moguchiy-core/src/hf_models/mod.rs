//! Speech models from Hugging Face: what a repository holds, which engine
//! can run it, and installing it as a user model in
//! `models/hf/<owner>/<name>@<commit>/`, pinned to the commit it was
//! inspected at. The large files are then fetched by
//! [`crate::model_manager::download_model`] like any built-in model.

pub mod analysis;
pub mod backend;
pub mod card;
pub mod catalog;
pub mod check;
pub mod detect;
pub mod ffi;
pub mod library;
pub mod local;
pub mod onnx_meta;
pub mod repo;
pub mod search;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};

use analysis::{is_root_license, RepoHints, MAX_LICENSE_BYTES};
pub use analysis::{ModelLayout, Problem, Variant};
use card::{Languages, LicenseInfo};
use detect::{Rejection, RepoFile};
use repo::{RepoInfo, RepoRef};

pub const DEFAULT_ENDPOINT: &str = "https://huggingface.co";
/// User models live under this folder of the models directory.
pub const USER_MODELS_DIR: &str = "hf";
const MANIFEST_FILE: &str = ".koe-manifest.json";

/// Git keeps no file above 10 MB outside LFS on Hugging Face.
const MAX_GIT_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_API_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const METADATA_READS_AT_ONCE: usize = 4;

/// `HF_ENDPOINT` when set (a mirror), else huggingface.co.
pub fn endpoint() -> String {
    std::env::var("HF_ENDPOINT")
        .ok()
        .map(|e| e.trim().trim_end_matches('/').to_string())
        .filter(|e| e.starts_with("https://") || e.starts_with("http://"))
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string())
}

pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("velikiy-moguchiy/1.0")
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("http client: {e}"))
}

#[derive(Debug, Clone, Serialize)]
pub struct Inspection {
    pub repo: String,
    pub revision: String,
    /// The commit everything below was read at.
    pub sha: String,
    pub page_url: String,
    /// What can be installed, the suggested one first.
    pub variants: Vec<Variant>,
    /// Why nothing can be installed, when `variants` is empty.
    pub problem: Option<Problem>,
    pub rejected: Vec<Rejection>,
    pub license: LicenseInfo,
    pub languages: Languages,
    pub warnings: Vec<String>,
    /// «<name> sherpa-onnx», for the user to search with.
    pub search_phrase: String,
    pub search_url: String,
    /// Repositories that look like sherpa-onnx versions of this one.
    pub alternatives: Vec<String>,
    pub repo_file_count: usize,
}

fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|s| urlencoding::encode(s).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Link to a file of `repo` at commit `sha`.
pub fn file_url(base: &str, repo: &str, sha: &str, path: &str) -> String {
    format!("{base}/{repo}/resolve/{sha}/{}", encode_path(path))
}

fn network_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "Hugging Face не ответил вовремя. Проверьте интернет и попробуйте ещё раз.".into()
    } else {
        format!("Не удалось связаться с Hugging Face ({e}). Проверьте интернет.")
    }
}

/// Reads at most `limit` bytes of a response body; `None` when it is longer.
pub(crate) async fn read_limited(
    response: reqwest::Response,
    limit: u64,
) -> Result<Option<Vec<u8>>, String> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| network_error(&e))?;
        if body.len() as u64 + chunk.len() as u64 > limit {
            return Ok(None);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Some(body))
}

pub async fn fetch_info(
    client: &reqwest::Client,
    base: &str,
    repo: &RepoRef,
) -> Result<RepoInfo, String> {
    let url = format!(
        "{base}/api/models/{}/{}/revision/{}?blobs=true",
        repo.owner,
        repo.name,
        urlencoding::encode(&repo.revision)
    );
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| network_error(&e))?;
    let status = response.status();
    if !status.is_success() {
        let code = response
            .headers()
            .get("x-error-code")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        return Err(match status.as_u16() {
            401 | 403 => "Репозиторий не найден или закрыт. Проверьте название.".into(),
            404 if code == "RevisionNotFound" => format!(
                "В репозитории нет ветки, тега или коммита «{}».",
                repo.revision
            ),
            404 => "Репозиторий не найден. Проверьте название.".into(),
            429 => {
                "Hugging Face просит подождать: слишком много запросов. Попробуйте через минуту."
                    .into()
            }
            s => format!("Hugging Face ответил ошибкой {s}. Попробуйте позже."),
        });
    }
    let body = read_limited(response, MAX_API_BYTES)
        .await?
        .ok_or("Hugging Face прислал слишком длинный список файлов.")?;
    serde_json::from_slice(&body).map_err(|e| format!("Hugging Face ответил непонятно: {e}"))
}

pub use analysis::MetadataRead;

/// Metadata of an ONNX file, from its last [`onnx_meta::TAIL_BYTES`].
async fn fetch_metadata(client: &reqwest::Client, url: &str, size: u64) -> MetadataRead {
    // An explicit range: some object stores ignore suffix ranges.
    let start = size.saturating_sub(onnx_meta::TAIL_BYTES);
    let range = if size > 0 {
        format!("bytes={start}-{}", size - 1)
    } else {
        "bytes=0-".to_string()
    };
    let response = client
        .get(url)
        .header("Range", range)
        .send()
        .await
        .map_err(|e| network_error(&e))?
        .error_for_status()
        .map_err(|e| format!("Не удалось прочитать файл модели: {e}"))?;
    let partial = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if !partial && size > onnx_meta::TAIL_BYTES {
        return Err("Сервер отдал файл модели целиком вместо его конца.".into());
    }
    let served_from = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes "))
        .and_then(|v| v.split('-').next())
        .and_then(|v| v.parse::<u64>().ok());
    if partial && served_from.is_some_and(|from| from != start) {
        return Err("Сервер отдал не тот кусок файла модели.".into());
    }
    let tail = read_limited(response, onnx_meta::TAIL_BYTES)
        .await?
        .ok_or("Сервер прислал больше, чем просили.")?;
    Ok(onnx_meta::metadata_from_tail(&tail))
}

/// The text of a small file kept in git, checked against its blob id.
async fn fetch_git_file(
    client: &reqwest::Client,
    url: &str,
    file: &RepoFile,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| network_error(&e))?
        .error_for_status()
        .map_err(|e| format!("{}: {e}", file.path))?;
    let body = read_limited(response, limit)
        .await?
        .ok_or_else(|| format!("{}: файл больше, чем обещал Hugging Face", file.path))?;
    if body.len() as u64 != file.size {
        return Err(format!(
            "{}: получено {} байт вместо {}",
            file.path,
            body.len(),
            file.size
        ));
    }
    if let Some(expected) = &file.git_sha1 {
        let actual = git_blob_sha1(&body);
        if &actual != expected {
            return Err(format!(
                "{}: файл не совпал с версией на Hugging Face",
                file.path
            ));
        }
    }
    Ok(body)
}

/// Git's object id of a file: SHA-1 of `blob <size>\0` and the content.
pub fn git_blob_sha1(content: &[u8]) -> String {
    let mut data = format!("blob {}\0", content.len()).into_bytes();
    data.extend_from_slice(content);
    hex(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &data).as_ref())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// What `repo` holds and whether this app can run it.
pub async fn inspect(
    client: &reqwest::Client,
    base: &str,
    repo: &RepoRef,
) -> Result<Inspection, String> {
    inspect_with(
        client,
        base,
        repo,
        crate::model_manager::supported_providers(),
    )
    .await
}

/// [`inspect`] for a build whose local providers are `compiled`.
pub async fn inspect_with(
    client: &reqwest::Client,
    base: &str,
    repo: &RepoRef,
    compiled: &[&str],
) -> Result<Inspection, String> {
    let info = fetch_info(client, base, repo).await?;
    let sha = info
        .sha
        .clone()
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("Hugging Face не сообщил, на каком коммите репозиторий.")?
        .to_ascii_lowercase();
    let repo_id = if info.id.is_empty() {
        repo.id()
    } else {
        info.id.clone()
    };
    let name = repo_id.split('/').nth(1).unwrap_or(&repo.name).to_string();
    let files = info.files();
    let search_phrase = format!("{} sherpa-onnx", repo::short_name(&name));

    let mut inspection = Inspection {
        repo: repo_id.clone(),
        revision: repo.revision.clone(),
        sha: sha.clone(),
        page_url: format!("{base}/{repo_id}"),
        variants: Vec::new(),
        problem: None,
        rejected: Vec::new(),
        license: LicenseInfo::unknown(),
        languages: Languages::unknown(),
        warnings: Vec::new(),
        search_url: format!(
            "{base}/models?search={}",
            urlencoding::encode(&format!("{} sherpa", repo::short_name(&name)))
        ),
        search_phrase,
        alternatives: Vec::new(),
        repo_file_count: files.len(),
    };

    let mut reads_by_path: HashMap<String, MetadataRead> = HashMap::new();
    if info.disabled {
        inspection.problem = Some(Problem {
            code: "disabled".into(),
            message: "Hugging Face отключил этот репозиторий.".into(),
        });
    } else if info.is_gated() {
        inspection.problem = Some(Problem {
            code: "gated".into(),
            message: "Доступ к модели выдают по запросу на Hugging Face, а приложение не входит в аккаунт и такие модели не скачивает.".into(),
        });
    } else {
        let hints = RepoHints {
            name: name.clone(),
            library: info.library_name.clone(),
            tags: info.tags.clone(),
            pipeline_tag: info.pipeline_tag.clone(),
        };
        let reads: Vec<_> = analysis::metadata_wanted(&files)
            .into_iter()
            .filter_map(|path| files.iter().find(|f| f.path == path))
            .map(|file| {
                let url = file_url(base, &repo_id, &sha, &file.path);
                let path = file.path.clone();
                let size = file.size;
                async move { (path, fetch_metadata(client, &url, size).await) }
            })
            .collect();
        let fetched: Vec<(String, MetadataRead)> = futures_util::stream::iter(reads)
            .buffer_unordered(METADATA_READS_AT_ONCE)
            .collect()
            .await;
        reads_by_path.extend(fetched);
        let mut config = None;
        if let Some(file) = analysis::config_wanted(&files, &hints)
            .and_then(|path| files.iter().find(|f| f.path == path))
        {
            let url = file_url(base, &repo_id, &sha, &file.path);
            if let Ok(body) = fetch_git_file(client, &url, file, MAX_CONFIG_BYTES).await {
                config = serde_json::from_slice::<serde_json::Value>(&body).ok();
            }
        }
        let found = analysis::analyze(&files, &reads_by_path, config.as_ref(), &hints, compiled);
        inspection.variants = found.variants;
        inspection.rejected = found.rejected;
        inspection.problem = found.problem;
    }

    let metadata: HashMap<&String, &BTreeMap<String, String>> = reads_by_path
        .iter()
        .filter_map(|(path, read)| read.as_ref().ok().map(|meta| (path, meta)))
        .collect();
    let primaries: Vec<&BTreeMap<String, String>> = if inspection.variants.is_empty() {
        metadata.values().copied().collect()
    } else {
        inspection
            .variants
            .iter()
            .filter_map(|v| metadata.get(&v.primary).copied())
            .collect()
    };
    let mut license = card::license_from_card(info.card_data.as_ref(), &info.tags)
        .or_else(|| card::license_from_file_names(files.iter().map(|f| f.path.as_str())));
    if license.is_none() {
        if let Some(file) = files
            .iter()
            .find(|f| is_root_license(&f.path) && f.size <= MAX_LICENSE_BYTES && f.sha256.is_none())
        {
            let url = file_url(base, &repo_id, &sha, &file.path);
            if let Ok(text) = fetch_git_file(client, &url, file, MAX_LICENSE_BYTES).await {
                license = card::license_from_text(&String::from_utf8_lossy(&text));
            }
        }
    }
    if license.is_none() {
        license = primaries.iter().find_map(|m| card::license_from_onnx(m));
    }
    inspection.license = license.unwrap_or_else(LicenseInfo::unknown);
    inspection.languages = card::languages(info.card_data.as_ref(), &primaries, &name);
    if inspection.languages.has_russian.is_none()
        && inspection
            .variants
            .iter()
            .any(|v| v.is_multilingual_whisper())
    {
        inspection.languages = Languages::whisper();
    }

    if inspection.problem.is_none() {
        if let Some(warning) = inspection.license.warning() {
            inspection.warnings.push(warning);
        }
        if inspection.languages.has_russian == Some(false) {
            inspection.warnings.push(
                "Русского языка в описании модели нет: русскую речь она, скорее всего, не распознаёт."
                    .into(),
            );
        }
    } else {
        inspection.alternatives = search_alternatives(client, base, &repo_id, &name).await;
    }
    Ok(inspection)
}

/// Repositories found by «<name> sherpa», at most five; nothing on errors.
async fn search_alternatives(
    client: &reqwest::Client,
    base: &str,
    repo_id: &str,
    name: &str,
) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct Hit {
        id: String,
    }
    let query = format!("{} sherpa", repo::short_name(name));
    let url = format!(
        "{base}/api/models?search={}&limit=10",
        urlencoding::encode(&query)
    );
    let Ok(response) = client.get(&url).send().await else {
        return Vec::new();
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let Ok(Some(body)) = read_limited(response, 1024 * 1024).await else {
        return Vec::new();
    };
    serde_json::from_slice::<Vec<Hit>>(&body)
        .unwrap_or_default()
        .into_iter()
        .map(|h| h.id)
        .filter(|id| id != repo_id)
        .take(5)
        .collect()
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
    let out = out.trim_matches(|c| c == '-' || c == '.').to_string();
    out.chars().take(60).collect()
}

/// Where a variant goes, relative to the models directory.
pub fn model_rel_path(inspection: &Inspection, variant: &Variant) -> String {
    let (owner, name) = inspection
        .repo
        .split_once('/')
        .unwrap_or(("hf", &inspection.repo));
    let mut dir = format!("{name}@{}", &inspection.sha[..12]);
    if inspection.variants.len() > 1 {
        let mut tag = slug(&variant.label);
        let shared = inspection
            .variants
            .iter()
            .any(|other| other.id != variant.id && slug(&other.label) == tag);
        if tag.is_empty() || shared {
            let id_hash = &sha256_hex(variant.id.as_bytes())[..8];
            tag = if tag.is_empty() {
                id_hash.to_string()
            } else {
                format!("{tag}-{id_hash}")
            };
        }
        dir = format!("{dir}-{tag}");
    }
    format!("{USER_MODELS_DIR}/{owner}/{dir}")
}

/// Creates `rel` under `root` without following a symlink on the way.
pub(crate) fn create_contained_dir(root: &Path, rel: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let mut current = root.to_path_buf();
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("unsafe path {rel}"));
        }
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(format!("{} is not a folder", current.display())),
            Err(_) => {
                std::fs::create_dir(&current).map_err(|e| format!("{}: {e}", current.display()))?
            }
        }
    }
    Ok(current)
}

pub(crate) fn write_atomically(path: &Path, content: &[u8]) -> Result<(), String> {
    let tmp = path.with_file_name(format!(
        ".{}.tmp.{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp);
    std::fs::write(&tmp, content)
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", path.display())
        })
}

/// Writes the manifest of `variant_id` and its small git files into its
/// folder and returns that folder relative to `models_root`. The model then
/// shows up in the list as not downloaded; [`crate::model_manager::download_model`]
/// fetches and checks the rest, and resumes where a previous attempt stopped.
pub async fn prepare_install(
    client: &reqwest::Client,
    base: &str,
    models_root: &Path,
    inspection: &Inspection,
    variant_id: &str,
) -> Result<String, String> {
    let variant = inspection
        .variants
        .iter()
        .find(|v| v.id == variant_id)
        .ok_or("Этого варианта модели в репозитории нет.")?;
    if !variant.engine_ready {
        return Err(variant
            .engine_note
            .clone()
            .unwrap_or_else(|| "Этой сборке нечем запустить модель.".into()));
    }
    let rel = model_rel_path(inspection, variant);
    let model_dir = create_contained_dir(models_root, &rel)?;

    let mut entries = Vec::new();
    for file in &variant.files {
        let url = file_url(base, &inspection.repo, &inspection.sha, &file.path);
        let sha256 = match &file.sha256 {
            Some(sha256) => sha256.clone(),
            None => {
                let parent = match file.path.rsplit_once('/') {
                    Some((dir, _)) => create_contained_dir(&model_dir, dir)?,
                    None => model_dir.clone(),
                };
                let dest = parent.join(file.path.rsplit('/').next().unwrap_or(&file.path));
                let existing = std::fs::symlink_metadata(&dest)
                    .ok()
                    .filter(|m| m.is_file() && m.len() == file.size)
                    .and_then(|_| std::fs::read(&dest).ok())
                    .filter(|data| {
                        file.git_sha1
                            .as_ref()
                            .is_none_or(|g| &git_blob_sha1(data) == g)
                    });
                let content = match existing {
                    Some(content) => content,
                    None => {
                        let content =
                            fetch_git_file(client, &url, file, MAX_GIT_FILE_BYTES).await?;
                        if std::fs::symlink_metadata(&dest).is_ok_and(|m| !m.is_file()) {
                            return Err(format!("{} is not a file", dest.display()));
                        }
                        write_atomically(&dest, &content)?;
                        content
                    }
                };
                sha256_hex(&content)
            }
        };
        entries.push(serde_json::json!({
            "name": file.path,
            "size": file.size,
            "sha256": sha256,
            "url": url,
        }));
    }

    let (_, name) = inspection
        .repo
        .split_once('/')
        .unwrap_or(("", &inspection.repo));
    let mut description = repo::short_name(name).to_string();
    if inspection.variants.len() > 1 && !variant.label.is_empty() {
        description = format!("{description} · {}", variant.label);
    }
    let installed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut manifest = serde_json::json!({
        "provider": variant.backend.provider(),
        "mode": "asr",
        "description": format!("{description} — Hugging Face"),
        "repo": inspection.repo,
        "files": entries,
        "huggingface": {
            "repo": inspection.repo,
            "revision": inspection.revision,
            "sha": inspection.sha,
            "variant": variant.id,
            "label": variant.label,
            "title": variant.title,
            "streaming": variant.streaming,
            "license": inspection.license,
            "languages": inspection.languages.codes,
            "installed_at": installed_at,
        },
    });
    add_layout_section(&mut manifest, &variant.layout);
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    write_atomically(&model_dir.join(MANIFEST_FILE), text.as_bytes())?;
    Ok(rel)
}

/// Records how the engine finds the files: `sherpa`, `whisper_cpp` or `mlx`.
pub fn add_layout_section(manifest: &mut serde_json::Value, layout: &ModelLayout) {
    let (key, value) = match layout {
        ModelLayout::Sherpa(layout) => ("sherpa", serde_json::to_value(layout).unwrap_or_default()),
        ModelLayout::WhisperCpp { model } => ("whisper_cpp", serde_json::json!({ "model": model })),
        ModelLayout::Mlx { kind } => ("mlx", serde_json::json!({ "kind": kind })),
    };
    manifest[key] = value;
}

/// Deletes a model installed from Hugging Face: its files, its manifest and
/// its folder. Returns the bytes freed. Refuses anything outside
/// `models_root/hf/` and folders without a Hugging Face manifest.
pub fn remove_user_model(models_root: &Path, model_path: &str) -> Result<u64, String> {
    let model_dir = if Path::new(model_path).is_absolute() {
        PathBuf::from(model_path)
    } else {
        models_root.join(model_path)
    };
    let hf_root = std::fs::canonicalize(models_root.join(USER_MODELS_DIR))
        .map_err(|_| "Такой модели нет.".to_string())?;
    let canonical =
        std::fs::canonicalize(&model_dir).map_err(|_| "Такой модели нет.".to_string())?;
    if !canonical.starts_with(&hf_root) || canonical == hf_root {
        return Err("Удалить можно только модель, добавленную с Hugging Face.".into());
    }
    let manifest_path = canonical.join(MANIFEST_FILE);
    let manifest: serde_json::Value = std::fs::symlink_metadata(&manifest_path)
        .ok()
        .filter(|m| m.is_file())
        .and_then(|_| std::fs::read_to_string(&manifest_path).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or("У модели нет описания — удалять её вслепую нельзя.")?;
    if manifest.get("huggingface").is_none() {
        return Err("Удалить можно только модель, добавленную с Hugging Face.".into());
    }
    let freed: u64 = manifest
        .get("files")
        .and_then(|f| f.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|f| f.get("name").and_then(|n| n.as_str()))
                .filter_map(|name| std::fs::symlink_metadata(canonical.join(name)).ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0);

    crate::model_manager::remove_model_files(&canonical).map_err(|e| e.to_string())?;
    std::fs::remove_file(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let _ = std::fs::remove_file(canonical.join(".DS_Store"));
    let _ = std::fs::remove_dir(&canonical);
    if let Some(owner_dir) = canonical.parent().filter(|p| *p != hf_root) {
        let _ = std::fs::remove_file(owner_dir.join(".DS_Store"));
        let _ = std::fs::remove_dir(owner_dir);
    }
    Ok(freed)
}

#[cfg(test)]
mod tests;
