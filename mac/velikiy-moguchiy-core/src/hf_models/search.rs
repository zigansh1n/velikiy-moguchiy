//! Live search for speech models on Hugging Face, with a first verdict from
//! the file names alone. Opening a result runs the full inspection.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::analysis::{self, RepoHints};
use super::backend::Backend;
use super::card;
use super::detect::RepoFile;
use super::repo::Sibling;

const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const CACHE_ENTRIES: usize = 40;
const EXPAND: &str = "expand[]=siblings&expand[]=downloads&expand[]=likes&expand[]=tags\
&expand[]=pipeline_tag&expand[]=library_name&expand[]=cardData&expand[]=gated";
/// Families that understand Russian although their repositories rarely
/// list languages.
const RUSSIAN_BY_NAME: &[&str] = &[
    "russian",
    "-ru-",
    "_ru_",
    "-ru",
    "gigaam",
    "giga-am",
    "vosk",
    "t-one",
    "parakeet-tdt-0.6b-v3",
    "omnilingual",
    "canary-1b-v2",
];

#[derive(Debug, Clone, Deserialize)]
struct Hit {
    id: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default)]
    pipeline_tag: Option<String>,
    #[serde(default)]
    library_name: Option<String>,
    #[serde(default)]
    gated: serde_json::Value,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default, rename = "cardData")]
    card_data: Option<serde_json::Value>,
    #[serde(default)]
    siblings: Vec<Sibling>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// This build can run it, as far as the file names tell.
    Fits,
    /// A model this app recognizes but this build cannot run (MLX).
    NeedsEngine,
    No,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchItem {
    pub repo: String,
    pub downloads: u64,
    pub likes: u64,
    pub verdict: Verdict,
    pub backend: Option<Backend>,
    /// «подходит · whisper.cpp», «MLX · нужен движок», «не подходит: нет ONNX».
    pub badge: String,
    pub reason: Option<String>,
    pub license: Option<String>,
    pub license_kind: card::LicenseKind,
    pub languages: String,
    pub has_russian: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResults {
    pub query: String,
    pub russian_only: bool,
    pub items: Vec<SearchItem>,
    /// Results left out because their languages do not include Russian.
    pub hidden: usize,
}

/// The two API queries: speech models by name, and sherpa-onnx exports,
/// which seldom carry the speech tag.
pub(crate) fn search_paths(query: &str) -> [String; 2] {
    let q = query.trim();
    let with_sherpa = if q.is_empty() {
        "sherpa".to_string()
    } else {
        format!("{q} sherpa")
    };
    [
        format!(
            "/api/models?search={}&pipeline_tag=automatic-speech-recognition&sort=downloads&limit=40&{EXPAND}",
            urlencoding::encode(q)
        ),
        format!(
            "/api/models?search={}&sort=downloads&limit=30&{EXPAND}",
            urlencoding::encode(&with_sherpa)
        ),
    ]
}

fn short_reason(code: &str) -> &'static str {
    match code {
        "pytorch" => "нет ONNX",
        "no_onnx" => "нет ONNX",
        "no_tokens" => "ONNX не для sherpa-onnx",
        "faster_whisper" => "формат faster-whisper",
        "gguf" => "формат GGUF",
        "ggml" => "нет ONNX",
        "empty" => "нет файлов модели",
        "not_asr" => "не распознавание речи",
        "ort" => "формат .ort",
        "unsupported_family" => "тип пока не поддерживается",
        _ => "не удалось понять формат",
    }
}

fn has_russian(hit: &Hit, languages: &card::Languages) -> Option<bool> {
    if languages.has_russian.is_some() {
        return languages.has_russian;
    }
    let name = hit.id.to_ascii_lowercase();
    if RUSSIAN_BY_NAME.iter().any(|w| name.contains(w)) {
        return Some(true);
    }
    let whisper = name.contains("whisper") && !name.contains(".en") && !name.ends_with("-en");
    whisper.then_some(true)
}

