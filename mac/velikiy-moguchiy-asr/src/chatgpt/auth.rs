//! «Sign in with ChatGPT»: the OAuth 2.0 authorization-code flow with PKCE that
//! OpenAI's Codex CLI uses (github.com/openai/codex, `codex-rs/login`), and the
//! refresh of its tokens. Re-implemented here; no Codex code is copied.

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::ChatGptError;

/// The public client id of the Codex CLI. «Sign in with ChatGPT» is only
/// offered to OpenAI's own clients, which is what makes this provider unofficial.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const ISSUER: &str = "https://auth.openai.com";
/// The redirect ports registered for [`CLIENT_ID`]; the second is for when
/// the first is taken.
pub const CALLBACK_PORT: u16 = 1455;
pub const FALLBACK_CALLBACK_PORT: u16 = 1457;
pub const CALLBACK_PATH: &str = "/auth/callback";
const SCOPE: &str = "openid profile email offline_access";
/// Sent as `originator` on authorize, as Codex CLI does.
pub const ORIGINATOR: &str = "codex_cli_rs";

/// An access token this close to its `exp` is refreshed before use.
const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);
/// Tokens without a readable `exp` are refreshed this often, as Codex does.
const REFRESH_INTERVAL: Duration = Duration::from_secs(8 * 24 * 60 * 60);

/// Where the OAuth endpoints live; tests point it at a local server.
#[derive(Debug, Clone)]
pub struct AuthEndpoints {
    pub issuer: String,
}

impl Default for AuthEndpoints {
    fn default() -> Self {
        Self {
            issuer: ISSUER.into(),
        }
    }
}

impl AuthEndpoints {
    pub fn authorize_url(&self) -> String {
        format!("{}/oauth/authorize", self.issuer.trim_end_matches('/'))
    }

    pub fn token_url(&self) -> String {
        format!("{}/oauth/token", self.issuer.trim_end_matches('/'))
    }
}

/// What is kept in the Keychain (Mac) or sealed with DPAPI (Windows).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatGptTokens {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    /// `chatgpt_account_id` of the id token; sent as `ChatGPT-Account-Id`.
    pub account_id: String,
    /// Unix seconds of the last sign-in or refresh.
    pub last_refresh: u64,
}

impl fmt::Debug for ChatGptTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatGptTokens")
            .field("account_id", &"<redacted>")
            .field("last_refresh", &self.last_refresh)
            .finish_non_exhaustive()
    }
}

impl ChatGptTokens {
    /// Builds tokens from a token-endpoint answer; the account id comes from
    /// the id token, falling back to the access token.
    pub fn from_response(id_token: String, access_token: String, refresh_token: String) -> Self {
        let account_id = account_id_from(&id_token)
            .or_else(|| account_id_from(&access_token))
            .unwrap_or_default();
        Self {
            id_token,
            access_token,
            refresh_token,
            account_id,
            last_refresh: unix_now(),
        }
    }

    pub fn needs_refresh(&self, now: u64) -> bool {
        match jwt_exp(&self.access_token) {
            Some(exp) => exp <= now + REFRESH_MARGIN.as_secs(),
            None => now >= self.last_refresh + REFRESH_INTERVAL.as_secs(),
        }
    }

    pub fn email(&self) -> Option<String> {
        let claims = jwt_claims(&self.id_token)?;
        claims
            .get("email")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    /// `chatgpt_plan_type`: "plus", "pro", "team", ...
    pub fn plan(&self) -> Option<String> {
        let claims = jwt_claims(&self.id_token)?;
        auth_claim(&claims, "chatgpt_plan_type")
    }

    /// Region-pinned workspaces: the residency the tokens name, if any.
    /// Without it such a workspace gets 401 "not authorized in this region".
    pub fn residency(&self) -> Option<String> {
        [&self.access_token, &self.id_token]
            .into_iter()
            .find_map(|jwt| {
                let claims = jwt_claims(jwt)?;
                auth_claim(&claims, "chatgpt_data_residency")
                    .or_else(|| auth_claim(&claims, "chatgpt_compute_residency"))
            })
    }
}

/// Where the token store keeps the tokens: never config.yaml, never a log.
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<ChatGptTokens>, String>;
    fn save(&self, tokens: &ChatGptTokens) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The payload of a JWT, unverified: the tokens come straight from OpenAI over
/// TLS and are only read for routing hints, never trusted for access decisions.
pub fn jwt_claims(jwt: &str) -> Option<serde_json::Value> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn jwt_exp(jwt: &str) -> Option<u64> {
    jwt_claims(jwt)?.get("exp")?.as_u64()
}

fn auth_claim(claims: &serde_json::Value, key: &str) -> Option<String> {
    claims
        .get("https://api.openai.com/auth")?
        .get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn account_id_from(jwt: &str) -> Option<String> {
    auth_claim(&jwt_claims(jwt)?, "chatgpt_account_id")
}

/// PKCE (RFC 7636) verifier and its S256 challenge.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Result<Self, ChatGptError> {
        let verifier = random_urlsafe(64)?;
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        Ok(Self {
            verifier,
            challenge,
        })
    }
}

