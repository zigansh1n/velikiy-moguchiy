//! What the user typed, and what the Hugging Face API says about it.

use serde::Deserialize;

use super::detect::RepoFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
    /// Branch, tag or commit as asked for; "main" when nothing was given.
    pub revision: String,
}

impl RepoRef {
    pub fn id(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

const NAME_LIMIT: usize = 96;
const REVISION_LIMIT: usize = 200;

fn plain_name(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= NAME_LIMIT
        && !part.contains("..")
        && !part.starts_with(['.', '-'])
        && !part.ends_with('.')
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn plain_revision(revision: &str) -> bool {
    !revision.is_empty()
        && revision.len() <= REVISION_LIMIT
        && !revision
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        && revision
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}

/// Reads `owner/name`, `owner/name@revision` or a huggingface.co link (also
/// `/tree/<revision>` and file links). A non-empty `revision` wins over one
/// in the link. Errors are the text the window shows.
pub fn parse_repo_input(input: &str, revision: &str) -> Result<RepoRef, String> {
    let example = "Укажите репозиторий в виде «владелец/название», например csukuangfj/sherpa-onnx-whisper-tiny, или ссылку на него.";
    let mut text = input
        .trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '<' | '>' | '«' | '»'))
        .trim();
    if text.is_empty() {
        return Err(example.into());
    }
    if let Some(i) = text.find(['?', '#']) {
        text = &text[..i];
    }
    let had_scheme = match text.split_once("://") {
        Some((_, rest)) => {
            text = rest;
            true
        }
        None => false,
    };
    let mut via_link = false;
    for host in ["www.huggingface.co/", "huggingface.co/", "hf.co/"] {
        if text
            .get(..host.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(host))
        {
            text = &text[host.len()..];
            via_link = true;
            break;
        }
    }
    if had_scheme && !via_link {
        return Err("Это не ссылка на Hugging Face.".into());
    }
    let segments: Vec<&str> = text.split('/').filter(|s| !s.is_empty()).collect();
    if via_link && segments.is_empty() {
        return Err(example.into());
    }
    if let Some(first) = segments.first() {
        match *first {
            "datasets" => return Err("Это ссылка на набор данных, а нужна модель.".into()),
            "spaces" => return Err("Это ссылка на Space, а нужна модель.".into()),
            "models" | "docs" | "blog" | "settings" | "organizations" => return Err(example.into()),
            _ => {}
        }
    }
    if segments.len() < 2 {
        return Err(example.into());
    }
    let owner = segments[0];
    let (name, at_revision) = match segments[1].split_once('@') {
        Some((name, rev)) => (name, Some(rev.to_string())),
        None => (segments[1], None),
    };
    let name = name.strip_suffix(".git").unwrap_or(name);
    let link_revision = match segments.get(2) {
        Some(&("tree" | "blob" | "resolve" | "commit")) => segments.get(3).map(|r| {
            urlencoding::decode(r)
                .map(|r| r.into_owned())
                .unwrap_or_else(|_| r.to_string())
        }),
        Some(_) if !via_link => return Err(example.into()),
        _ => None,
    };
    if !plain_name(owner) || owner.contains('.') || !plain_name(name) {
        return Err(example.into());
    }
    let revision = Some(revision.trim().to_string())
        .filter(|r| !r.is_empty())
        .or(at_revision)
        .or(link_revision)
        .unwrap_or_else(|| "main".into());
    if !plain_revision(&revision) {
        return Err(format!(
            "Не понял ревизию «{revision}»: это ветка, тег или коммит."
        ));
    }
    Ok(RepoRef {
        owner: owner.into(),
        name: name.into(),
        revision,
    })
}

/// `GET /api/models/{repo}/revision/{revision}?blobs=true`, the parts used.
#[derive(Debug, Clone, Deserialize)]
pub struct RepoInfo {
    pub id: String,
    #[serde(default)]
    pub sha: Option<String>,
    #[serde(default)]
    pub private: bool,
    /// `false`, `"auto"` or `"manual"`.
    #[serde(default)]
    pub gated: serde_json::Value,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub pipeline_tag: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, rename = "cardData")]
    pub card_data: Option<serde_json::Value>,
    #[serde(default)]
    pub siblings: Vec<Sibling>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Sibling {
    pub rfilename: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default, rename = "blobId")]
    pub blob_id: Option<String>,
    #[serde(default)]
    pub lfs: Option<Lfs>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Lfs {
    pub sha256: String,
    pub size: u64,
}

impl RepoInfo {
    pub fn is_gated(&self) -> bool {
        match &self.gated {
            serde_json::Value::Bool(gated) => *gated,
            serde_json::Value::String(mode) => !mode.is_empty() && mode != "false",
            _ => false,
        }
    }

