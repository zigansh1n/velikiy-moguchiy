//! Repository fixtures are real Hugging Face answers (file lists, sizes, LFS
//! hashes, cards) with the ONNX metadata of their main files, served by a
//! local server; nothing here touches the network.

use std::collections::{BTreeMap, HashMap};
use std::fs;

use velikiy_moguchiy_asr::{SherpaFamily, SherpaRole};

use super::*;
use crate::test_http::{Reply, Server};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../testdata/hf/", $name, ".json"))
    };
}

fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

/// What an ONNX file's tail looks like: weights, then metadata.
fn onnx_body(meta: &BTreeMap<String, String>) -> Vec<u8> {
    let entries: Vec<(&str, &str)> = meta.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut body = noise(20_000, meta.len() as u64);
    body.extend(onnx_meta::encode_entries(&entries));
    body
}

fn api_path(repo: &str, revision: &str) -> String {
    format!("/api/models/{repo}/revision/{revision}?blobs=true")
}

fn resolve_path(repo: &str, sha: &str, path: &str) -> String {
    file_url("", repo, sha, path)
}

/// A hub serving one fixture: the API answer and the ONNX files of its metadata.
fn hub_for(fixture: &str) -> (Server, RepoRef) {
    let doc: serde_json::Value = serde_json::from_str(fixture).unwrap();
    let info = &doc["info"];
    let repo_id = info["id"].as_str().unwrap();
    let sha = info["sha"].as_str().unwrap();
    let mut routes = HashMap::new();
    routes.insert(
        api_path(repo_id, "main"),
        Reply::Body(info.to_string().into_bytes()),
    );
    if let Some(metadata) = doc["metadata"].as_object() {
        for (path, meta) in metadata {
            let meta: BTreeMap<String, String> = serde_json::from_value(meta.clone()).unwrap();
            let tail = onnx_body(&meta);
            let size = info["siblings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["rfilename"] == path.as_str())
                .and_then(|s| s["lfs"]["size"].as_u64().or_else(|| s["size"].as_u64()))
                .map(|s| s as usize)
                .unwrap_or(tail.len())
                .max(tail.len());
            routes.insert(
                resolve_path(repo_id, sha, path),
                Reply::Sparse { size, tail },
            );
        }
    }
    let (owner, name) = repo_id.split_once('/').unwrap();
    let repo = RepoRef {
        owner: owner.into(),
        name: name.into(),
        revision: "main".into(),
    };
    (Server::start(routes), repo)
}

fn run<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

fn inspect_fixture(fixture: &str) -> Inspection {
    let (hub, repo) = hub_for(fixture);
    run(inspect(&http_client().unwrap(), &hub.base, &repo)).unwrap()
}

struct Expect {
    family: SherpaFamily,
    streaming: bool,
    files: &'static [&'static str],
}

fn check(name: &str, fixture: &str, expect: Expect) -> Inspection {
    let inspection = inspect_fixture(fixture);
    assert!(
        inspection.problem.is_none(),
        "{name}: {:?}",
        inspection.problem
    );
    let variant = &inspection.variants[0];
    assert_eq!(
        (variant.family, variant.streaming),
        (expect.family, expect.streaming),
        "{name}"
    );
    let mut paths: Vec<&str> = variant.files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    let mut want = expect.files.to_vec();
    want.sort();
    assert_eq!(paths, want, "{name}");
    assert_eq!(
        variant.total_size,
        variant.files.iter().map(|f| f.size).sum::<u64>()
    );
    variant
        .layout
        .validate()
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    inspection
}

#[test]
fn streaming_transducer_skips_the_bpe_model() {
    let i = check(
        "vosk small streaming",
        fixture!("csukuangfj__sherpa-onnx-streaming-zipformer-small-ru-vosk-int8-2025-08-16"),
        Expect {
            family: SherpaFamily::Transducer,
            streaming: true,
            files: &[
                "decoder.onnx",
                "encoder.int8.onnx",
                "joiner.int8.onnx",
                "tokens.txt",
            ],
        },
    );
    assert_eq!(i.sha, "31fa603e4f31279c6e1f7600fed13dc4312663ab");
    assert_eq!(i.languages.codes, vec!["ru"]);
    assert_eq!(i.license.kind, card::LicenseKind::Unknown);
    assert!(i.warnings.iter().any(|w| w.contains("Лицензия не указана")));
    assert_eq!(i.variants[0].precision, "int8 и fp32");
}

