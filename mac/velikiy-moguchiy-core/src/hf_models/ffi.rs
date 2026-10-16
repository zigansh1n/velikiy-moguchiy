//! C interface for adding models from Hugging Face. Results are JSON
//! strings; network calls run on their own small runtime and report through
//! a callback, called exactly once on a background thread.

use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use futures_util::FutureExt;

use super::{repo, Inspection};

/// Receives the JSON result; the string is valid only during the call.
pub type HfDoneCallback = extern "C" fn(ctx: *mut c_void, json: *const c_char);

struct Callback {
    done: HfDoneCallback,
    ctx: *mut c_void,
}

// The app's callbacks hop to the main queue themselves.
unsafe impl Send for Callback {}
unsafe impl Sync for Callback {}

impl Callback {
    fn finish(&self, json: serde_json::Value) {
        if let Ok(text) = CString::new(json.to_string()) {
            (self.done)(self.ctx, text.as_ptr());
        }
    }
}

/// Inspections the window may still install from, newest last.
static INSPECTIONS: Mutex<VecDeque<(u64, Arc<Inspection>)>> = Mutex::new(VecDeque::new());
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
const KEPT_INSPECTIONS: usize = 8;

fn hf_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("hf-models")
            .enable_all()
            .build()
            .expect("hf models runtime")
    })
}

/// Runs `work` so that a panic inside it still answers the window, which
/// otherwise waits for the callback forever.
async fn answered<T>(
    work: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    std::panic::AssertUnwindSafe(work)
        .catch_unwind()
        .await
        .unwrap_or_else(|_| Err("Внутренняя ошибка ядра. Попробуйте ещё раз.".into()))
}

fn c_text(ptr: *const c_char) -> String {
    unsafe { crate::ffi::cstr_to_str(ptr) }
        .unwrap_or("")
        .to_string()
}

fn into_c_string(json: serde_json::Value) -> *mut c_char {
    CString::new(json.to_string())
        .unwrap_or_default()
        .into_raw()
}

/// Reads a repository (`owner/name` or a link) at `revision` (may be empty
/// or null) and reports `{"token", ...inspection}` or `{"error"}`. The token
/// names the result for [`sp_hf_install_start`].
///
/// # Safety
/// `input` and `revision` must be null or valid C strings; both are copied.
#[no_mangle]
pub unsafe extern "C" fn sp_hf_inspect_start(
    input: *const c_char,
    revision: *const c_char,
    done: HfDoneCallback,
    ctx: *mut c_void,
) {
    let input = c_text(input);
    let revision = c_text(revision);
    let callback = Callback { done, ctx };
    hf_runtime().spawn(async move {
        let result = answered(async {
            let repo = repo::parse_repo_input(&input, &revision)?;
            let client = super::http_client()?;
            super::inspect(&client, &super::endpoint(), &repo).await
        })
        .await;
        match result {
            Ok(inspection) => {
                let token = NEXT_TOKEN.fetch_add(1, Ordering::SeqCst);
                let mut json = serde_json::to_value(&inspection).unwrap_or_default();
                json["token"] = token.into();
                let mut kept = INSPECTIONS.lock().unwrap_or_else(|e| e.into_inner());
                kept.push_back((token, Arc::new(inspection)));
                while kept.len() > KEPT_INSPECTIONS {
                    kept.pop_front();
                }
                drop(kept);
                callback.finish(json);
            }
            Err(error) => callback.finish(serde_json::json!({ "error": error })),
        }
    });
}

/// Registers variant `variant_id` of inspection `token` as a user model and
/// reports `{"model_path"}` (for `sp_core_download_model`) or `{"error"}`.
///
/// # Safety
/// `variant_id` must be null or a valid C string; it is copied.
#[no_mangle]
pub unsafe extern "C" fn sp_hf_install_start(
    token: u64,
    variant_id: *const c_char,
    done: HfDoneCallback,
    ctx: *mut c_void,
) {
    let variant_id = c_text(variant_id);
    let callback = Callback { done, ctx };
    let inspection = INSPECTIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(t, _)| *t == token)
        .map(|(_, i)| i.clone());
    hf_runtime().spawn(async move {
        let result = answered(async {
            let inspection =
                inspection.ok_or("Сведения о репозитории устарели: проверьте его ещё раз.")?;
            let client = super::http_client()?;
            super::prepare_install(
                &client,
                &super::endpoint(),
                &crate::model_manager::models_dir(),
                &inspection,
                &variant_id,
            )
            .await
        })
        .await;
        callback.finish(match result {
            Ok(path) => serde_json::json!({ "model_path": path }),
            Err(error) => serde_json::json!({ "error": error }),
        });
    });
}