    pub fn files(&self) -> Vec<RepoFile> {
        self.siblings
            .iter()
            .map(|s| match &s.lfs {
                Some(lfs) => RepoFile {
                    path: s.rfilename.clone(),
                    size: lfs.size,
                    sha256: Some(lfs.sha256.to_ascii_lowercase()),
                    git_sha1: None,
                },
                None => RepoFile {
                    path: s.rfilename.clone(),
                    size: s.size.unwrap_or(0),
                    sha256: None,
                    git_sha1: s.blob_id.clone().map(|b| b.to_ascii_lowercase()),
                },
            })
            .collect()
    }
}

/// A repository name without the "sherpa-onnx-" and "-onnx" decorations,
/// for searching and for the model list.
pub fn short_name(name: &str) -> &str {
    let mut name = name;
    for prefix in ["sherpa-onnx-", "sherpa_onnx_"] {
        if name.len() > prefix.len()
            && name
                .get(..prefix.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
        {
            name = &name[prefix.len()..];
        }
    }
    for suffix in ["-sherpa-onnx", "_sherpa_onnx", "-onnx", "_onnx"] {
        if name.len() > suffix.len()
            && name
                .get(name.len() - suffix.len()..)
                .is_some_and(|p| p.eq_ignore_ascii_case(suffix))
        {
            name = &name[..name.len() - suffix.len()];
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> (String, String) {
        let r = parse_repo_input(input, "").unwrap();
        (r.id(), r.revision)
    }

    #[test]
    fn accepts_ids_and_links() {
        let want = ("istupakov/gigaam-v3-onnx".to_string(), "main".to_string());
        assert_eq!(parsed("istupakov/gigaam-v3-onnx"), want);
        assert_eq!(
            parsed("  https://huggingface.co/istupakov/gigaam-v3-onnx/  "),
            want
        );
        assert_eq!(parsed("huggingface.co/istupakov/gigaam-v3-onnx"), want);
        assert_eq!(parsed("https://hf.co/istupakov/gigaam-v3-onnx.git"), want);
        assert_eq!(parsed("«istupakov/gigaam-v3-onnx»"), want);
        assert_eq!(
            parsed("https://huggingface.co/istupakov/gigaam-v3-onnx?library=onnx"),
            want
        );
        assert_eq!(
            parsed("https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3/tree/v1.2"),
            ("nvidia/parakeet-tdt-0.6b-v3".into(), "v1.2".into())
        );
        assert_eq!(
            parsed("https://huggingface.co/csukuangfj/sherpa-onnx-whisper-tiny/blob/65176e2deb/tiny-tokens.txt"),
            ("csukuangfj/sherpa-onnx-whisper-tiny".into(), "65176e2deb".into())
        );
        assert_eq!(parsed("a/b@dev").1, "dev");
        assert_eq!(
            parse_repo_input("https://huggingface.co/a/b/tree/x", " v2 ")
                .unwrap()
                .revision,
            "v2"
        );
    }

    #[test]
    fn refuses_what_is_not_a_model_repository() {
        for bad in [
            "",
            "parakeet",
            "https://huggingface.co/",
            "https://huggingface.co/datasets/mozilla-foundation/common_voice_17_0",
            "https://huggingface.co/spaces/k2-fsa/automatic-speech-recognition",
            "https://github.com/k2-fsa/sherpa-onnx",
            "github.com/k2-fsa",
            "../etc/passwd",
            "a/b/c",
            "owner/..",
            "own er/name",
            "владелец/модель",
            "hf.co/владелец",
        ] {
            assert!(parse_repo_input(bad, "").is_err(), "{bad:?} accepted");
        }
        assert!(parse_repo_input("https://huggingface.co/datasets/x/y", "")
            .unwrap_err()
            .contains("набор данных"));
        assert!(parse_repo_input("https://github.com/a/b", "")
            .unwrap_err()
            .contains("не ссылка"));
        assert!(parse_repo_input("a/b", "../main").is_err());
        assert!(parse_repo_input("a/b", "main branch").is_err());
    }

    #[test]
    fn short_name_drops_the_decorations() {
        assert_eq!(short_name("sherpa-onnx-whisper-tiny"), "whisper-tiny");
        assert_eq!(short_name("gigaam-v3-onnx"), "gigaam-v3");
        assert_eq!(short_name("GigaAM-v3-sherpa-onnx"), "GigaAM-v3");
        assert_eq!(short_name("parakeet-tdt-0.6b-v3"), "parakeet-tdt-0.6b-v3");
        assert_eq!(short_name("модель"), "модель");
        assert_eq!(short_name("sherpa-onnx-модель-onnx"), "модель");
    }
}