pub fn random_urlsafe(bytes: usize) -> Result<String, ChatGptError> {
    use ring::rand::SecureRandom;
    let mut buf = vec![0u8; bytes];
    ring::rand::SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| {
            ChatGptError::Protocol("системный генератор случайных чисел недоступен".into())
        })?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

pub fn authorize_url(
    endpoints: &AuthEndpoints,
    redirect_uri: &str,
    pkce: &Pkce,
    state: &str,
) -> String {
    let mut url = url::Url::parse(&endpoints.authorize_url()).expect("issuer is a valid URL");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("state", state)
        .append_pair("originator", ORIGINATOR);
    url.into()
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

pub async fn exchange_code(
    client: &reqwest::Client,
    endpoints: &AuthEndpoints,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<ChatGptTokens, ChatGptError> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", CLIENT_ID),
        ("code_verifier", verifier),
    ];
    let response = client
        .post(endpoints.token_url())
        .form(&form)
        .send()
        .await
        .map_err(ChatGptError::network)?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        log::warn!("[ChatGPT] code exchange refused: HTTP {status}");
        return Err(ChatGptError::LoginFailed(format!(
            "сервер входа ответил HTTP {}{}",
            status.as_u16(),
            oauth_error_suffix(&body)
        )));
    }
    let tokens: TokenResponse = response
        .json()
        .await
        .map_err(|e| ChatGptError::Protocol(format!("ответ сервера входа не прочитан: {e}")))?;
    match (tokens.id_token, tokens.access_token, tokens.refresh_token) {
        (Some(id), Some(access), Some(refresh)) => {
            let tokens = ChatGptTokens::from_response(id, access, refresh);
            if tokens.account_id.is_empty() {
                return Err(ChatGptError::LoginFailed(
                    "в ответе нет аккаунта ChatGPT: войдите аккаунтом с подпиской".into(),
                ));
            }
            Ok(tokens)
        }
        _ => Err(ChatGptError::Protocol(
            "сервер входа не прислал все токены".into(),
        )),
    }
}

/// Swaps the refresh token for new tokens, keeping what the answer leaves out.
pub async fn refresh(
    client: &reqwest::Client,
    endpoints: &AuthEndpoints,
    current: &ChatGptTokens,
) -> Result<ChatGptTokens, ChatGptError> {
    let body = serde_json::json!({
        "client_id": CLIENT_ID,
        "grant_type": "refresh_token",
        "refresh_token": current.refresh_token,
    });
    let response = client
        .post(endpoints.token_url())
        .json(&body)
        .send()
        .await
        .map_err(ChatGptError::network)?;
    let status = response.status();
    if !status.is_success() {
        log::warn!("[ChatGPT] token refresh refused: HTTP {status}");
        // 400/401 with invalid_grant or refresh_token_* means the sign-in is
        // over (expired, revoked, or the token was used by another client).
        return Err(if matches!(status.as_u16(), 400 | 401 | 403) {
            ChatGptError::SessionExpired
        } else if status.as_u16() == 429 {
            ChatGptError::RateLimited
        } else {
            ChatGptError::Service(status.as_u16())
        });
    }
    let fresh: TokenResponse = response
        .json()
        .await
        .map_err(|e| ChatGptError::Protocol(format!("ответ обновления не прочитан: {e}")))?;
    let id_token = fresh.id_token.unwrap_or_else(|| current.id_token.clone());
    let access_token = fresh
        .access_token
        .ok_or_else(|| ChatGptError::Protocol("сервер не прислал новый токен доступа".into()))?;
    let refresh_token = fresh
        .refresh_token
        .unwrap_or_else(|| current.refresh_token.clone());
    let mut tokens = ChatGptTokens::from_response(id_token, access_token, refresh_token);
    if tokens.account_id.is_empty() {
        tokens.account_id = current.account_id.clone();
    }
    Ok(tokens)
}