/// Deletes a user model added from Hugging Face. Returns `{"freed": bytes}`
/// or `{"error"}`; free with `sp_core_free_string`.
///
/// # Safety
/// `model_path` must be null or a valid C string.
#[no_mangle]
pub unsafe extern "C" fn sp_hf_remove_model_json(model_path: *const c_char) -> *mut c_char {
    let path = c_text(model_path);
    if crate::model_download_in_progress(&path) {
        return into_c_string(serde_json::json!({
            "error": "Модель сейчас скачивается: остановите загрузку и удалите её снова."
        }));
    }
    into_c_string(
        match super::remove_user_model(&crate::model_manager::models_dir(), &path) {
            Ok(freed) => serde_json::json!({ "freed": freed }),
            Err(error) => serde_json::json!({ "error": error }),
        },
    )
}

/// Runs the 16 kHz mono WAV in `wav` through the local model at
/// `model_path` and returns `{"ok", "text", "seconds", "error"}`. Blocks for
/// as long as the model takes; free the result with `sp_core_free_string`.
///
/// # Safety
/// `model_path` must be a valid C string and `wav` must point to `wav_len`
/// readable bytes (or be null with `wav_len` 0).
#[no_mangle]
pub unsafe extern "C" fn sp_local_model_check_json(
    model_path: *const c_char,
    wav: *const u8,
    wav_len: usize,
) -> *mut c_char {
    let path = c_text(model_path);
    let bytes = if wav.is_null() || wav_len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(wav, wav_len) }
    };
    let outcome = match super::check::pcm_from_wav(bytes) {
        Ok(pcm) => super::check::check_model(&crate::config::resolve_model_dir(&path), &pcm),
        Err(error) => super::check::CheckOutcome {
            ok: false,
            text: String::new(),
            seconds: 0.0,
            error: Some(error),
        },
    };
    into_c_string(serde_json::to_value(&outcome).unwrap_or_default())
}

fn compiled() -> &'static [&'static str] {
    crate::model_manager::supported_providers()
}

fn home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::config::config_dir()
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default()
        })
}

fn spawn_answer<T: serde::Serialize>(
    callback: Callback,
    work: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
) {
    hf_runtime().spawn(async move {
        let json = match answered(work).await {
            Ok(value) => serde_json::to_value(value).unwrap_or_default(),
            Err(error) => serde_json::json!({ "error": error }),
        };
        callback.finish(json);
    });
}

/// Disk work off the runtime's threads, answered like the network calls.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| "Внутренняя ошибка ядра. Попробуйте ещё раз.".to_string())
}

/// «Рекомендуемые» with what this Mac has of each, as a JSON array. Free
/// with `sp_core_free_string`.
#[no_mangle]
pub extern "C" fn sp_models_recommended_json() -> *mut c_char {
    let states =
        super::catalog::recommended_states(&crate::model_manager::models_dir(), compiled());
    into_c_string(serde_json::to_value(states).unwrap_or_default())
}

/// Prepares recommended model `id` and reports `{"model_path"}` for
/// `sp_core_download_model`, or `{"error"}`.
///
/// # Safety
/// `id` must be null or a valid C string; it is copied.
#[no_mangle]
pub unsafe extern "C" fn sp_models_install_recommended_start(
    id: *const c_char,
    done: HfDoneCallback,
    ctx: *mut c_void,
) {
    let id = c_text(id);
    spawn_answer(Callback { done, ctx }, async move {
        let client = super::http_client()?;
        let models_root = crate::model_manager::models_dir();
        let path = super::catalog::install_recommended(
            &client,
            &super::endpoint(),
            &models_root,
            &id,
            compiled(),
        )
        .await?;
        Ok(serde_json::json!({ "model_path": path }))
    });
}