#[test]
fn offline_transducer_in_subfolders_takes_only_the_onnx_export() {
    let i = check(
        "vosk-model-ru",
        fixture!("alphacep__vosk-model-ru"),
        Expect {
            family: SherpaFamily::Transducer,
            streaming: false,
            files: &[
                "am-onnx/decoder.onnx",
                "am-onnx/encoder.int8.onnx",
                "am-onnx/joiner.int8.onnx",
                "lang/tokens.txt",
            ],
        },
    );
    assert_eq!(i.license.id.as_deref(), Some("apache-2.0"));
    assert_eq!(i.languages.has_russian, Some(true));
    assert!(i.warnings.is_empty(), "{:?}", i.warnings);

    check(
        "vosk-model-small-ru",
        fixture!("alphacep__vosk-model-small-ru"),
        Expect {
            family: SherpaFamily::Transducer,
            streaming: false,
            files: &[
                "am/decoder.onnx",
                "am/encoder.int8.onnx",
                "am/joiner.int8.onnx",
                "lang/tokens.txt",
            ],
        },
    );
}

#[test]
fn nemo_transducers_offline_and_streaming() {
    let i = check(
        "gigaam v3 transducer",
        fixture!("csukuangfj__sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16"),
        Expect {
            family: SherpaFamily::NemoTransducer,
            streaming: false,
            files: &[
                "LICENSE",
                "decoder.onnx",
                "encoder.int8.onnx",
                "joiner.onnx",
                "tokens.txt",
            ],
        },
    );
    let v = &i.variants[0];
    assert_eq!(v.title, "GigaAM (трансдьюсер)");
    assert_eq!(v.layout.model_type.as_deref(), Some("nemo_transducer"));
    assert_eq!(i.languages.codes, vec!["ru"]);
    assert_eq!(
        i.license.link.as_deref(),
        Some("https://github.com/salute-developers/GigaAM/blob/main/LICENSE")
    );

    let i = check(
        "parakeet int8",
        fixture!("csukuangfj__sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"),
        Expect {
            family: SherpaFamily::NemoTransducer,
            streaming: false,
            files: &[
                "decoder.int8.onnx",
                "encoder.int8.onnx",
                "joiner.int8.onnx",
                "tokens.txt",
            ],
        },
    );
    assert_eq!(i.variants[0].title, "Parakeet (NeMo-трансдьюсер)");
    assert_eq!(i.variants[0].layout.feature_dim, Some(128));

    check(
        "parakeet fp32 with external weights",
        fixture!("csukuangfj__sherpa-onnx-nemo-parakeet-tdt-0.6b-v3"),
        Expect {
            family: SherpaFamily::NemoTransducer,
            streaming: false,
            files: &[
                "decoder.onnx",
                "encoder.onnx",
                "encoder.weights",
                "joiner.onnx",
                "tokens.txt",
            ],
        },
    );

    let i = check(
        "nemo streaming transducer",
        fixture!("csukuangfj__sherpa-onnx-nemo-streaming-fast-conformer-transducer-en-80ms-int8"),
        Expect {
            family: SherpaFamily::NemoTransducer,
            streaming: true,
            files: &[
                "decoder.int8.onnx",
                "encoder.int8.onnx",
                "joiner.int8.onnx",
                "tokens.txt",
            ],
        },
    );
    assert!(i.variants[0].layout.model_type.is_none());
}

