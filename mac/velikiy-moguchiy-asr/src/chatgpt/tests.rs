use super::auth::tests::{id_token, jwt};
use super::*;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct MemoryStore(Mutex<Option<ChatGptTokens>>);

impl TokenStore for MemoryStore {
    fn load(&self) -> std::result::Result<Option<ChatGptTokens>, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&self, tokens: &ChatGptTokens) -> std::result::Result<(), String> {
        *self.0.lock().unwrap() = Some(tokens.clone());
        Ok(())
    }
    fn clear(&self) -> std::result::Result<(), String> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

type Routes = Arc<Mutex<HashMap<&'static str, VecDeque<(u16, String)>>>>;

/// A local server answering each path from a queue of `(status, body)`;
/// the last answer of a queue repeats. Records every request.
struct FakeServer {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl FakeServer {
    async fn start(routes: Vec<(&'static str, Vec<(u16, String)>)>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let routes: Routes = Arc::new(Mutex::new(
            routes
                .into_iter()
                .map(|(p, r)| (p, r.into_iter().collect()))
                .collect(),
        ));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let routes = routes.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let Some(request) = read_request(&mut stream).await else {
                        return;
                    };
                    let answer = {
                        let mut routes = routes.lock().unwrap();
                        match routes.get_mut(request.path.as_str()) {
                            Some(queue) if queue.len() > 1 => queue.pop_front().unwrap(),
                            Some(queue) => queue.front().cloned().unwrap(),
                            None => (404, String::new()),
                        }
                    };
                    log.lock().unwrap().push(request);
                    let head = format!(
                        "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        answer.0,
                        answer.1.len()
                    );
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(answer.1.as_bytes()).await;
                });
            }
        });
        Self { base, seen }
    }

    fn endpoints(&self) -> ChatGptEndpoints {
        ChatGptEndpoints {
            transcribe_url: format!("{}/backend-api/transcribe", self.base),
            auth: AuthEndpoints {
                issuer: self.base.clone(),
            },
        }
    }

    fn seen(&self, path: &str) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path == path)
            .cloned()
            .collect()
    }
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let path = first.next()?.split('?').next()?.to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some(Seen {
        method,
        path,
        headers,
        body,
    })
}

const TRANSCRIBE: &str = "/backend-api/transcribe";
const TOKEN: &str = "/oauth/token";

fn fresh_access(tag: &str) -> String {
    jwt(serde_json::json!({ "exp": auth::unix_now() + 3600, "tag": tag }))
}

fn signed_in_store(access: String) -> Arc<MemoryStore> {
    let store = Arc::new(MemoryStore::default());
    store
        .save(&ChatGptTokens::from_response(
            id_token("acc-1"),
            access,
            "refresh-1".into(),
        ))
        .unwrap();
    store
}

fn text(body: &str) -> (u16, String) {
    (200, serde_json::json!({ "text": body }).to_string())
}

fn token_answer(access: &str, refresh: &str) -> (u16, String) {
    (
        200,
        serde_json::json!({
            "id_token": id_token("acc-1"),
            "access_token": access,
            "refresh_token": refresh,
        })
        .to_string(),
    )
}

#[tokio::test]
async fn sends_one_multipart_wav_with_the_codex_headers() {
    let server = FakeServer::start(vec![(TRANSCRIBE, vec![text(" Привет, мир ")])]).await;
    let access = fresh_access("a");
    let store = signed_in_store(access.clone());
    let mut client = ChatGptClient::signed_in(store, server.endpoints()).unwrap();

    let pcm = vec![1u8; 40_000];
    let got = client
        .transcribe(&AudioFile::wav_from_pcm16(&pcm), Some("ru"))
        .await
        .unwrap();
    assert_eq!(got, "Привет, мир");

    let seen = server.seen(TRANSCRIBE);
    assert_eq!(seen.len(), 1);
    let request = &seen[0];
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {access}").as_str())
    );
    assert_eq!(request.header("chatgpt-account-id"), Some("acc-1"));
    assert_eq!(request.header("originator"), Some("Codex Desktop"));
    assert!(request
        .header("user-agent")
        .unwrap()
        .starts_with("Codex Desktop/"));
    assert_eq!(request.header("accept"), Some("application/json"));
    let content_type = request.header("content-type").unwrap();
    assert!(
        content_type.starts_with("multipart/form-data; boundary="),
        "{content_type}"
    );

    let body = String::from_utf8_lossy(&request.body);
    assert!(
        body.contains("name=\"file\"; filename=\"audio.wav\""),
        "{body}"
    );
    assert!(body.contains("Content-Type: audio/wav"), "{body}");
    assert!(body.contains("name=\"language\"\r\n\r\nru\r\n"), "{body}");
    let riff = request.body.windows(4).position(|w| w == b"RIFF").unwrap();
    assert_eq!(&request.body[riff + 8..riff + 12], b"WAVE");
    // 44-byte header + the PCM, then the closing boundary.
    assert_eq!(&request.body[riff + 44..riff + 44 + pcm.len()], &pcm[..]);
}