fn item_for(hit: &Hit, compiled: &[&str]) -> SearchItem {
    let files: Vec<RepoFile> = hit
        .siblings
        .iter()
        .map(|s| RepoFile {
            path: s.rfilename.clone(),
            size: s.size.unwrap_or(0),
            sha256: None,
            git_sha1: None,
        })
        .collect();
    let name = hit.id.split('/').nth(1).unwrap_or(&hit.id).to_string();
    let hints = RepoHints {
        name: name.clone(),
        library: hit.library_name.clone(),
        tags: hit.tags.clone(),
        pipeline_tag: hit.pipeline_tag.clone(),
    };
    let quick = analysis::quick_look(&files, &hints, compiled);
    let languages = card::languages(hit.card_data.as_ref(), &[], &name);
    let russian = has_russian(hit, &languages);
    let display = match (languages.has_russian, russian) {
        (None, Some(true)) if name.to_ascii_lowercase().contains("whisper") => {
            card::Languages::whisper().display
        }
        (None, Some(true)) => "русский (по названию)".to_string(),
        _ => languages.display,
    };
    let gated = matches!(&hit.gated, serde_json::Value::Bool(true))
        || matches!(&hit.gated, serde_json::Value::String(s) if !s.is_empty() && s != "false");
    let license = card::license_from_card(hit.card_data.as_ref(), &hit.tags);
    let (verdict, backend, badge, reason) = if gated {
        (
            Verdict::No,
            None,
            "не подходит: доступ по запросу".to_string(),
            Some("Доступ к модели выдают по запросу на Hugging Face.".to_string()),
        )
    } else {
        match quick {
            analysis::QuickLook::Runs(backend) => (
                Verdict::Fits,
                Some(backend),
                format!("подходит · {}", backend.display_name()),
                None,
            ),
            analysis::QuickLook::NeedsEngine(backend, note) => (
                Verdict::NeedsEngine,
                Some(backend),
                format!("{} · нужен движок", backend.display_name()),
                Some(note),
            ),
            analysis::QuickLook::No(problem) => (
                Verdict::No,
                None,
                format!("не подходит: {}", short_reason(&problem.code)),
                Some(problem.message),
            ),
        }
    };
    SearchItem {
        repo: hit.id.clone(),
        downloads: hit.downloads,
        likes: hit.likes,
        verdict,
        backend,
        badge,
        reason,
        license: license.as_ref().map(|l| l.name.clone()),
        license_kind: license
            .map(|l| l.kind)
            .unwrap_or(card::LicenseKind::Unknown),
        languages: display,
        has_russian: russian,
    }
}

/// Ranks and filters the answers of [`search_paths`]: what runs first, then
/// by downloads; with `russian_only`, models whose languages leave out
/// Russian or are unknown are counted in `hidden`.
pub(crate) fn rank(
    answers: &[Vec<serde_json::Value>],
    query: &str,
    russian_only: bool,
    compiled: &[&str],
) -> SearchResults {
    let mut seen = HashMap::new();
    let mut items = Vec::new();
    for answer in answers {
        for value in answer {
            let Ok(hit) = serde_json::from_value::<Hit>(value.clone()) else {
                continue;
            };
            if seen.insert(hit.id.clone(), ()).is_some() {
                continue;
            }
            items.push(item_for(&hit, compiled));
        }
    }
    let before = items.len();
    if russian_only {
        items.retain(|item| item.has_russian == Some(true));
    }
    let hidden = before - items.len();
    items.sort_by(|a, b| {
        a.verdict
            .cmp(&b.verdict)
            .then(b.downloads.cmp(&a.downloads))
    });
    SearchResults {
        query: query.trim().to_string(),
        russian_only,
        items,
        hidden,
    }
}

type Cache = HashMap<(String, bool), (Instant, SearchResults)>;
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

