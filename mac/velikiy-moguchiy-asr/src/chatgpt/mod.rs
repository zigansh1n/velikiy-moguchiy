//! Transcription through the user's own ChatGPT subscription: the endpoint the
//! ChatGPT apps use for voice input, signed in the way OpenAI's Codex CLI is.
//! Unofficial: OpenAI documents neither, and either may change at any time.
//!
//! Record-then-transcribe: audio is buffered while the user speaks and sent as
//! one WAV file when the recording ends.

pub mod auth;
pub mod login;

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::config::AsrConfig;
use crate::error::{AsrError, Result};
use crate::event::AsrEvent;
use crate::provider::AsrProvider;
use auth::{AuthEndpoints, ChatGptTokens, TokenStore};

pub const TRANSCRIBE_URL: &str = "https://chatgpt.com/backend-api/transcribe";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Codex refuses clips shorter than a second; shorter ones are padded.
const MIN_AUDIO_SECONDS: usize = 1;

/// `originator` and `User-Agent` pairs, tried in order on a 403: Cloudflare
/// answers 403 to requests without a first-party identity. The first is the
/// one seen working against `/transcribe`; the second is Codex CLI's own.
const IDENTITIES: &[(&str, &str)] = &[
    ("Codex Desktop", "Codex Desktop/0.159.0"),
    ("codex_cli_rs", "codex_cli_rs/0.159.0"),
];

/// Every failure the user can meet, worded for them. `Display` gives
/// `ChatGPT [code]: text`; the Mac app reads the code to pick its notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatGptError {
    NotSignedIn,
    SessionExpired,
    /// 401 after a successful refresh, or 403: the endpoint no longer accepts
    /// this client.
    Rejected(u16),
    RateLimited,
    TooLarge,
    Service(u16),
    Network(String),
    Storage(String),
    Protocol(String),
    PortBusy(u16),
    LoginFailed(String),
    LoginCancelled,
}

impl ChatGptError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotSignedIn => "not-signed-in",
            Self::SessionExpired => "session-expired",
            Self::Rejected(_) => "rejected",
            Self::RateLimited => "rate-limited",
            Self::TooLarge => "too-large",
            Self::Service(_) => "service",
            Self::Network(_) => "network",
            Self::Storage(_) => "storage",
            Self::Protocol(_) => "protocol",
            Self::PortBusy(_) => "port-busy",
            Self::LoginFailed(_) => "login-failed",
            Self::LoginCancelled => "login-cancelled",
        }
    }

    pub(crate) fn network(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Self::Network("ChatGPT не ответил вовремя".into())
        } else {
            Self::Network("нет связи с chatgpt.com".into())
        }
    }

    /// The text alone, as a sentence for a settings window.
    pub fn sentence(&self) -> String {
        let text = self.text();
        let mut chars = text.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => text,
        }
    }

    fn text(&self) -> String {
        match self {
            Self::NotSignedIn => "вы не вошли в ChatGPT. Войдите: Настройки → Распознавание → «Войти через ChatGPT».".into(),
            Self::SessionExpired => "вход в ChatGPT истёк или отозван. Войдите снова: Настройки → Распознавание.".into(),
            Self::Rejected(status) => format!(
                "ChatGPT отклонил запрос (HTTP {status}). Похоже, OpenAI что-то изменил в неофициальном \
                 способе входа. Выберите другого провайдера в Настройках → Распознавание."
            ),
            Self::RateLimited => "ChatGPT временно ограничил расшифровку для вашего аккаунта. Подождите и повторите.".into(),
            Self::TooLarge => "запись слишком длинная для ChatGPT. Продиктуйте короче.".into(),
            Self::Service(status) => format!("сервис ChatGPT сейчас не работает (HTTP {status}). Повторите позже."),
            Self::Network(detail) => format!("{detail}. Проверьте интернет или VPN."),
            Self::Storage(detail) => format!("не удалось прочитать или сохранить вход: {detail}."),
            Self::Protocol(detail) => format!("неожиданный ответ ChatGPT: {detail}."),
            Self::PortBusy(port) => format!(
                "порт {port} занят: его держит другой вход через ChatGPT (например, Codex). \
                 Закройте его и повторите."
            ),
            Self::LoginFailed(detail) => format!("не удалось войти: {detail}."),
            Self::LoginCancelled => "вход отменён.".into(),
        }
    }
}

impl fmt::Display for ChatGptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChatGPT [{}]: {}", self.code(), self.text())
    }
}

impl std::error::Error for ChatGptError {}

#[derive(Debug, Clone)]
pub struct ChatGptEndpoints {
    pub transcribe_url: String,
    pub auth: AuthEndpoints,
}