#[test]
fn ctc_models_of_every_kind() {
    let cases: &[(&str, &str, SherpaFamily, bool)] = &[
        (
            "gigaam ctc",
            fixture!("csukuangfj__sherpa-onnx-nemo-ctc-punct-giga-am-v3-russian-2025-12-16"),
            SherpaFamily::NemoCtc,
            false,
        ),
        (
            "nemo streaming ctc",
            fixture!("csukuangfj__sherpa-onnx-nemo-streaming-fast-conformer-ctc-en-80ms-int8"),
            SherpaFamily::NemoCtc,
            true,
        ),
        (
            "zipformer ctc",
            fixture!("csukuangfj__sherpa-onnx-zipformer-ctc-small-zh-int8-2025-07-16"),
            SherpaFamily::ZipformerCtc,
            false,
        ),
        (
            "zipformer2 streaming ctc",
            fixture!("csukuangfj__sherpa-onnx-streaming-zipformer-small-ctc-zh-int8-2025-04-01"),
            SherpaFamily::ZipformerCtc,
            true,
        ),
        (
            "t-one",
            fixture!("csukuangfj__sherpa-onnx-streaming-t-one-russian-2025-09-08"),
            SherpaFamily::ToneCtc,
            true,
        ),
        (
            "omnilingual",
            fixture!(
                "csukuangfj__sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-int8-2025-11-12"
            ),
            SherpaFamily::OmnilingualCtc,
            false,
        ),
        (
            "telespeech",
            fixture!("csukuangfj__sherpa-onnx-telespeech-ctc-int8-zh-2024-06-04"),
            SherpaFamily::TelespeechCtc,
            false,
        ),
        (
            "medasr",
            fixture!("csukuangfj__sherpa-onnx-medasr-ctc-en-int8-2025-12-25"),
            SherpaFamily::MedasrCtc,
            false,
        ),
        (
            "dolphin",
            fixture!("csukuangfj__sherpa-onnx-dolphin-base-ctc-multi-lang-int8-2025-04-02"),
            SherpaFamily::Dolphin,
            false,
        ),
        (
            "gigaam v2 community",
            fixture!("Alexanrd__GigaAMv2_CTC_RU_ASR_for_sherpa_onnx"),
            SherpaFamily::NemoCtc,
            false,
        ),
    ];
    for (name, fixture, family, streaming) in cases {
        let i = inspect_fixture(fixture);
        assert!(i.problem.is_none(), "{name}: {:?}", i.problem);
        let v = &i.variants[0];
        assert_eq!((v.family, v.streaming), (*family, *streaming), "{name}");
        assert!(v.layout.files.contains_key(&SherpaRole::Model), "{name}");
        assert!(
            v.files.iter().all(|f| !f.path.ends_with(".model")),
            "{name}: a bpe model is not needed"
        );
    }

    let t_one = inspect_fixture(fixture!(
        "csukuangfj__sherpa-onnx-streaming-t-one-russian-2025-09-08"
    ));
    assert_eq!(t_one.variants[0].layout.sample_rate, Some(8000));
    assert_eq!(t_one.languages.codes, vec!["ru"]);
    let medasr = inspect_fixture(fixture!(
        "csukuangfj__sherpa-onnx-medasr-ctc-en-int8-2025-12-25"
    ));
    assert_eq!(medasr.license.kind, card::LicenseKind::Restricted);
    assert!(medasr
        .warnings
        .iter()
        .any(|w| w.contains("особая лицензия")));
    assert!(medasr.warnings.iter().any(|w| w.contains("Русского языка")));
    let v2 = inspect_fixture(fixture!("Alexanrd__GigaAMv2_CTC_RU_ASR_for_sherpa_onnx"));
    assert_eq!(v2.variants[0].layout.feature_dim, Some(64));
}

#[test]
fn wenet_offers_the_offline_model_and_refuses_the_streaming_one() {
    let i = check(
        "wenet",
        fixture!("csukuangfj__sherpa-onnx-en-wenet-librispeech"),
        Expect {
            family: SherpaFamily::WenetCtc,
            streaming: false,
            files: &["model.int8.onnx", "tokens.txt"],
        },
    );
    assert_eq!(i.variants.len(), 1);
    assert!(i
        .rejected
        .iter()
        .any(|r| r.reason == detect::RejectReason::UnsupportedMode));
}

