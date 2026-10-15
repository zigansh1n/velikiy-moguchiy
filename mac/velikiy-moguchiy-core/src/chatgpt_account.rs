//! The ChatGPT sign-in behind the `chatgpt` provider: where its tokens are
//! kept, and the sign-in, check and sign-out the settings windows call.
//!
//! Tokens never go to config.yaml or the log. On a Mac they live in the login
//! Keychain; the Windows app installs a [`SealedFileStore`] backed by DPAPI.

use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use velikiy_moguchiy_asr::chatgpt::{self, login::LoginFlow, ChatGptEndpoints};

pub use velikiy_moguchiy_asr::chatgpt::auth::{ChatGptTokens, TokenStore};
pub use velikiy_moguchiy_asr::chatgpt::ChatGptError;

/// How long the browser has to come back before the sign-in gives up.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

static INSTALLED: Mutex<Option<Arc<dyn TokenStore>>> = Mutex::new(None);

/// Replaces the platform's default store (the Windows app, tests).
pub fn install_store(store: Arc<dyn TokenStore>) {
    *INSTALLED.lock().unwrap_or_else(|e| e.into_inner()) = Some(store);
}

/// The installed store, else the Keychain on a Mac, else one that refuses.
pub fn store() -> Arc<dyn TokenStore> {
    if let Some(store) = INSTALLED.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return store;
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(KeychainStore)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Arc::new(NoStore)
    }
}

#[cfg(target_os = "macos")]
pub struct KeychainStore;

#[cfg(target_os = "macos")]
impl KeychainStore {
    const SERVICE: &'static str = "io.github.zigansh1n.velikiy-moguchiy.chatgpt";
    const ACCOUNT: &'static str = "oauth-tokens";
    /// errSecItemNotFound
    const NOT_FOUND: i32 = -25300;
}

#[cfg(target_os = "macos")]
impl TokenStore for KeychainStore {
    fn load(&self) -> Result<Option<ChatGptTokens>, String> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(Self::SERVICE, Self::ACCOUNT) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| "запись в связке ключей повреждена".to_string()),
            Err(e) if e.code() == Self::NOT_FOUND => Ok(None),
            Err(e) => Err(format!("связка ключей: {e}")),
        }
    }

    fn save(&self, tokens: &ChatGptTokens) -> Result<(), String> {
        use security_framework::passwords::set_generic_password;
        let bytes = serde_json::to_vec(tokens).map_err(|e| e.to_string())?;
        set_generic_password(Self::SERVICE, Self::ACCOUNT, &bytes)
            .map_err(|e| format!("связка ключей: {e}"))
    }

    fn clear(&self) -> Result<(), String> {
        use security_framework::passwords::delete_generic_password;
        match delete_generic_password(Self::SERVICE, Self::ACCOUNT) {
            Err(e) if e.code() != Self::NOT_FOUND => Err(format!("связка ключей: {e}")),
            _ => Ok(()),
        }
    }
}

#[cfg(not(target_os = "macos"))]
struct NoStore;