impl Default for ChatGptEndpoints {
    fn default() -> Self {
        Self {
            transcribe_url: TRANSCRIBE_URL.into(),
            auth: AuthEndpoints::default(),
        }
    }
}

pub fn http_client() -> std::result::Result<reqwest::Client, ChatGptError> {
    reqwest::Client::builder()
        // Tokens travel in headers: never follow a redirect anywhere.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| ChatGptError::Protocol(format!("HTTP-клиент не создан: {e}")))
}

/// One audio file in a container `/transcribe` accepts.
pub struct AudioFile {
    pub bytes: Vec<u8>,
    /// Its extension must name the container: `audio.wav`, `audio.m4a`, ...
    pub file_name: &'static str,
    pub mime: &'static str,
}

impl AudioFile {
    /// 16-bit mono PCM at 16 kHz, as the app records it.
    pub fn wav_from_pcm16(pcm: &[u8]) -> Self {
        let min_len = 16_000 * 2 * MIN_AUDIO_SECONDS;
        let bytes = if pcm.len() < min_len {
            let mut padded = pcm.to_vec();
            padded.resize(min_len, 0);
            crate::glm::wrap_wav(&padded)
        } else {
            crate::glm::wrap_wav(pcm)
        };
        Self {
            bytes,
            file_name: "audio.wav",
            mime: "audio/wav",
        }
    }
}

/// A signed-in client: keeps the tokens fresh and writes refreshed ones back.
pub struct ChatGptClient {
    http: reqwest::Client,
    endpoints: ChatGptEndpoints,
    store: Arc<dyn TokenStore>,
    tokens: ChatGptTokens,
}

impl ChatGptClient {
    pub fn signed_in(
        store: Arc<dyn TokenStore>,
        endpoints: ChatGptEndpoints,
    ) -> std::result::Result<Self, ChatGptError> {
        let tokens = store
            .load()
            .map_err(ChatGptError::Storage)?
            .ok_or(ChatGptError::NotSignedIn)?;
        Ok(Self {
            http: http_client()?,
            endpoints,
            store,
            tokens,
        })
    }

    pub fn tokens(&self) -> &ChatGptTokens {
        &self.tokens
    }

    /// Refresh tokens rotate: a second refresh with the same token is
    /// refused as reuse, so refreshes are serialized and a token another
    /// session already replaced is adopted instead.
    async fn refresh(&mut self) -> std::result::Result<(), ChatGptError> {
        static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _one_at_a_time = REFRESH.lock().await;
        if let Ok(Some(stored)) = self.store.load() {
            if stored.refresh_token != self.tokens.refresh_token {
                self.tokens = stored;
                return Ok(());
            }
        }
        log::info!("[ChatGPT] refreshing the access token");
        let fresh = auth::refresh(&self.http, &self.endpoints.auth, &self.tokens).await?;
        self.store.save(&fresh).map_err(ChatGptError::Storage)?;
        self.tokens = fresh;
        Ok(())
    }

    /// The text of `audio`. Refreshes ahead of expiry, and once more when the
    /// server answers 401 to a token it still considered fresh.
    pub async fn transcribe(
        &mut self,
        audio: &AudioFile,
        language: Option<&str>,
    ) -> std::result::Result<String, ChatGptError> {
        if self.tokens.needs_refresh(auth::unix_now()) {
            self.refresh().await?;
        }
        let mut refreshed = false;
        let mut identity = 0;
        loop {
            match self.post(audio, language, IDENTITIES[identity]).await {
                Err(ChatGptError::Rejected(401)) if !refreshed => {
                    refreshed = true;
                    self.refresh().await?;
                }
                Err(ChatGptError::Rejected(403)) if identity + 1 < IDENTITIES.len() => {
                    identity += 1;
                }
                other => return other,
            }
        }
    }

    async fn post(
        &self,
        audio: &AudioFile,
        language: Option<&str>,
        (originator, user_agent): (&str, &str),
    ) -> std::result::Result<String, ChatGptError> {
        let file = reqwest::multipart::Part::bytes(audio.bytes.clone())
            .file_name(audio.file_name)
            .mime_str(audio.mime)
            .map_err(|e| ChatGptError::Protocol(e.to_string()))?;
        let mut form = reqwest::multipart::Form::new().part("file", file);
        if let Some(language) = language.map(str::trim).filter(|l| !l.is_empty()) {
            form = form.text("language", language.to_string());
        }
        let mut request = self
            .http
            .post(&self.endpoints.transcribe_url)
            .bearer_auth(&self.tokens.access_token)
            .header("ChatGPT-Account-Id", &self.tokens.account_id)
            .header("originator", originator)
            .header(reqwest::header::USER_AGENT, user_agent)
            .header(reqwest::header::ACCEPT, "application/json")
            .multipart(form);
        if let Some(residency) = self.tokens.residency() {
            request = request.header("x-openai-internal-codex-residency", residency);
        }
        let response = request.send().await.map_err(ChatGptError::network)?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(ChatGptError::network)?;
        if !(200..300).contains(&status) {
            log::warn!("[ChatGPT] transcribe refused: HTTP {status}");
        }
        parse_transcribe_response(status, &body)
    }
}