#[test]
fn encoder_decoder_models() {
    let i = check(
        "whisper tiny",
        fixture!("csukuangfj__sherpa-onnx-whisper-tiny"),
        Expect {
            family: SherpaFamily::Whisper,
            streaming: false,
            files: &[
                "tiny-decoder.int8.onnx",
                "tiny-encoder.int8.onnx",
                "tiny-tokens.txt",
            ],
        },
    );
    assert_eq!(i.variants[0].title, "Whisper tiny");
    assert_eq!(i.languages.has_russian, Some(true));

    let i = check(
        "whisper turbo",
        fixture!("csukuangfj__sherpa-onnx-whisper-turbo"),
        Expect {
            family: SherpaFamily::Whisper,
            streaming: false,
            files: &[
                "turbo-decoder.int8.onnx",
                "turbo-encoder.int8.onnx",
                "turbo-tokens.txt",
            ],
        },
    );
    assert!(
        i.variants[0].total_size < 1_100_000_000,
        "int8 turbo, not the 2.6 GB fp32 weights"
    );

    check(
        "fire red asr",
        fixture!("csukuangfj__sherpa-onnx-fire-red-asr-large-zh_en-2025-02-16"),
        Expect {
            family: SherpaFamily::FireRedAsr,
            streaming: false,
            files: &["decoder.int8.onnx", "encoder.int8.onnx", "tokens.txt"],
        },
    );
    let canary = check(
        "canary",
        fixture!("csukuangfj__sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8"),
        Expect {
            family: SherpaFamily::Canary,
            streaming: false,
            files: &["decoder.int8.onnx", "encoder.int8.onnx", "tokens.txt"],
        },
    );
    assert_eq!(canary.variants[0].layout.feature_dim, Some(128));
    check(
        "streaming paraformer",
        fixture!("csukuangfj__sherpa-onnx-streaming-paraformer-bilingual-zh-en"),
        Expect {
            family: SherpaFamily::Paraformer,
            streaming: true,
            files: &["decoder.int8.onnx", "encoder.int8.onnx", "tokens.txt"],
        },
    );
}

#[test]
fn single_file_and_moonshine_models() {
    check(
        "paraformer",
        fixture!("csukuangfj__sherpa-onnx-paraformer-zh-int8-2025-10-07"),
        Expect {
            family: SherpaFamily::Paraformer,
            streaming: false,
            files: &["model.int8.onnx", "tokens.txt"],
        },
    );
    let i = check(
        "sense voice",
        fixture!("csukuangfj__sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"),
        Expect {
            family: SherpaFamily::SenseVoice,
            streaming: false,
            files: &["model.int8.onnx", "tokens.txt"],
        },
    );
    assert!(i.languages.codes.contains(&"yue".to_string()));
    check(
        "moonshine",
        fixture!("csukuangfj__sherpa-onnx-moonshine-tiny-en-int8"),
        Expect {
            family: SherpaFamily::Moonshine,
            streaming: false,
            files: &[
                "LICENSE",
                "cached_decode.int8.onnx",
                "encode.int8.onnx",
                "preprocess.onnx",
                "tokens.txt",
                "uncached_decode.int8.onnx",
            ],
        },
    );
}

#[test]
fn older_streaming_transducers_and_language_models_beside_them() {
    let i = check(
        "bilingual zh-en with RNN LMs",
        fixture!("csukuangfj__sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20"),
        Expect {
            family: SherpaFamily::Transducer,
            streaming: true,
            files: &[
                "decoder-epoch-99-avg-1.onnx",
                "encoder-epoch-99-avg-1.int8.onnx",
                "joiner-epoch-99-avg-1.int8.onnx",
                "tokens.txt",
            ],
        },
    );
    assert_eq!(i.variants.len(), 1, "the RNN LMs are not speech models");
    let lstm = check(
        "lstm",
        fixture!("csukuangfj__sherpa-onnx-lstm-en-2023-02-17"),
        Expect {
            family: SherpaFamily::Transducer,
            streaming: true,
            files: &[
                "decoder-epoch-99-avg-1.onnx",
                "encoder-epoch-99-avg-1.int8.onnx",
                "joiner-epoch-99-avg-1.int8.onnx",
                "tokens.txt",
            ],
        },
    );
    assert_eq!(lstm.variants[0].title, "LSTM-трансдьюсер");
}

