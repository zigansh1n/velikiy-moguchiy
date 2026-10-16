//! Licence and languages of a repository, from its model card, its tags,
//! its LICENSE file and the metadata inside its ONNX files.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseKind {
    Permissive,
    Copyleft,
    NonCommercial,
    /// Commercial use allowed with conditions (RAIL, Llama, Gemma, "other").
    Restricted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LicenseInfo {
    /// Hugging Face licence id, e.g. "apache-2.0"; `None` when unknown.
    pub id: Option<String>,
    /// What the window shows.
    pub name: String,
    pub kind: LicenseKind,
    pub link: Option<String>,
    /// Where it came from: "card", "tags", "license_file", "onnx" or "none".
    pub source: &'static str,
}

impl LicenseInfo {
    pub fn unknown() -> Self {
        Self {
            id: None,
            name: "не указана".into(),
            kind: LicenseKind::Unknown,
            link: None,
            source: "none",
        }
    }

    /// The warning the window shows before a download, if any.
    pub fn warning(&self) -> Option<String> {
        match self.kind {
            LicenseKind::Unknown => Some(
                "Лицензия не указана: прежде чем пользоваться моделью в работе, проверьте условия на её странице."
                    .into(),
            ),
            LicenseKind::NonCommercial => Some(format!(
                "Лицензия {} запрещает коммерческое использование.",
                self.name
            )),
            LicenseKind::Restricted => Some(format!(
                "У модели особая лицензия ({}): проверьте её условия.",
                self.name
            )),
            LicenseKind::Permissive | LicenseKind::Copyleft => None,
        }
    }
}

/// Classifies a Hugging Face licence id.
pub fn license_kind(id: &str) -> LicenseKind {
    let id = id.trim().to_ascii_lowercase();
    let nc = id.contains("-nc")
        || id.contains("noncommercial")
        || id.contains("non-commercial")
        || id.contains("research")
        || matches!(id.as_str(), "apple-amlr" | "deepfloyd-if-license");
    if nc {
        return LicenseKind::NonCommercial;
    }
    if id.is_empty() || id == "unknown" {
        return LicenseKind::Unknown;
    }
    const PERMISSIVE: &[&str] = &[
        "mit",
        "apache-2.0",
        "bsd",
        "bsd-2-clause",
        "bsd-3-clause",
        "bsd-3-clause-clear",
        "isc",
        "zlib",
        "unlicense",
        "cc0-1.0",
        "cc-by-2.0",
        "cc-by-2.5",
        "cc-by-3.0",
        "cc-by-4.0",
        "wtfpl",
        "bsl-1.0",
        "afl-3.0",
        "artistic-2.0",
        "ecl-2.0",
        "ms-pl",
        "ncsa",
        "postgresql",
        "pddl",
        "odc-by",
        "cdla-permissive-1.0",
        "cdla-permissive-2.0",
        "ofl-1.1",
        "etalab-2.0",
        "apple-ascl",
    ];
    const COPYLEFT: &[&str] = &[
        "gpl",
        "gpl-2.0",
        "gpl-3.0",
        "lgpl",
        "lgpl-2.1",
        "lgpl-3.0",
        "lgpl-lr",
        "agpl-3.0",
        "mpl-2.0",
        "epl-1.0",
        "epl-2.0",
        "eupl-1.1",
        "eupl-1.2",
        "osl-3.0",
        "odbl",
        "gfdl",
        "cc-by-sa-3.0",
        "cc-by-sa-4.0",
        "cdla-sharing-1.0",
        "lppl-1.3c",
    ];
    if PERMISSIVE.contains(&id.as_str()) {
        LicenseKind::Permissive
    } else if COPYLEFT.contains(&id.as_str()) {
        LicenseKind::Copyleft
    } else {
        LicenseKind::Restricted
    }
}