fn parse_transcribe_response(status: u16, body: &str) -> std::result::Result<String, ChatGptError> {
    match status {
        200..=299 => {
            let json: serde_json::Value = serde_json::from_str(body)
                .map_err(|_| ChatGptError::Protocol("ответ не JSON".into()))?;
            json.get("text")
                .and_then(|t| t.as_str())
                .map(|t| t.trim().to_string())
                .ok_or_else(|| ChatGptError::Protocol("в ответе нет текста".into()))
        }
        // 404/405: the endpoint moved or went away.
        401 | 403 | 404 | 405 => Err(ChatGptError::Rejected(status)),
        413 => Err(ChatGptError::TooLarge),
        429 => Err(ChatGptError::RateLimited),
        500..=599 => Err(ChatGptError::Service(status)),
        _ => Err(ChatGptError::Protocol(format!("HTTP {status}"))),
    }
}

/// «Проверить»: one real request with the stored sign-in, refreshing it if due.
pub async fn check_with_audio(
    store: Arc<dyn TokenStore>,
    endpoints: ChatGptEndpoints,
) -> std::result::Result<String, ChatGptError> {
    let mut client = ChatGptClient::signed_in(store, endpoints)?;
    // Silence: a real request that carries no voice.
    let silence = vec![0u8; 16_000 * 2 * MIN_AUDIO_SECONDS];
    client
        .transcribe(&AudioFile::wav_from_pcm16(&silence), None)
        .await
}

/// The provider a dictation session drives.
pub struct ChatGptAsrProvider {
    store: Arc<dyn TokenStore>,
    endpoints: ChatGptEndpoints,
    client: Option<ChatGptClient>,
    language: Option<String>,
    audio: Vec<u8>,
    pending: VecDeque<AsrEvent>,
    finished: bool,
}

impl ChatGptAsrProvider {
    pub fn new(store: Arc<dyn TokenStore>) -> Self {
        Self::with_endpoints(store, ChatGptEndpoints::default())
    }

    pub fn with_endpoints(store: Arc<dyn TokenStore>, endpoints: ChatGptEndpoints) -> Self {
        Self {
            store,
            endpoints,
            client: None,
            language: None,
            audio: Vec::new(),
            pending: VecDeque::new(),
            finished: false,
        }
    }
}

#[async_trait::async_trait]
impl AsrProvider for ChatGptAsrProvider {
    async fn connect(&mut self, config: &AsrConfig) -> Result<()> {
        let client = ChatGptClient::signed_in(self.store.clone(), self.endpoints.clone())
            .map_err(|e| AsrError::Connection(e.to_string()))?;
        self.client = Some(client);
        self.language = config.language.clone();
        self.pending.push_back(AsrEvent::Connected);
        Ok(())
    }

    async fn send_audio(&mut self, frame: &[u8]) -> Result<()> {
        if self.client.is_none() {
            return Err(AsrError::Connection("not connected".into()));
        }
        self.audio.extend_from_slice(frame);
        Ok(())
    }

    async fn finish_input(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        if self.audio.is_empty() {
            self.pending.push_back(AsrEvent::Final(String::new()));
            return Ok(());
        }
        let client = self
            .client
            .as_mut()
            .ok_or_else(|| AsrError::Connection("not connected".into()))?;
        let audio = AudioFile::wav_from_pcm16(&self.audio);
        log::info!("[ChatGPT] uploading {} bytes of WAV", audio.bytes.len());
        // The error goes out as an event, not as Err, so the session shows
        // the Russian text as it is instead of wrapping it in "ASR finish failed".
        match client.transcribe(&audio, self.language.as_deref()).await {
            Ok(text) => {
                log::info!("[ChatGPT] transcript: {} chars", text.chars().count());
                self.pending.push_back(AsrEvent::Final(text));
            }
            Err(e) => self.pending.push_back(AsrEvent::Error(e.to_string())),
        }
        Ok(())
    }

    async fn next_event(&mut self) -> Result<AsrEvent> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(event);
            }
            if self.finished {
                return Ok(AsrEvent::Closed(None));
            }
            // Nothing arrives before the recording ends; the session polls
            // this alongside the audio channel and drops it on every frame.
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.client = None;
        self.audio.clear();
        self.pending.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