#[test]
fn a_repository_with_several_models_offers_each_with_punctuation_first() {
    let i = inspect_fixture(fixture!("BahamutRU__GigaAM-v3-sherpa-onnx"));
    let labels: Vec<&str> = i.variants.iter().map(|v| v.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "gigaam_v3_e2e_rnnt",
            "gigaam_v3_e2e_ctc",
            "gigaam_v3_rnnt",
            "gigaam_v3_ctc"
        ]
    );
    let first = &i.variants[0];
    assert_eq!(first.family, SherpaFamily::NemoTransducer);
    let files: Vec<&str> = first.files.iter().map(|f| f.path.as_str()).collect();
    assert!(files.contains(&"gigaam_v3_e2e_rnnt_encoder_int8.onnx"));
    assert!(files.contains(&"gigaam_v3_e2e_rnnt_joint.onnx"));
    assert!(files.contains(&"gigaam_v3_e2e_rnnt_tokens.txt"));
    assert!(!files.iter().any(|f| f.contains("_ctc")));
    assert_eq!(i.variants[3].family, SherpaFamily::NemoCtc);
    assert_eq!(
        model_rel_path(&i, first),
        "hf/BahamutRU/GigaAM-v3-sherpa-onnx@b2176bb2acab-gigaam_v3_e2e_rnnt"
    );
}

fn problem(fixture: &str) -> (Inspection, Problem) {
    let i = inspect_fixture(fixture);
    assert!(i.variants.is_empty(), "{:?}", i.variants);
    let p = i.problem.clone().expect("a reason");
    (i, p)
}

#[test]
fn unsupported_repositories_say_why_in_russian() {
    let (i, p) = problem(fixture!("nvidia__parakeet-tdt-0.6b-v3"));
    assert_eq!(p.code, "pytorch");
    assert_eq!(
        p.message,
        "В репозитории нет ONNX-файлов — эта модель только для PyTorch."
    );
    assert_eq!(i.search_phrase, "parakeet-tdt-0.6b-v3 sherpa-onnx");
    assert!(i
        .search_url
        .ends_with("/models?search=parakeet-tdt-0.6b-v3%20sherpa"));

    for fixture in [
        fixture!("openai__whisper-large-v3-turbo"),
        fixture!("ai-sage__GigaAM-v3"),
    ] {
        assert_eq!(problem(fixture).1.code, "pytorch");
    }

    let (i, p) = problem(fixture!("istupakov__gigaam-v3-onnx"));
    assert_eq!(p.code, "no_tokens");
    assert!(p.message.contains("onnx-asr"), "{}", p.message);
    assert_eq!(i.search_phrase, "gigaam-v3 sherpa-onnx");

    let (_, p) = problem(fixture!("onnx-community__whisper-large-v3-turbo"));
    assert!(p.message.contains("Transformers.js"), "{}", p.message);

    assert_eq!(
        problem(fixture!("csukuangfj__sherpa-onnx-nemo-fast-conformer-ctc-be-de-en-es-fr-hr-it-pl-ru-uk-20k-int8")).1.code,
        "empty"
    );
    assert_eq!(
        problem(fixture!(
            "csukuangfj__sherpa-onnx-funasr-nano-int8-2025-12-30"
        ))
        .1
        .code,
        "unsupported_family"
    );
    assert_eq!(
        problem(fixture!(
            "bookbot__sherpa-onnx-ort-streaming-zipformer-en-2023-06-26"
        ))
        .1
        .code,
        "ort"
    );
    assert!(problem(fixture!("csukuangfj__sherpa-onnx-tdnn-yesno"))
        .1
        .message
        .contains("yes/no"));
    assert!(problem(fixture!("csukuangfj__sherpa-onnx-vits-zh-ll"))
        .1
        .message
        .contains("синтеза речи"));
    let (_, gated) = problem(fixture!("meta-llama__Llama-2-7b-hf"));
    assert_eq!(gated.code, "gated");
}

#[test]
fn alternatives_are_suggested_for_an_unsupported_repository() {
    let doc: serde_json::Value =
        serde_json::from_str(fixture!("nvidia__parakeet-tdt-0.6b-v3")).unwrap();
    let mut routes = HashMap::new();
    routes.insert(
        api_path("nvidia/parakeet-tdt-0.6b-v3", "main"),
        Reply::Body(doc["info"].to_string().into_bytes()),
    );
    routes.insert(
        "/api/models?search=parakeet-tdt-0.6b-v3%20sherpa&limit=10".into(),
        Reply::Body(
            br#"[{"id":"csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"},{"id":"nvidia/parakeet-tdt-0.6b-v3"}]"#
                .to_vec(),
        ),
    );
    let hub = Server::start(routes);
    let repo = repo::parse_repo_input("nvidia/parakeet-tdt-0.6b-v3", "").unwrap();
    let i = run(inspect(&http_client().unwrap(), &hub.base, &repo)).unwrap();
    assert_eq!(
        i.alternatives,
        vec!["csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"]
    );
}