/// The licence the model card declares, or a `license:` tag.
pub fn license_from_card(card: Option<&serde_json::Value>, tags: &[String]) -> Option<LicenseInfo> {
    let card_id = card
        .and_then(|c| c.get("license"))
        .and_then(|l| match l {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(items) => {
                items.first().and_then(|v| v.as_str()).map(String::from)
            }
            _ => None,
        })
        .filter(|s| !s.trim().is_empty());
    let (id, source) = match card_id {
        Some(id) => (id, "card"),
        None => (
            tags.iter()
                .find_map(|t| t.strip_prefix("license:"))
                .filter(|s| !s.is_empty())?
                .to_string(),
            "tags",
        ),
    };
    let text = |key: &str| {
        card.and_then(|c| c.get(key))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(String::from)
    };
    let lower = id.to_ascii_lowercase();
    if lower == "unknown" {
        return None;
    }
    let mut kind = license_kind(&lower);
    let mut name = id.clone();
    if lower == "other" {
        kind = LicenseKind::Restricted;
        name = text("license_name").unwrap_or_else(|| "собственная".into());
        if license_kind(&name) == LicenseKind::NonCommercial {
            kind = LicenseKind::NonCommercial;
        }
    }
    Some(LicenseInfo {
        id: Some(lower),
        name,
        kind,
        link: text("license_link"),
        source,
    })
}

/// Recognizes the common licence texts in a LICENSE file.
pub fn license_from_text(text: &str) -> Option<LicenseInfo> {
    let head: String = text.chars().take(4000).collect::<String>().to_lowercase();
    let squashed = head.split_whitespace().collect::<Vec<_>>().join(" ");
    if squashed.starts_with("<!doctype") || squashed.starts_with("<html") {
        return None;
    }
    let (id, name, kind) = if squashed.contains("noncommercial")
        || squashed.contains("non-commercial")
        || squashed.contains("non commercial")
    {
        (None, "некоммерческая", LicenseKind::NonCommercial)
    } else if squashed.contains("mit license")
        || squashed.contains("permission is hereby granted, free of charge")
    {
        (Some("mit"), "MIT", LicenseKind::Permissive)
    } else if squashed.contains("apache license") && squashed.contains("2.0") {
        (Some("apache-2.0"), "Apache-2.0", LicenseKind::Permissive)
    } else if squashed.contains("redistribution and use in source and binary forms") {
        (Some("bsd"), "BSD", LicenseKind::Permissive)
    } else if squashed.contains("gnu affero general public license") {
        (Some("agpl-3.0"), "AGPL", LicenseKind::Copyleft)
    } else if squashed.contains("gnu lesser general public license") {
        (Some("lgpl"), "LGPL", LicenseKind::Copyleft)
    } else if squashed.contains("gnu general public license") {
        (Some("gpl"), "GPL", LicenseKind::Copyleft)
    } else if squashed.contains("attribution-sharealike 4.0") {
        (Some("cc-by-sa-4.0"), "CC-BY-SA-4.0", LicenseKind::Copyleft)
    } else if squashed.contains("creative commons attribution 4.0") {
        (Some("cc-by-4.0"), "CC-BY-4.0", LicenseKind::Permissive)
    } else {
        return None;
    };
    Some(LicenseInfo {
        id: id.map(String::from),
        name: name.into(),
        kind,
        link: None,
        source: "license_file",
    })
}

/// A licence named by a file such as «GigaAM License_NC.pdf», when the card
/// says nothing.
pub fn license_from_file_names<'a>(paths: impl Iterator<Item = &'a str>) -> Option<LicenseInfo> {
    for path in paths {
        let name = path.rsplit('/').next().unwrap_or(path);
        let lower = urlencoding::decode(name)
            .map(|n| n.to_lowercase())
            .unwrap_or_else(|_| name.to_lowercase());
        let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).collect();
        if (lower.contains("license") || lower.contains("licence"))
            && (words.contains(&"nc")
                || lower.contains("noncommercial")
                || lower.contains("non-commercial"))
        {
            let shown = urlencoding::decode(name)
                .map(|n| n.into_owned())
                .unwrap_or_else(|_| name.into());
            return Some(LicenseInfo {
                id: None,
                name: format!("некоммерческая ({shown})"),
                kind: LicenseKind::NonCommercial,
                link: None,
                source: "license_file",
            });
        }
    }
    None
}