#[cfg(not(target_os = "macos"))]
impl TokenStore for NoStore {
    fn load(&self) -> Result<Option<ChatGptTokens>, String> {
        Ok(None)
    }
    fn save(&self, _: &ChatGptTokens) -> Result<(), String> {
        Err("на этой системе нет защищённого хранилища".into())
    }
    fn clear(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Encryption bound to the current user (DPAPI on Windows).
pub trait Vault: Send + Sync {
    fn seal(&self, plain: &[u8]) -> Result<Vec<u8>, String>;
    fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, String>;
}

/// Tokens sealed by a [`Vault`] in their own owner-only file next to config.yaml.
pub struct SealedFileStore {
    path: PathBuf,
    vault: Box<dyn Vault>,
}

impl SealedFileStore {
    pub fn new(vault: Box<dyn Vault>) -> Self {
        Self::at(crate::config::config_dir().join("chatgpt-auth.bin"), vault)
    }

    pub fn at(path: PathBuf, vault: Box<dyn Vault>) -> Self {
        Self { path, vault }
    }
}

impl TokenStore for SealedFileStore {
    fn load(&self) -> Result<Option<ChatGptTokens>, String> {
        let sealed = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        let plain = self.vault.open(&sealed)?;
        serde_json::from_slice(&plain)
            .map(Some)
            .map_err(|_| "файл входа повреждён".to_string())
    }

    fn save(&self, tokens: &ChatGptTokens) -> Result<(), String> {
        let plain = serde_json::to_vec(tokens).map_err(|e| e.to_string())?;
        let sealed = self.vault.seal(&plain)?;
        let tmp = self.path.with_extension("bin.tmp");
        crate::config::write_owner_only(&tmp, &sealed).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    fn clear(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Status {
    pub signed_in: bool,
    pub email: Option<String>,
    pub plan: Option<String>,
    /// Set when the store could not be read.
    pub error: Option<String>,
}

pub fn status() -> Status {
    match store().load() {
        Ok(Some(tokens)) => Status {
            signed_in: true,
            email: tokens.email(),
            plan: tokens.plan(),
            error: None,
        },
        Ok(None) => Status {
            signed_in: false,
            email: None,
            plan: None,
            error: None,
        },
        Err(e) => Status {
            signed_in: false,
            email: None,
            plan: None,
            error: Some(ChatGptError::Storage(e).to_string()),
        },
    }
}

/// Forgets the tokens on this computer. OpenAI keeps the session until it
/// expires; there is no revocation endpoint for this client.
pub fn logout() -> Result<(), String> {
    cancel_login();
    store().clear()
}

struct PendingLogin {
    cancel: Arc<tokio::sync::Notify>,
    result: mpsc::Receiver<Result<ChatGptTokens, ChatGptError>>,
    thread: std::thread::JoinHandle<()>,
}

static PENDING: Mutex<Option<PendingLogin>> = Mutex::new(None);
static CANCEL: Mutex<Option<Arc<tokio::sync::Notify>>> = Mutex::new(None);

/// Opens the callback port and returns the page to show in the browser. The
/// sign-in then completes in the background; [`wait_login`] reports it.
pub fn start_login() -> Result<String, ChatGptError> {
    start_login_with(
        ChatGptEndpoints::default(),
        velikiy_moguchiy_asr::chatgpt::auth::CALLBACK_PORT,
    )
}

fn start_login_with(endpoints: ChatGptEndpoints, port: u16) -> Result<String, ChatGptError> {
    cancel_login();
    if let Some(previous) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() {
        // Its listener must be closed before the port is bound again.
        let _ = previous.thread.join();
    }
    let cancel = Arc::new(tokio::sync::Notify::new());
    let (url_tx, url_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let store = store();
    let task_cancel = cancel.clone();
    let thread = std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let _ = url_tx.send(Err(ChatGptError::LoginFailed(e.to_string())));
                return;
            }
        };
        runtime.block_on(async move {
            let flow = match LoginFlow::start(&endpoints.auth, port).await {
                Ok(flow) => flow,
                Err(e) => {
                    let _ = url_tx.send(Err(e));
                    return;
                }
            };
            let _ = url_tx.send(Ok(flow.url().to_string()));
            let http = match chatgpt::http_client() {
                Ok(http) => http,
                Err(e) => {
                    let _ = result_tx.send(Err(e));
                    return;
                }
            };
            let result = tokio::select! {
                result = flow.finish(&http, &endpoints.auth) => result,
                _ = task_cancel.notified() => Err(ChatGptError::LoginCancelled),
                _ = tokio::time::sleep(LOGIN_TIMEOUT) => Err(ChatGptError::LoginFailed(
                    "браузер не вернулся за 10 минут".into(),
                )),
            };
            let result = result.and_then(|tokens| {
                store.save(&tokens).map_err(ChatGptError::Storage)?;
                Ok(tokens)
            });
            let _ = result_tx.send(result);
        });
    });
    let url = url_rx
        .recv()
        .unwrap_or_else(|_| Err(ChatGptError::LoginFailed("вход прервался".into())));
    match url {
        Ok(url) => {
            *CANCEL.lock().unwrap_or_else(|e| e.into_inner()) = Some(cancel.clone());
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(PendingLogin {
                cancel,
                result: result_rx,
                thread,
            });
            log::info!("[ChatGPT] sign-in started, waiting for the browser");
            Ok(url)
        }
        Err(e) => {
            let _ = thread.join();
            Err(e)
        }
    }
}

/// Blocks until the sign-in started by [`start_login`] ends.
pub fn wait_login() -> Result<Status, ChatGptError> {
    let pending = PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or(ChatGptError::LoginCancelled)?;
    let result = pending
        .result
        .recv()
        .unwrap_or_else(|_| Err(ChatGptError::LoginFailed("вход прервался".into())));
    let _ = pending.thread.join();
    let mut cancel = CANCEL.lock().unwrap_or_else(|e| e.into_inner());
    if cancel
        .as_ref()
        .is_some_and(|c| Arc::ptr_eq(c, &pending.cancel))
    {
        *cancel = None;
    }
    drop(cancel);
    match &result {
        Ok(_) => log::info!("[ChatGPT] signed in"),
        Err(e) => log::warn!("[ChatGPT] sign-in ended: {}", e.code()),
    }
    result.map(|_| status())
}

pub fn cancel_login() {
    if let Some(cancel) = CANCEL.lock().unwrap_or_else(|e| e.into_inner()).take() {
        cancel.notify_one();
    }
}

/// «Проверить»: half a second of silence through the stored sign-in.
pub fn check() -> Result<(), ChatGptError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ChatGptError::Protocol(e.to_string()))?;
    runtime
        .block_on(chatgpt::check_with_audio(
            store(),
            ChatGptEndpoints::default(),
        ))
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct XorVault;

    impl Vault for XorVault {
        fn seal(&self, plain: &[u8]) -> Result<Vec<u8>, String> {
            Ok(plain.iter().map(|b| b ^ 0x5A).collect())
        }
        fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, String> {
            Ok(sealed.iter().map(|b| b ^ 0x5A).collect())
        }
    }

    fn tokens() -> ChatGptTokens {
        ChatGptTokens {
            id_token: "id.secret-id".into(),
            access_token: "secret-access".into(),
            refresh_token: "secret-refresh".into(),
            account_id: "acc".into(),
            last_refresh: 1,
        }
    }

    #[test]
    fn sealed_file_round_trips_and_holds_no_plaintext() {
        let dir = std::env::temp_dir().join(format!("vm-chatgpt-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chatgpt-auth.bin");
        let store = SealedFileStore::at(path.clone(), Box::new(XorVault));
        assert_eq!(store.load().unwrap(), None);
        store.save(&tokens()).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("secret"));
        assert_eq!(store.load().unwrap(), Some(tokens()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        store.clear().unwrap();
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_login_can_be_cancelled_and_frees_its_port() {
        let endpoints = ChatGptEndpoints {
            transcribe_url: "http://127.0.0.1:9/".into(),
            auth: velikiy_moguchiy_asr::chatgpt::auth::AuthEndpoints {
                issuer: "http://127.0.0.1:9".into(),
            },
        };
        let url = start_login_with(endpoints.clone(), 0).unwrap();
        assert!(url.contains("code_challenge="));
        cancel_login();
        assert_eq!(wait_login(), Err(ChatGptError::LoginCancelled));
        assert_eq!(wait_login(), Err(ChatGptError::LoginCancelled));
    }
}