#[test]
fn api_errors_become_plain_messages() {
    let mut routes = HashMap::new();
    routes.insert(
        api_path("a/missing", "main"),
        Reply::Status(401, Vec::new()),
    );
    routes.insert(
        api_path("a/model", "nope"),
        Reply::Status(404, vec![("X-Error-Code", "RevisionNotFound".into())]),
    );
    routes.insert(api_path("a/busy", "main"), Reply::Status(429, Vec::new()));
    let hub = Server::start(routes);
    let client = http_client().unwrap();
    let ask = |input: &str, rev: &str| {
        let repo = repo::parse_repo_input(input, rev).unwrap();
        run(inspect(&client, &hub.base, &repo)).unwrap_err()
    };
    assert!(ask("a/missing", "").contains("не найден или закрыт"));
    assert!(ask("a/model", "nope").contains("«nope»"));
    assert!(ask("a/busy", "").contains("подождать"));
}

#[test]
fn metadata_comes_from_a_range_request_not_the_whole_file() {
    let (hub, repo) = hub_for(fixture!(
        "csukuangfj__sherpa-onnx-streaming-t-one-russian-2025-09-08"
    ));
    run(inspect(&http_client().unwrap(), &hub.base, &repo)).unwrap();
    let model = hub
        .requests()
        .into_iter()
        .find(|r| r.path.ends_with("/model.onnx"))
        .unwrap();
    assert_eq!(model.range.as_deref(), Some("bytes=144128166-144193701"));
}

#[test]
fn a_server_that_ignores_ranges_is_not_asked_for_gigabytes() {
    let doc: serde_json::Value = serde_json::from_str(fixture!(
        "csukuangfj__sherpa-onnx-streaming-t-one-russian-2025-09-08"
    ))
    .unwrap();
    let sha = doc["info"]["sha"].as_str().unwrap();
    let repo_id = "csukuangfj/sherpa-onnx-streaming-t-one-russian-2025-09-08";
    let mut routes = HashMap::new();
    routes.insert(
        api_path(repo_id, "main"),
        Reply::Body(doc["info"].to_string().into_bytes()),
    );
    routes.insert(
        resolve_path(repo_id, sha, "model.onnx"),
        Reply::IgnoreRange(noise(200_000, 1)),
    );
    let hub = Server::start(routes);
    let repo = repo::parse_repo_input(repo_id, "").unwrap();
    let i = run(inspect(&http_client().unwrap(), &hub.base, &repo)).unwrap();
    let problem = i.problem.unwrap();
    assert_eq!(problem.code, "unreadable");
    assert!(problem.message.contains("целиком"), "{}", problem.message);
}