/// ` (access_denied)`-style tail from an OAuth error body, without any free
/// text the server put there.
fn oauth_error_suffix(body: &str) -> String {
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            let error = v.get("error")?;
            error
                .as_str()
                .or_else(|| error.get("code").and_then(|c| c.as_str()))
                .map(str::to_string)
        });
    match code {
        Some(code)
            if code.len() < 64 && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
        {
            format!(" ({code})")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An unsigned JWT with `claims` as its payload.
    pub(crate) fn jwt(claims: serde_json::Value) -> String {
        let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.sig",
            enc(br#"{"alg":"none"}"#),
            enc(claims.to_string().as_bytes())
        )
    }

    pub(crate) fn id_token(account: &str) -> String {
        jwt(serde_json::json!({
            "email": "user@example.com",
            "https://api.openai.com/auth": {
                "chatgpt_account_id": account,
                "chatgpt_plan_type": "plus",
            }
        }))
    }

    #[test]
    fn claims_give_account_email_and_plan() {
        let tokens = ChatGptTokens::from_response(id_token("acc-1"), "a".into(), "r".into());
        assert_eq!(tokens.account_id, "acc-1");
        assert_eq!(tokens.email().as_deref(), Some("user@example.com"));
        assert_eq!(tokens.plan().as_deref(), Some("plus"));
        assert_eq!(tokens.residency(), None);

        let pinned = ChatGptTokens::from_response(
            id_token("acc-1"),
            jwt(serde_json::json!({
                "https://api.openai.com/auth": { "chatgpt_data_residency": "eu" }
            })),
            "r".into(),
        );
        assert_eq!(pinned.residency().as_deref(), Some("eu"));
    }

    #[test]
    fn debug_output_hides_every_secret() {
        let tokens = ChatGptTokens::from_response(
            id_token("acc-1"),
            "secret-access".into(),
            "secret-refresh".into(),
        );
        let shown = format!("{tokens:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(!shown.contains("acc-1"), "{shown}");
    }

    #[test]
    fn refresh_is_due_near_expiry_and_after_the_interval_without_exp() {
        let now = 1_000_000;
        let mut tokens = ChatGptTokens::from_response(
            id_token("a"),
            jwt(serde_json::json!({ "exp": now + 3600 })),
            "r".into(),
        );
        assert!(!tokens.needs_refresh(now));
        assert!(tokens.needs_refresh(now + 3600 - 60));

        tokens.access_token = "opaque".into();
        tokens.last_refresh = now;
        assert!(!tokens.needs_refresh(now + 60));
        assert!(tokens.needs_refresh(now + REFRESH_INTERVAL.as_secs()));
    }

    #[test]
    fn pkce_challenge_is_the_s256_of_the_verifier() {
        let pkce = Pkce::new().unwrap();
        assert!(pkce.verifier.len() >= 43);
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(pkce.verifier.as_bytes()));
        assert_eq!(pkce.challenge, expected);
        assert_ne!(Pkce::new().unwrap().verifier, pkce.verifier);
    }

    #[test]
    fn authorize_url_carries_the_codex_parameters() {
        let pkce = Pkce::new().unwrap();
        let url = authorize_url(
            &AuthEndpoints::default(),
            &redirect_uri(CALLBACK_PORT),
            &pkce,
            "st",
        );
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(parsed.host_str(), Some("auth.openai.com"));
        assert_eq!(parsed.path(), "/oauth/authorize");
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], CLIENT_ID);
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:1455/auth/callback");
        assert_eq!(q["code_challenge"], pkce.challenge);
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["state"], "st");
        assert!(q["scope"].contains("offline_access"));
    }

    #[test]
    fn oauth_error_tail_keeps_only_a_code() {
        assert_eq!(
            oauth_error_suffix(r#"{"error":"invalid_grant"}"#),
            " (invalid_grant)"
        );
        assert_eq!(
            oauth_error_suffix(r#"{"error":{"code":"refresh_token_expired","message":"x"}}"#),
            " (refresh_token_expired)"
        );
        assert_eq!(oauth_error_suffix(r#"{"error":"has spaces <b>"}"#), "");
        assert_eq!(oauth_error_suffix("not json"), "");
    }
}