#[tokio::test]
async fn an_empty_language_is_left_out() {
    let server = FakeServer::start(vec![(TRANSCRIBE, vec![text("ok")])]).await;
    let mut client =
        ChatGptClient::signed_in(signed_in_store(fresh_access("a")), server.endpoints()).unwrap();
    client
        .transcribe(&AudioFile::wav_from_pcm16(&[0u8; 320]), Some(" "))
        .await
        .unwrap();
    let body = String::from_utf8_lossy(&server.seen(TRANSCRIBE)[0].body).to_string();
    assert!(!body.contains("name=\"language\""), "{body}");
}

#[tokio::test]
async fn an_expiring_token_is_refreshed_first_and_saved() {
    let server = FakeServer::start(vec![
        (TRANSCRIBE, vec![text("ok")]),
        (TOKEN, vec![token_answer("new-access", "refresh-2")]),
    ])
    .await;
    let stale = jwt(serde_json::json!({ "exp": auth::unix_now() + 10 }));
    let store = signed_in_store(stale);
    let mut client = ChatGptClient::signed_in(store.clone(), server.endpoints()).unwrap();
    client
        .transcribe(&AudioFile::wav_from_pcm16(&[0u8; 320]), None)
        .await
        .unwrap();

    let refresh = &server.seen(TOKEN)[0];
    let json: serde_json::Value = serde_json::from_slice(&refresh.body).unwrap();
    assert_eq!(json["grant_type"], "refresh_token");
    assert_eq!(json["refresh_token"], "refresh-1");
    assert_eq!(json["client_id"], auth::CLIENT_ID);
    assert_eq!(refresh.header("content-type"), Some("application/json"));
    assert_eq!(
        server.seen(TRANSCRIBE)[0].header("authorization"),
        Some("Bearer new-access")
    );
    let saved = store.load().unwrap().unwrap();
    assert_eq!(saved.access_token, "new-access");
    assert_eq!(saved.refresh_token, "refresh-2");
    assert_eq!(saved.account_id, "acc-1");
}