#[test]
fn one_unreadable_variant_does_not_hide_the_others() {
    let doc: serde_json::Value =
        serde_json::from_str(fixture!("BahamutRU__GigaAM-v3-sherpa-onnx")).unwrap();
    let info = &doc["info"];
    let repo_id = "BahamutRU/GigaAM-v3-sherpa-onnx";
    let sha = info["sha"].as_str().unwrap();
    let mut routes = HashMap::new();
    routes.insert(
        api_path(repo_id, "main"),
        Reply::Body(info.to_string().into_bytes()),
    );
    for (path, meta) in doc["metadata"].as_object().unwrap() {
        if path.contains("_ctc") {
            continue;
        }
        let meta: BTreeMap<String, String> = serde_json::from_value(meta.clone()).unwrap();
        let size = info["siblings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["rfilename"] == path.as_str())
            .and_then(|s| s["lfs"]["size"].as_u64())
            .unwrap() as usize;
        routes.insert(
            resolve_path(repo_id, sha, path),
            Reply::Sparse {
                size,
                tail: onnx_body(&meta),
            },
        );
    }
    let hub = Server::start(routes);
    let repo = repo::parse_repo_input(repo_id, "").unwrap();
    let i = run(inspect(&http_client().unwrap(), &hub.base, &repo)).unwrap();
    let labels: Vec<&str> = i.variants.iter().map(|v| v.label.as_str()).collect();
    assert_eq!(labels, vec!["gigaam_v3_e2e_rnnt", "gigaam_v3_rnnt"]);
    assert_eq!(
        i.rejected
            .iter()
            .filter(|r| r.reason == detect::RejectReason::Unreadable)
            .count(),
        2
    );
}

#[test]
fn a_repository_name_in_cyrillic_is_refused_not_a_crash() {
    assert!(repo::parse_repo_input("https://huggingface.co/владелец/модель", "").is_err());
}

/// A tiny repository whose files the hub really serves, behind a redirect
/// like Hugging Face's CDN.
struct TinyRepo {
    hub: Server,
    repo: RepoRef,
    files: Vec<(&'static str, Vec<u8>)>,
}

const TINY_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn tiny_repo(tamper_tokens: bool) -> TinyRepo {
    let meta: BTreeMap<String, String> = [
        ("model_type", "EncDecRNNTBPEModel"),
        ("vocab_size", "3"),
        ("subsampling_factor", "4"),
        ("pred_rnn_layers", "1"),
        ("pred_hidden", "8"),
        ("is_giga_am", "1"),
        ("language", "Russian"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let files: Vec<(&'static str, Vec<u8>)> = vec![
        ("encoder.int8.onnx", onnx_body(&meta)),
        ("decoder.onnx", b"decoder weights".to_vec()),
        ("joiner.onnx", b"joiner weights".to_vec()),
        ("tokens.txt", "<blk> 0\n▁а 1\n▁б 2\n".as_bytes().to_vec()),
        (
            "LICENSE",
            b"MIT License\n\nPermission is hereby granted, free of charge, to any person".to_vec(),
        ),
        ("bpe.model", b"not needed".to_vec()),
    ];
    let repo_id = "tester/tiny-asr";
    let siblings: Vec<serde_json::Value> = files
        .iter()
        .map(|(path, data)| {
            if path.ends_with(".onnx") || path.ends_with(".model") {
                serde_json::json!({"rfilename": path, "size": data.len(),
                    "lfs": {"sha256": sha256_hex(data), "size": data.len()}})
            } else {
                let mut blob = git_blob_sha1(data);
                if tamper_tokens && *path == "tokens.txt" {
                    blob = git_blob_sha1(b"something else");
                }
                serde_json::json!({"rfilename": path, "size": data.len(), "blobId": blob})
            }
        })
        .collect();
    let info = serde_json::json!({"id": repo_id, "sha": TINY_SHA, "siblings": siblings,
        "cardData": {"license": "mit", "language": ["ru"]}});
    let mut routes = HashMap::new();
    routes.insert(
        api_path(repo_id, "main"),
        Reply::Body(info.to_string().into_bytes()),
    );
    for (path, data) in &files {
        let cdn = format!("/cdn/{path}");
        routes.insert(
            resolve_path(repo_id, TINY_SHA, path),
            Reply::Redirect(cdn.clone()),
        );
        routes.insert(cdn, Reply::Body(data.clone()));
    }
    TinyRepo {
        hub: Server::start(routes),
        repo: repo::parse_repo_input(repo_id, "").unwrap(),
        files,
    }
}

fn temp_models_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "velikiy-moguchiy-hf-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn install_download_and_remove_a_user_model() {
    let tiny = tiny_repo(false);
    let client = http_client().unwrap();
    let inspection = run(inspect(&client, &tiny.hub.base, &tiny.repo)).unwrap();
    assert_eq!(inspection.variants.len(), 1);
    assert_eq!(inspection.license.id.as_deref(), Some("mit"));
    assert!(inspection.warnings.is_empty(), "{:?}", inspection.warnings);
    let variant = &inspection.variants[0];
    assert!(!variant.files.iter().any(|f| f.path == "bpe.model"));

    let root = temp_models_root("install");
    let rel = run(prepare_install(
        &client,
        &tiny.hub.base,
        &root,
        &inspection,
        &variant.id,
    ))
    .unwrap();
    assert_eq!(rel, "hf/tester/tiny-asr@0123456789ab");
    let dir = root.join(&rel);
    assert_eq!(
        fs::read(dir.join("tokens.txt")).unwrap(),
        tiny.files[3].1,
        "git files are fetched at once"
    );
    assert!(dir.join("LICENSE").is_file());
    assert!(
        !dir.join("encoder.int8.onnx").exists(),
        "large files wait for the download"
    );

    // The manifest reads back as the layout, the source and the file list.
    assert_eq!(
        crate::model_manager::sherpa_layout(&dir).unwrap(),
        variant.layout
    );
    let source = crate::model_manager::hf_source(&dir).unwrap();
    assert_eq!(source["sha"], TINY_SHA);
    assert_eq!(source["revision"], "main");
    assert_eq!(source["license"]["id"], "mit");
    assert_eq!(source["languages"], serde_json::json!(["ru"]));
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap()).unwrap();
    assert_eq!(manifest["provider"], "sherpa-onnx");
    assert_eq!(manifest["description"], "tiny-asr — Hugging Face");
    for entry in manifest["files"].as_array().unwrap() {
        let url = entry["url"].as_str().unwrap();
        assert!(
            url.contains(&format!("/resolve/{TINY_SHA}/")),
            "pinned to the commit: {url}"
        );
        assert_eq!(entry["sha256"].as_str().unwrap().len(), 64);
    }

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::model_manager::download_model(
            &dir,
            |_| {},
            crate::model_manager::CancellationToken::new(),
        ))
        .unwrap();
    assert_eq!(
        crate::model_manager::model_status(&dir, crate::model_manager::VerifyMode::ForceVerify),
        crate::model_manager::ModelStatus::Installed
    );
    assert_eq!(
        fs::read(dir.join("encoder.int8.onnx")).unwrap(),
        tiny.files[0].1
    );

    // Installing again reuses what is there.
    let again = run(prepare_install(
        &client,
        &tiny.hub.base,
        &root,
        &inspection,
        &variant.id,
    ))
    .unwrap();
    assert_eq!(again, rel);

    let expected: u64 = variant.files.iter().map(|f| f.size).sum();
    assert_eq!(remove_user_model(&root, &rel).unwrap(), expected);
    assert!(!dir.exists());
    assert!(!root.join("hf/tester").exists());
    assert!(root.join("hf").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_git_file_that_does_not_match_its_blob_id_stops_the_install() {
    let tiny = tiny_repo(true);
    let client = http_client().unwrap();
    let inspection = run(inspect(&client, &tiny.hub.base, &tiny.repo)).unwrap();
    let root = temp_models_root("tampered");
    let err = run(prepare_install(
        &client,
        &tiny.hub.base,
        &root,
        &inspection,
        &inspection.variants[0].id,
    ))
    .unwrap_err();
    assert!(err.contains("tokens.txt"), "{err}");
    let dir = root.join("hf/tester/tiny-asr@0123456789ab");
    assert!(!dir.join(MANIFEST_FILE).exists(), "nothing is registered");
    assert!(!dir.join("tokens.txt").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn removal_is_refused_outside_user_models() {
    let root = temp_models_root("refuse");
    let builtin = root.join("sherpa-onnx/ru-vosk-small");
    fs::create_dir_all(&builtin).unwrap();
    fs::write(
        builtin.join(MANIFEST_FILE),
        include_str!("../manifests/sherpa-onnx/ru-vosk-small.json"),
    )
    .unwrap();
    fs::create_dir_all(root.join("hf/someone/model@abc")).unwrap();
    fs::write(
        root.join("hf/someone/model@abc").join(MANIFEST_FILE),
        r#"{"provider":"sherpa-onnx","files":[]}"#,
    )
    .unwrap();
    assert!(remove_user_model(&root, "sherpa-onnx/ru-vosk-small").is_err());
    assert!(remove_user_model(&root, "hf").is_err());
    assert!(remove_user_model(&root, "hf/../sherpa-onnx/ru-vosk-small").is_err());
    assert!(
        remove_user_model(&root, "hf/someone/model@abc").is_err(),
        "no huggingface section"
    );
    assert!(builtin.join(MANIFEST_FILE).exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn git_blob_ids_match_git() {
    // `printf 'hello\n' | git hash-object --stdin`
    assert_eq!(
        git_blob_sha1(b"hello\n"),
        "ce013625030ba8dba906f756967f9e9ca394464a"
    );
    assert_eq!(
        git_blob_sha1(b""),
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
    );
}