/// Searches Hugging Face for speech models and reports the results or
/// `{"error"}`. Repeating a query within minutes answers from memory.
///
/// # Safety
/// `query` must be null or a valid C string; it is copied.
#[no_mangle]
pub unsafe extern "C" fn sp_hf_search_start(
    query: *const c_char,
    russian_only: bool,
    done: HfDoneCallback,
    ctx: *mut c_void,
) {
    let query = c_text(query);
    spawn_answer(Callback { done, ctx }, async move {
        let client = super::http_client()?;
        super::search::search(
            &client,
            &super::endpoint(),
            &query,
            russian_only,
            compiled(),
        )
        .await
    });
}

/// Looks for speech models already on this Mac and reports `{"finds"}`.
/// Reads only; nothing found is moved or changed.
#[no_mangle]
pub extern "C" fn sp_models_scan_local_start(done: HfDoneCallback, ctx: *mut c_void) {
    spawn_answer(Callback { done, ctx }, async {
        let finds = blocking(|| {
            let roots = super::local::default_roots(&home_dir());
            super::local::scan(&roots, &crate::model_manager::models_dir(), compiled())
        })
        .await?;
        Ok(serde_json::json!({ "finds": finds }))
    });
}

/// Reads the folder the user picked and reports `{"finds"}`: the models in
/// it, or one entry that says why there is none.
///
/// # Safety
/// `path` must be null or a valid C string; it is copied.
#[no_mangle]
pub unsafe extern "C" fn sp_models_inspect_folder_start(
    path: *const c_char,
    done: HfDoneCallback,
    ctx: *mut c_void,
) {
    let path = c_text(path);
    spawn_answer(Callback { done, ctx }, async move {
        let finds = blocking(move || {
            super::local::inspect_folder(
                std::path::Path::new(&path),
                &crate::model_manager::models_dir(),
                compiled(),
            )
        })
        .await?;
        Ok(serde_json::json!({ "finds": finds }))
    });
}

/// Adds variant `variant_id` of the model in folder `location` to the list
/// without copying it. Returns `{"model_path"}` or `{"error"}`; free with
/// `sp_core_free_string`.
///
/// # Safety
/// `location` and `variant_id` must be null or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn sp_models_add_external_json(
    location: *const c_char,
    variant_id: *const c_char,
) -> *mut c_char {
    let location = c_text(location);
    let variant_id = c_text(variant_id);
    let result = super::local::add_external(
        &crate::model_manager::models_dir(),
        std::path::Path::new(&location),
        &variant_id,
        compiled(),
    );
    into_c_string(match result {
        Ok(path) => serde_json::json!({ "model_path": path }),
        Err(error) => serde_json::json!({ "error": error }),
    })
}

/// «Мои модели» as a JSON array, the model dictation uses first. Free with
/// `sp_core_free_string`.
#[no_mangle]
pub extern "C" fn sp_models_list_json() -> *mut c_char {
    let provider = crate::config::config_get("asr.provider").unwrap_or_default();
    let model = crate::config::config_get(&format!("asr.{provider}.model")).unwrap_or_default();
    let primary =
        (!provider.is_empty() && !model.is_empty()).then_some((provider.as_str(), model.as_str()));
    let list = super::library::my_models(&crate::model_manager::models_dir(), compiled(), primary);
    into_c_string(serde_json::to_value(list).unwrap_or_default())
}

/// Deletes a downloaded model, or takes an added one out of the list without
/// touching its folder. Returns `{"removal": "delete" | "unlist"}` or
/// `{"error"}`; free with `sp_core_free_string`.
///
/// # Safety
/// `model_path` must be null or a valid C string.
#[no_mangle]
pub unsafe extern "C" fn sp_models_remove_json(model_path: *const c_char) -> *mut c_char {
    let path = c_text(model_path);
    if path.is_empty() {
        return into_c_string(serde_json::json!({ "error": "Такой модели нет." }));
    }
    if crate::model_download_in_progress(&path) {
        return into_c_string(serde_json::json!({
            "error": "Модель сейчас скачивается: остановите загрузку и удалите её снова."
        }));
    }
    into_c_string(
        match super::library::remove(&crate::model_manager::models_dir(), &path) {
            Ok(removal) => serde_json::json!({ "removal": removal }),
            Err(error) => serde_json::json!({ "error": error }),
        },
    )
}