/// A link to the original licence some sherpa-onnx exports record in their
/// metadata (GigaAM, MedASR). Says where the terms are, not what they are.
pub fn license_from_onnx(metadata: &BTreeMap<String, String>) -> Option<LicenseInfo> {
    let link = metadata
        .get("license")
        .filter(|l| l.starts_with("https://") || l.starts_with("http://"))?;
    Some(LicenseInfo {
        id: None,
        name: "указана по ссылке".into(),
        kind: LicenseKind::Unknown,
        link: Some(link.clone()),
        source: "onnx",
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Languages {
    /// ISO 639-1 codes (`yue` for Cantonese), in the order found.
    pub codes: Vec<String>,
    /// "card", "onnx", "name" or "none".
    pub source: &'static str,
    /// What the window shows, e.g. «русский, английский».
    pub display: String,
    /// `None` when the languages are unknown.
    pub has_russian: Option<bool>,
}

impl Languages {
    fn from_codes(codes: Vec<String>, source: &'static str) -> Self {
        let has_russian = Some(codes.iter().any(|c| c == "ru"));
        let display = if codes.len() > 6 {
            let russian = if has_russian == Some(true) {
                ", включая русский"
            } else {
                ", без русского"
            };
            format!("{} языков{russian}", codes.len())
        } else {
            codes
                .iter()
                .map(|c| {
                    language_name(c)
                        .map(String::from)
                        .unwrap_or_else(|| c.clone())
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        Self {
            codes,
            source,
            display,
            has_russian,
        }
    }

    pub fn unknown() -> Self {
        Self {
            codes: Vec::new(),
            source: "none",
            display: "не указаны".into(),
            has_russian: None,
        }
    }

    /// Multilingual Whisper, whose repositories rarely list its languages.
    pub fn whisper() -> Self {
        Self {
            codes: vec!["ru".into(), "en".into()],
            source: "family",
            display: "99 языков, включая русский".into(),
            has_russian: Some(true),
        }
    }
}

const LANGUAGES: &[(&str, &str, &str)] = &[
    ("ru", "русский", "russian"),
    ("en", "английский", "english"),
    ("uk", "украинский", "ukrainian"),
    ("be", "белорусский", "belarusian"),
    ("kk", "казахский", "kazakh"),
    ("uz", "узбекский", "uzbek"),
    ("ky", "киргизский", "kyrgyz"),
    ("tt", "татарский", "tatar"),
    ("de", "немецкий", "german"),
    ("fr", "французский", "french"),
    ("es", "испанский", "spanish"),
    ("it", "итальянский", "italian"),
    ("pt", "португальский", "portuguese"),
    ("pl", "польский", "polish"),
    ("cs", "чешский", "czech"),
    ("bg", "болгарский", "bulgarian"),
    ("hr", "хорватский", "croatian"),
    ("nl", "нидерландский", "dutch"),
    ("tr", "турецкий", "turkish"),
    ("ar", "арабский", "arabic"),
    ("zh", "китайский", "chinese"),
    ("yue", "кантонский", "cantonese"),
    ("ja", "японский", "japanese"),
    ("ko", "корейский", "korean"),
    ("vi", "вьетнамский", "vietnamese"),
    ("th", "тайский", "thai"),
    ("id", "индонезийский", "indonesian"),
    ("hi", "хинди", "hindi"),
];

fn language_name(code: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, ru, _)| *ru)
}

fn code_for_english_name(name: &str) -> Option<&'static str> {
    let name = name.trim().to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|(_, _, en)| *en == name)
        .map(|(c, _, _)| *c)
}

fn push_unique(codes: &mut Vec<String>, code: &str) {
    let code = code.trim().to_ascii_lowercase();
    if !code.is_empty() && !codes.contains(&code) {
        codes.push(code);
    }
}

/// Languages from the model card, else from ONNX metadata, else from the
/// repository name.
pub fn languages(
    card: Option<&serde_json::Value>,
    metadata: &[&BTreeMap<String, String>],
    repo_name: &str,
) -> Languages {
    let mut codes = Vec::new();
    match card.and_then(|c| c.get("language")) {
        Some(serde_json::Value::String(code)) => push_unique(&mut codes, code),
        Some(serde_json::Value::Array(items)) => {
            for code in items.iter().filter_map(|v| v.as_str()) {
                push_unique(&mut codes, code);
            }
        }
        _ => {}
    }
    codes.retain(|c| c != "multilingual");
    if !codes.is_empty() {
        return Languages::from_codes(codes, "card");
    }

    for meta in metadata {
        if let Some(code) = meta.get("language").and_then(|l| code_for_english_name(l)) {
            push_unique(&mut codes, code);
        }
        if let Some(list) = meta.get("all_language_codes") {
            for code in list.split(',') {
                push_unique(&mut codes, code);
            }
        }
        for key in meta.keys() {
            if let Some(code) = key.strip_prefix("lang_") {
                if code != "auto" && code != "nospeech" {
                    push_unique(&mut codes, code);
                }
            }
        }
    }
    if !codes.is_empty() {
        return Languages::from_codes(codes, "onnx");
    }

    for word in repo_name.to_ascii_lowercase().split(['-', '_', '.']) {
        if let Some(code) = code_for_english_name(word) {
            push_unique(&mut codes, code);
        } else if language_name(word).is_some() {
            push_unique(&mut codes, word);
        }
    }
    if codes.is_empty() {
        Languages::unknown()
    } else {
        Languages::from_codes(codes, "name")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn licence_ids_are_classified() {
        assert_eq!(license_kind("mit"), LicenseKind::Permissive);
        assert_eq!(license_kind("Apache-2.0"), LicenseKind::Permissive);
        assert_eq!(license_kind("cc-by-4.0"), LicenseKind::Permissive);
        assert_eq!(license_kind("cc-by-sa-4.0"), LicenseKind::Copyleft);
        assert_eq!(license_kind("cc-by-nc-4.0"), LicenseKind::NonCommercial);
        assert_eq!(license_kind("cc-by-nc-sa-4.0"), LicenseKind::NonCommercial);
        assert_eq!(
            license_kind("fair-noncommercial-research-license"),
            LicenseKind::NonCommercial
        );
        assert_eq!(license_kind("openrail"), LicenseKind::Restricted);
        assert_eq!(license_kind("llama2"), LicenseKind::Restricted);
        assert_eq!(license_kind(""), LicenseKind::Unknown);
    }

    #[test]
    fn card_licence_wins_over_tags_and_other_uses_its_name() {
        let card = json!({"license": "mit"});
        let info = license_from_card(Some(&card), &["license:apache-2.0".into()]).unwrap();
        assert_eq!((info.id.as_deref(), info.source), (Some("mit"), "card"));
        assert!(info.warning().is_none());

        let tags = license_from_card(None, &["license:cc-by-nc-4.0".into()]).unwrap();
        assert_eq!(tags.kind, LicenseKind::NonCommercial);
        assert!(tags.warning().unwrap().contains("коммерческое"));

        let other = json!({
            "license": "other",
            "license_name": "health-ai-developer-foundations",
            "license_link": "https://developers.google.com/health-ai-developer-foundations/terms"
        });
        let info = license_from_card(Some(&other), &[]).unwrap();
        assert_eq!(info.kind, LicenseKind::Restricted);
        assert_eq!(info.name, "health-ai-developer-foundations");
        assert!(info.link.unwrap().contains("terms"));

        assert!(license_from_card(Some(&json!({})), &[]).is_none());
        assert!(LicenseInfo::unknown()
            .warning()
            .unwrap()
            .contains("не указана"));
    }

    #[test]
    fn licence_files_are_recognized() {
        let mit = "MIT License\n\nCopyright (c) 2024 GigaChat Team\n\nPermission is hereby granted, free of charge";
        assert_eq!(license_from_text(mit).unwrap().id.as_deref(), Some("mit"));
        let apache = "Copyright 2025 T-Software DC\n\nLicensed under the Apache License, Version 2.0 (the \"License\")";
        assert_eq!(
            license_from_text(apache).unwrap().id.as_deref(),
            Some("apache-2.0")
        );
        let nc = "Attribution-NonCommercial 4.0 International\n\nCreative Commons Corporation";
        assert_eq!(
            license_from_text(nc).unwrap().kind,
            LicenseKind::NonCommercial
        );
        assert!(license_from_text("<!DOCTYPE html>\n<html lang=\"en\">").is_none());
        assert!(license_from_text("Ref to https://github.com/modelscope/FunASR#license").is_none());
    }

    #[test]
    fn a_non_commercial_licence_file_name_is_noticed() {
        let names = ["README.md", "GigaAM%20License_NC.pdf", "model.int8.onnx"];
        let info = license_from_file_names(names.iter().copied()).unwrap();
        assert_eq!(info.kind, LicenseKind::NonCommercial);
        assert!(info.name.contains("GigaAM License_NC.pdf"), "{}", info.name);
        assert!(license_from_file_names(["LICENSE", "encoder.onnx"].iter().copied()).is_none());
    }

    #[test]
    fn onnx_licence_link_is_kept_but_stays_unknown() {
        let meta: BTreeMap<String, String> = [(
            "license".to_string(),
            "https://github.com/salute-developers/GigaAM/blob/main/LICENSE".to_string(),
        )]
        .into();
        let info = license_from_onnx(&meta).unwrap();
        assert_eq!(info.kind, LicenseKind::Unknown);
        assert!(info.link.unwrap().ends_with("LICENSE"));
    }

    #[test]
    fn languages_come_from_card_then_metadata_then_name() {
        let card = json!({"language": ["ru", "en"]});
        let l = languages(Some(&card), &[], "x");
        assert_eq!(
            (l.display.as_str(), l.has_russian),
            ("русский, английский", Some(true))
        );

        let whisper: BTreeMap<String, String> = [(
            "all_language_codes".to_string(),
            "en,zh,de,es,ru,ko,fr,ja,pt,tr".to_string(),
        )]
        .into();
        let l = languages(None, &[&whisper], "sherpa-onnx-whisper-tiny");
        assert_eq!(
            (l.source, l.display.as_str()),
            ("onnx", "10 языков, включая русский")
        );

        let gigaam: BTreeMap<String, String> =
            [("language".to_string(), "Russian".to_string())].into();
        assert_eq!(languages(None, &[&gigaam], "x").codes, vec!["ru"]);

        let sense_voice: BTreeMap<String, String> = [
            "lang_auto",
            "lang_zh",
            "lang_en",
            "lang_nospeech",
            "with_itn",
        ]
        .iter()
        .map(|k| (k.to_string(), "1".to_string()))
        .collect();
        assert_eq!(
            languages(None, &[&sense_voice], "x").codes,
            vec!["en", "zh"]
        );

        let l = languages(
            None,
            &[],
            "sherpa-onnx-streaming-zipformer-small-ru-vosk-int8-2025-08-16",
        );
        assert_eq!(
            (l.source, l.codes.clone()),
            ("name", vec!["ru".to_string()])
        );
        let l = languages(None, &[], "sherpa-onnx-paraformer-zh-2024-03-09");
        assert_eq!(l.has_russian, Some(false));
        assert_eq!(
            languages(None, &[], "sherpa-onnx-whisper-turbo").has_russian,
            None
        );
    }
}