pub async fn search(
    client: &reqwest::Client,
    base: &str,
    query: &str,
    russian_only: bool,
    compiled: &[&str],
) -> Result<SearchResults, String> {
    let key = (query.trim().to_lowercase(), russian_only);
    if let Some((at, results)) = CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .get(&key)
    {
        if at.elapsed() < CACHE_TTL {
            return Ok(results.clone());
        }
    }

    let mut answers = Vec::new();
    let mut failure = None;
    for path in search_paths(query) {
        match client.get(format!("{base}{path}")).send().await {
            Ok(response) if response.status().is_success() => {
                match super::read_limited(response, 8 * 1024 * 1024).await {
                    Ok(Some(body)) => answers.push(serde_json::from_slice::<Vec<serde_json::Value>>(&body).unwrap_or_default()),
                    Ok(None) => answers.push(Vec::new()),
                    Err(e) => failure = Some(e),
                }
            }
            Ok(response) if response.status().as_u16() == 429 => {
                failure = Some("Hugging Face просит подождать: слишком много запросов. Попробуйте через минуту.".into())
            }
            Ok(response) => failure = Some(format!("Hugging Face ответил ошибкой {}.", response.status().as_u16())),
            Err(_) => failure = Some(
                "Нет связи с Hugging Face: поиск сейчас недоступен. «Рекомендуемые» и «На этом Mac» работают и без сети."
                    .into(),
            ),
        }
    }
    if answers.is_empty() {
        return Err(failure.unwrap_or_else(|| "Поиск не ответил.".into()));
    }
    let results = rank(&answers, query, russian_only, compiled);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::new);
    if cache.len() >= CACHE_ENTRIES {
        cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        if cache.len() >= CACHE_ENTRIES {
            cache.clear();
        }
    }
    cache.insert(key, (Instant::now(), results.clone()));
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers(names: &[&str]) -> Vec<Vec<serde_json::Value>> {
        names
            .iter()
            .map(|n| {
                let text = match *n {
                    "gigaam_asr" => include_str!("../testdata/hf_search/gigaam_asr.json"),
                    "gigaam_sherpa" => include_str!("../testdata/hf_search/gigaam_sherpa.json"),
                    "turbo_asr" => include_str!("../testdata/hf_search/turbo_asr.json"),
                    "turbo_sherpa" => include_str!("../testdata/hf_search/turbo_sherpa.json"),
                    _ => unreachable!(),
                };
                serde_json::from_str(text).unwrap()
            })
            .collect()
    }

    const RELEASE: &[&str] = &["sherpa-onnx", "whisper-cpp", "apple-speech"];

    fn find<'a>(r: &'a SearchResults, repo: &str) -> &'a SearchItem {
        r.items
            .iter()
            .find(|i| i.repo == repo)
            .unwrap_or_else(|| panic!("{repo} missing"))
    }

    #[test]
    fn gigaam_search_puts_what_runs_first_and_names_the_rest() {
        let r = rank(
            &answers(&["gigaam_asr", "gigaam_sherpa"]),
            "gigaam",
            true,
            RELEASE,
        );
        assert_eq!(r.items[0].verdict, Verdict::Fits);
        let bahamut = find(&r, "BahamutRU/GigaAM-v3-sherpa-onnx");
        assert_eq!(
            (bahamut.badge.as_str(), bahamut.backend),
            ("подходит · sherpa-onnx", Some(Backend::SherpaOnnx))
        );
        let pytorch = find(&r, "ai-sage/GigaAM-v3");
        assert_eq!(pytorch.badge, "не подходит: нет ONNX");
        assert!(pytorch.reason.as_deref().unwrap().contains("PyTorch"));
        let onnx_asr = find(&r, "istupakov/gigaam-v3-onnx");
        assert_eq!(onnx_asr.badge, "не подходит: ONNX не для sherpa-onnx");
        let gguf = find(&r, "handy-computer/gigaam-v3-e2e-rnnt-gguf");
        assert_eq!(gguf.badge, "не подходит: формат GGUF");
        let mlx = find(&r, "aystream/GigaAM-v3-e2e-rnnt-mlx");
        assert_eq!(
            (mlx.verdict, mlx.badge.as_str()),
            (Verdict::NeedsEngine, "MLX · нужен движок")
        );
        let fits = r
            .items
            .iter()
            .take_while(|i| i.verdict == Verdict::Fits)
            .count();
        assert!(r.items[..fits]
            .windows(2)
            .all(|w| w[0].downloads >= w[1].downloads));
        assert_eq!(
            r.items
                .iter()
                .filter(|i| i.repo == "BahamutRU/GigaAM-v3-sherpa-onnx")
                .count(),
            1
        );
    }

    #[test]
    fn the_russian_filter_keeps_multilingual_whisper_and_drops_the_rest() {
        let all = rank(
            &answers(&["turbo_asr", "turbo_sherpa"]),
            "turbo",
            false,
            RELEASE,
        );
        let ru = rank(
            &answers(&["turbo_asr", "turbo_sherpa"]),
            "turbo",
            true,
            RELEASE,
        );
        assert_eq!(all.items.len(), ru.items.len() + ru.hidden);
        assert!(ru.hidden > 0);
        assert!(ru.items.iter().all(|i| i.has_russian == Some(true)));
        assert!(ru
            .items
            .iter()
            .any(|i| i.repo == "csukuangfj/sherpa-onnx-whisper-turbo"));
        assert!(!ru
            .items
            .iter()
            .any(|i| i.repo == "primeline/whisper-large-v3-turbo-german"));
        let ct2 = find(&all, "deepdml/faster-whisper-large-v3-turbo-ct2");
        assert_eq!(ct2.badge, "не подходит: формат faster-whisper");
        let sherpa = find(&ru, "csukuangfj/sherpa-onnx-whisper-turbo");
        assert_eq!(sherpa.languages, "99 языков, включая русский");
    }

    #[test]
    fn search_paths_ask_for_speech_models_and_sherpa_exports() {
        let [speech, sherpa] = search_paths(" GigaAM v3 ");
        assert!(speech.starts_with(
            "/api/models?search=GigaAM%20v3&pipeline_tag=automatic-speech-recognition"
        ));
        assert!(sherpa.starts_with("/api/models?search=GigaAM%20v3%20sherpa&"));
        assert!(speech.contains("expand[]=siblings"));
        assert!(search_paths("")[1].contains("search=sherpa&"));
    }

    #[test]
    fn offline_search_says_what_still_works() {
        let client = super::super::http_client().unwrap();
        let err = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(search(
                &client,
                "http://127.0.0.1:9",
                "offline-test",
                true,
                RELEASE,
            ))
            .unwrap_err();
        assert!(err.contains("без сети"), "{err}");
    }
}