#[tokio::test]
async fn a_token_another_session_refreshed_is_adopted_not_refreshed_again() {
    let server = FakeServer::start(vec![
        (TRANSCRIBE, vec![(401, "{}".into()), text("ok")]),
        (
            TOKEN,
            vec![(400, r#"{"error":"refresh_token_reused"}"#.into())],
        ),
    ])
    .await;
    let store = signed_in_store(fresh_access("old"));
    let mut client = ChatGptClient::signed_in(store.clone(), server.endpoints()).unwrap();
    store
        .save(&ChatGptTokens::from_response(
            id_token("acc-1"),
            fresh_access("new"),
            "refresh-2".into(),
        ))
        .unwrap();
    client
        .transcribe(&AudioFile::wav_from_pcm16(&[0u8; 320]), None)
        .await
        .unwrap();
    assert!(server.seen(TOKEN).is_empty());
    assert_eq!(client.tokens().refresh_token, "refresh-2");
}

#[tokio::test]
async fn a_401_refreshes_once_and_retries() {
    let server = FakeServer::start(vec![
        (TRANSCRIBE, vec![(401, "{}".into()), text("second try")]),
        (TOKEN, vec![token_answer("new-access", "refresh-2")]),
    ])
    .await;
    let mut client =
        ChatGptClient::signed_in(signed_in_store(fresh_access("a")), server.endpoints()).unwrap();
    let got = client
        .transcribe(&AudioFile::wav_from_pcm16(&[0u8; 320]), None)
        .await
        .unwrap();
    assert_eq!(got, "second try");
    assert_eq!(server.seen(TOKEN).len(), 1);
    assert_eq!(server.seen(TRANSCRIBE).len(), 2);
}

#[tokio::test]
async fn a_401_after_a_good_refresh_means_openai_changed_something() {
    let server = FakeServer::start(vec![
        (TRANSCRIBE, vec![(401, "{}".into())]),
        (TOKEN, vec![token_answer("new-access", "refresh-2")]),
    ])
    .await;
    let mut client =
        ChatGptClient::signed_in(signed_in_store(fresh_access("a")), server.endpoints()).unwrap();
    let err = client
        .transcribe(&AudioFile::wav_from_pcm16(&[0u8; 320]), None)
        .await
        .unwrap_err();
    assert_eq!(err, ChatGptError::Rejected(401));
    assert!(err.to_string().contains("другого провайдера"), "{err}");
}

#[tokio::test]
async fn a_refused_refresh_means_the_session_expired() {
    let server = FakeServer::start(vec![
        (TRANSCRIBE, vec![(401, "{}".into())]),
        (TOKEN, vec![(400, r#"{"error":"invalid_grant"}"#.into())]),
    ])
    .await;
    let store = signed_in_store(fresh_access("a"));
    let mut client = ChatGptClient::signed_in(store.clone(), server.endpoints()).unwrap();
    let err = client
        .transcribe(&AudioFile::wav_from_pcm16(&[0u8; 320]), None)
        .await
        .unwrap_err();
    assert_eq!(err, ChatGptError::SessionExpired);
    assert_eq!(server.seen(TRANSCRIBE).len(), 1);
}

#[tokio::test]
async fn a_403_tries_the_codex_cli_identity_before_giving_up() {
    let server = FakeServer::start(vec![(
        TRANSCRIBE,
        vec![(403, "<html>challenge</html>".into()), text("через CLI")],
    )])
    .await;
    let mut client =
        ChatGptClient::signed_in(signed_in_store(fresh_access("a")), server.endpoints()).unwrap();
    let got = client
        .transcribe(&AudioFile::wav_from_pcm16(&[0u8; 320]), None)
        .await
        .unwrap();
    assert_eq!(got, "через CLI");
    let seen = server.seen(TRANSCRIBE);
    assert_eq!(seen[0].header("originator"), Some("Codex Desktop"));
    assert_eq!(seen[1].header("originator"), Some("codex_cli_rs"));
    assert!(seen[1]
        .header("user-agent")
        .unwrap()
        .starts_with("codex_cli_rs/"));
    assert!(
        server.seen(TOKEN).is_empty(),
        "a 403 is not a sign-in problem"
    );
}

#[tokio::test]
async fn a_residency_claim_is_sent_as_the_codex_header() {
    let server = FakeServer::start(vec![(TRANSCRIBE, vec![text("ok")])]).await;
    let access = jwt(serde_json::json!({
        "exp": auth::unix_now() + 3600,
        "https://api.openai.com/auth": { "chatgpt_data_residency": "eu" }
    }));
    let mut client = ChatGptClient::signed_in(signed_in_store(access), server.endpoints()).unwrap();
    client
        .transcribe(&AudioFile::wav_from_pcm16(&[0u8; 320]), None)
        .await
        .unwrap();
    assert_eq!(
        server.seen(TRANSCRIBE)[0].header("x-openai-internal-codex-residency"),
        Some("eu")
    );
}

#[tokio::test]
async fn a_usage_limit_body_is_the_limit_whatever_the_status() {
    for status in [400, 403, 429] {
        let server = FakeServer::start(vec![(
            TRANSCRIBE,
            vec![(
                status,
                r#"{"detail":"Codex dictation usage limit reached"}"#.into(),
            )],
        )])
        .await;
        let mut client =
            ChatGptClient::signed_in(signed_in_store(fresh_access("a")), server.endpoints())
                .unwrap();
        let err = client
            .transcribe(&AudioFile::wav_from_pcm16(&[0u8; 320]), None)
            .await
            .unwrap_err();
        assert_eq!(err, ChatGptError::RateLimited, "HTTP {status}");
        assert_eq!(server.seen(TRANSCRIBE).len(), 1, "a limit is not retried");
    }
    let shown = ChatGptError::RateLimited.to_string();
    assert!(shown.contains("лимит голосового ввода OpenAI"), "{shown}");
    assert!(shown.contains("другую модель"), "{shown}");
}

#[tokio::test]
async fn statuses_map_to_their_errors() {
    for (status, expected) in [
        (403, ChatGptError::Rejected(403)),
        (404, ChatGptError::Rejected(404)),
        (413, ChatGptError::TooLarge),
        (429, ChatGptError::RateLimited),
        (503, ChatGptError::Service(503)),
    ] {
        let server =
            FakeServer::start(vec![(TRANSCRIBE, vec![(status, "<html>cf</html>".into())])]).await;
        let mut client =
            ChatGptClient::signed_in(signed_in_store(fresh_access("a")), server.endpoints())
                .unwrap();
        let err = client
            .transcribe(&AudioFile::wav_from_pcm16(&[0u8; 320]), None)
            .await
            .unwrap_err();
        assert_eq!(err, expected, "HTTP {status}");
        let tries = if status == 403 { IDENTITIES.len() } else { 1 };
        assert_eq!(server.seen(TRANSCRIBE).len(), tries, "HTTP {status}");
        assert!(
            server.seen(TOKEN).is_empty(),
            "HTTP {status} must not refresh"
        );
    }
}

#[test]
fn every_error_is_russian_with_a_stable_code() {
    let all = [
        ChatGptError::NotSignedIn,
        ChatGptError::SessionExpired,
        ChatGptError::Rejected(403),
        ChatGptError::RateLimited,
        ChatGptError::TooLarge,
        ChatGptError::Service(502),
        ChatGptError::Network("нет связи с chatgpt.com".into()),
        ChatGptError::Storage("x".into()),
        ChatGptError::Protocol("x".into()),
        ChatGptError::PortBusy(1455),
        ChatGptError::LoginFailed("x".into()),
        ChatGptError::LoginCancelled,
    ];
    for err in all {
        let shown = err.to_string();
        assert!(
            shown.starts_with(&format!("ChatGPT [{}]: ", err.code())),
            "{shown}"
        );
        assert!(shown.chars().any(|c| ('а'..='я').contains(&c)), "{shown}");
    }
    assert!(ChatGptError::Rejected(403).to_string().contains("HTTP 403"));
    assert_eq!(ChatGptError::LoginCancelled.sentence(), "Вход отменён.");
}

#[test]
fn a_success_without_text_is_a_protocol_error() {
    assert_eq!(
        parse_transcribe_response(200, r#"{"text":"да"}"#),
        Ok("да".into())
    );
    assert!(matches!(
        parse_transcribe_response(200, "{}"),
        Err(ChatGptError::Protocol(_))
    ));
    assert!(matches!(
        parse_transcribe_response(200, "<html>"),
        Err(ChatGptError::Protocol(_))
    ));
    assert!(matches!(
        parse_transcribe_response(302, ""),
        Err(ChatGptError::Protocol(_))
    ));
}

#[tokio::test]
async fn the_provider_says_not_signed_in_before_recording() {
    let mut provider = ChatGptAsrProvider::new(Arc::new(MemoryStore::default()));
    let err = provider.connect(&AsrConfig::default()).await.unwrap_err();
    assert!(err.to_string().contains("ChatGPT [not-signed-in]"), "{err}");
}

#[tokio::test]
async fn the_provider_buffers_then_sends_once_and_finishes() {
    let server = FakeServer::start(vec![(TRANSCRIBE, vec![text("готово")])]).await;
    let mut provider =
        ChatGptAsrProvider::with_endpoints(signed_in_store(fresh_access("a")), server.endpoints());
    let config = AsrConfig {
        language: Some("ru".into()),
        ..AsrConfig::default()
    };
    provider.connect(&config).await.unwrap();
    assert!(matches!(
        provider.next_event().await.unwrap(),
        AsrEvent::Connected
    ));
    provider.send_audio(&[1u8; 640]).await.unwrap();
    provider.send_audio(&[2u8; 640]).await.unwrap();
    assert!(server.seen(TRANSCRIBE).is_empty());
    provider.finish_input().await.unwrap();
    match provider.next_event().await.unwrap() {
        AsrEvent::Final(text) => assert_eq!(text, "готово"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        provider.next_event().await.unwrap(),
        AsrEvent::Closed(None)
    ));
    let seen = server.seen(TRANSCRIBE);
    assert_eq!(seen.len(), 1);
    let riff = seen[0].body.windows(4).position(|w| w == b"RIFF").unwrap();
    let data_len = u32::from_le_bytes(seen[0].body[riff + 40..riff + 44].try_into().unwrap());
    // A short clip is padded with silence to the one-second minimum.
    assert_eq!(data_len, 32_000);
    assert_eq!(&seen[0].body[riff + 44..riff + 44 + 640], &[1u8; 640][..]);
    assert_eq!(
        &seen[0].body[riff + 44 + 640..riff + 44 + 1280],
        &[2u8; 640][..]
    );
}

#[tokio::test]
async fn the_provider_reports_failures_as_an_error_event() {
    let server = FakeServer::start(vec![(TRANSCRIBE, vec![(429, "{}".into())])]).await;
    let mut provider =
        ChatGptAsrProvider::with_endpoints(signed_in_store(fresh_access("a")), server.endpoints());
    provider.connect(&AsrConfig::default()).await.unwrap();
    provider.send_audio(&[1u8; 640]).await.unwrap();
    provider.finish_input().await.unwrap();
    let _connected = provider.next_event().await.unwrap();
    match provider.next_event().await.unwrap() {
        AsrEvent::Error(message) => {
            assert!(message.starts_with("ChatGPT [rate-limited]"), "{message}")
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn silence_only_sends_nothing() {
    let server = FakeServer::start(vec![(TRANSCRIBE, vec![text("x")])]).await;
    let mut provider =
        ChatGptAsrProvider::with_endpoints(signed_in_store(fresh_access("a")), server.endpoints());
    provider.connect(&AsrConfig::default()).await.unwrap();
    provider.finish_input().await.unwrap();
    let _connected = provider.next_event().await.unwrap();
    assert!(matches!(provider.next_event().await.unwrap(), AsrEvent::Final(t) if t.is_empty()));
    assert!(server.seen(TRANSCRIBE).is_empty());
}

#[tokio::test]
async fn login_exchanges_the_code_from_the_browser() {
    let server =
        FakeServer::start(vec![(TOKEN, vec![token_answer("acc-token", "ref-token")])]).await;
    let endpoints = server.endpoints();
    let flow = login::LoginFlow::start(&endpoints.auth, 0).await.unwrap();
    let url = url::Url::parse(flow.url()).unwrap();
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let redirect = url::Url::parse(&query["redirect_uri"]).unwrap();
    let callback = format!(
        "http://127.0.0.1:{}/auth/callback?code=the-code&state={}",
        redirect.port().unwrap(),
        query["state"]
    );
    let http = http_client().unwrap();
    let browser = tokio::spawn(async move {
        // A stray request first: the flow must keep waiting for its callback.
        let _ = reqwest::get(format!(
            "http://127.0.0.1:{}/favicon.ico",
            redirect.port().unwrap()
        ))
        .await;
        reqwest::get(callback).await.unwrap().text().await.unwrap()
    });
    let tokens = flow.finish(&http, &endpoints.auth).await.unwrap();
    assert_eq!(tokens.access_token, "acc-token");
    assert_eq!(tokens.refresh_token, "ref-token");
    assert_eq!(tokens.account_id, "acc-1");
    assert!(browser.await.unwrap().contains("Вход выполнен"));

    let exchange = &server.seen(TOKEN)[0];
    assert_eq!(
        exchange.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    let form: HashMap<String, String> = url::form_urlencoded::parse(&exchange.body)
        .into_owned()
        .collect();
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["code"], "the-code");
    assert_eq!(form["client_id"], auth::CLIENT_ID);
    assert_eq!(form["redirect_uri"], query["redirect_uri"]);
    let challenge = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        <sha2::Sha256 as sha2::Digest>::digest(form["code_verifier"].as_bytes()),
    );
    assert_eq!(challenge, query["code_challenge"]);
}

#[tokio::test]
async fn a_busy_callback_port_is_reported_in_words() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let err = match login::LoginFlow::start(&AuthEndpoints::default(), port).await {
        Err(e) => e,
        Ok(_) => panic!("bound a taken port"),
    };
    assert_eq!(err, ChatGptError::PortBusy(port));
}
