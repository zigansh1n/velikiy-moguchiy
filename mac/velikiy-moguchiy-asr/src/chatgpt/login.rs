//! The browser half of «Sign in with ChatGPT»: a one-shot HTTP listener on
//! 127.0.0.1 that receives the authorization code and swaps it for tokens.

use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::auth::{self, AuthEndpoints, ChatGptTokens, Pkce};
use super::ChatGptError;

const MAX_REQUEST_HEAD: usize = 16 * 1024;

pub struct LoginFlow {
    listener: TcpListener,
    redirect_uri: String,
    pkce: Pkce,
    state: String,
    url: String,
}

enum Callback {
    Code(String),
    Denied(String),
    Cancel,
    Other,
}

impl LoginFlow {
    /// Binds the callback port; `port` 0 picks a free one (tests only: OpenAI
    /// accepts the registered ports alone). [`auth::CALLBACK_PORT`] falls back
    /// to [`auth::FALLBACK_CALLBACK_PORT`] when another sign-in holds it.
    pub async fn start(endpoints: &AuthEndpoints, port: u16) -> Result<Self, ChatGptError> {
        let listener = match bind(port).await {
            Err(ChatGptError::PortBusy(_)) if port == auth::CALLBACK_PORT => {
                bind(auth::FALLBACK_CALLBACK_PORT)
                    .await
                    .map_err(|e| match e {
                        ChatGptError::PortBusy(_) => ChatGptError::PortBusy(port),
                        other => other,
                    })?
            }
            other => other?,
        };
        let port = listener
            .local_addr()
            .map_err(|e| ChatGptError::LoginFailed(e.to_string()))?
            .port();
        let redirect_uri = auth::redirect_uri(port);
        let pkce = Pkce::new()?;
        let state = auth::random_urlsafe(32)?;
        let url = auth::authorize_url(endpoints, &redirect_uri, &pkce, &state);
        Ok(Self {
            listener,
            redirect_uri,
            pkce,
            state,
            url,
        })
    }

    /// The page to open in the user's browser.
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Serves the callback until the browser comes back with a code (or an
    /// error), then exchanges it. Drop the future to stop listening.
    pub async fn finish(
        self,
        client: &reqwest::Client,
        endpoints: &AuthEndpoints,
    ) -> Result<ChatGptTokens, ChatGptError> {
        loop {
            let (mut stream, _) = self
                .listener
                .accept()
                .await
                .map_err(|e| ChatGptError::LoginFailed(e.to_string()))?;
            let target = match read_request_target(&mut stream).await {
                Some(target) => target,
                None => continue,
            };
            match parse_callback(&target, &self.state) {
                Callback::Code(code) => {
                    let result = auth::exchange_code(
                        client,
                        endpoints,
                        &code,
                        &self.redirect_uri,
                        &self.pkce.verifier,
                    )
                    .await;
                    let page = match &result {
                        Ok(_) => page(
                            "Вход выполнен",
                            "Вернитесь в «Великий &amp; Могучий». Эту вкладку можно закрыть.",
                        ),
                        Err(e) => page("Не удалось войти", &html_escape(&e.to_string())),
                    };
                    respond(&mut stream, 200, &page).await;
                    return result;
                }
                Callback::Denied(reason) => {
                    respond(
                        &mut stream,
                        200,
                        &page("Вход отменён", "Можно закрыть вкладку и попробовать снова."),
                    )
                    .await;
                    return Err(ChatGptError::LoginFailed(reason));
                }
                Callback::Cancel => {
                    respond(&mut stream, 200, &page("Вход отменён", "")).await;
                    return Err(ChatGptError::LoginCancelled);
                }
                Callback::Other => respond(&mut stream, 404, "").await,
            }
        }
    }
}

async fn bind(port: u16) -> Result<TcpListener, ChatGptError> {
    TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            ChatGptError::PortBusy(port)
        } else {
            ChatGptError::LoginFailed(format!("не удалось открыть порт {port}: {e}"))
        }
    })
}

/// `/auth/callback?code=…&state=…` → the code, once the state matches.
fn parse_callback(target: &str, expected_state: &str) -> Callback {
    let Ok(url) = url::Url::parse(&format!("http://localhost{target}")) else {
        return Callback::Other;
    };
    if url.path() == "/cancel" {
        return Callback::Cancel;
    }
    if url.path() != auth::CALLBACK_PATH {
        return Callback::Other;
    }
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    if query.get("state").map(String::as_str) != Some(expected_state) {
        // A stale tab or a forged request: not ours, keep waiting.
        log::warn!("[ChatGPT] login callback with a foreign state ignored");
        return Callback::Other;
    }
    if let Some(error) = query.get("error") {
        let safe: String = error
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(64)
            .collect();
        return Callback::Denied(format!("вход не подтверждён ({safe})"));
    }
    match query.get("code") {
        Some(code) if !code.is_empty() => Callback::Code(code.clone()),
        _ => Callback::Other,
    }
}

async fn read_request_target(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 || buf.len() + n > MAX_REQUEST_HEAD {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf);
    let mut parts = head.lines().next()?.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    parts.next().map(str::to_string)
}

async fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn page(title: &str, text: &str) -> String {
    format!(
        "<!doctype html><html lang=\"ru\"><head><meta charset=\"utf-8\"><title>{title}</title>\
         <style>body{{font:16px -apple-system,Segoe UI,sans-serif;max-width:32em;margin:15vh auto;padding:0 16px}}</style>\
         </head><body><h1>{title}</h1><p>{text}</p></body></html>"
    )
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_needs_the_right_path_and_state() {
        assert!(matches!(
            parse_callback("/auth/callback?code=c1&state=s", "s"),
            Callback::Code(c) if c == "c1"
        ));
        assert!(matches!(
            parse_callback("/auth/callback?code=c1&state=x", "s"),
            Callback::Other
        ));
        assert!(matches!(
            parse_callback("/auth/callback?code=c1", "s"),
            Callback::Other
        ));
        assert!(matches!(
            parse_callback("/favicon.ico", "s"),
            Callback::Other
        ));
        assert!(matches!(parse_callback("/cancel", "s"), Callback::Cancel));
        assert!(matches!(
            parse_callback("/auth/callback?error=access_denied&state=s", "s"),
            Callback::Denied(r) if r.contains("access_denied")
        ));
        assert!(matches!(
            parse_callback("/auth/callback?error=%3Cscript%3E&state=s", "s"),
            Callback::Denied(r) if !r.contains('<')
        ));
    }
}
