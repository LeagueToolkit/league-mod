use crate::error::{AppError, AppResult, IpcResult};
use crate::patcher::api::{CSLogLevel, PatcherApi, PatcherError, PATCHER_DLL_NAME};
use crate::patcher::PatcherState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tauri::{AppHandle, Manager, State};

/// Default timeout for hook initialization (5 minutes in milliseconds).
const DEFAULT_HOOK_TIMEOUT_MS: u32 = 300_000;
/// Step interval for the hook loop (milliseconds).
const HOOK_STEP_MS: u32 = 100;

/// Configuration for starting the patcher.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatcherConfig {
    /// Path prefix for the patcher configuration (where overlay files are located).
    pub config_path: String,
    /// Optional log file path.
    pub log_file: Option<String>,
    /// Timeout in milliseconds for hook initialization. Defaults to 5 minutes.
    pub timeout_ms: Option<u32>,
}

/// Current status of the patcher.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatcherStatus {
    /// Whether the patcher is currently running.
    pub running: bool,
    /// The config path the patcher was started with.
    pub config_path: Option<String>,
}

#[derive(Debug, thiserror::Error)]
enum PatcherLoopError {
    #[error(transparent)]
    Patcher(#[from] PatcherError),
    #[error("Failed to begin hook")]
    HookFailed,
    #[error("Hook initialization timed out")]
    HookTimeout,
    #[error("Patcher stopped by request")]
    Stopped,
}

/// Resolve the path to the patcher DLL from bundled resources.
fn resolve_patcher_dll_path(app_handle: &AppHandle) -> AppResult<PathBuf> {
    let resource_path = app_handle
        .path()
        .resource_dir()
        .map_err(|e| AppError::Other(format!("Failed to get resource directory: {}", e)))?
        .join(PATCHER_DLL_NAME);

    if resource_path.exists() {
        return Ok(resource_path);
    }

    // Fallback for development: check next to executable
    let dev_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .map(|p| p.join(PATCHER_DLL_NAME));

    if let Some(path) = dev_path {
        if path.exists() {
            return Ok(path);
        }
    }

    Err(AppError::Other(format!(
        "Patcher DLL not found. Expected at: {}",
        resource_path.display()
    )))
}

/// Start the patcher with the given configuration.
///
/// The patcher runs in a background thread, continuously monitoring for the
/// League of Legends process and applying hooks when found.
#[tauri::command]
pub fn start_patcher(
    config: PatcherConfig,
    app_handle: AppHandle,
    state: State<PatcherState>,
) -> IpcResult<()> {
    start_patcher_inner(config, &app_handle, &state).into()
}

fn start_patcher_inner(
    config: PatcherConfig,
    app_handle: &AppHandle,
    state: &State<PatcherState>,
) -> AppResult<()> {
    let mut patcher_state = state
        .0
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?;

    if patcher_state.is_running() {
        return Err(AppError::Other("Patcher is already running".to_string()));
    }

    let dll_path = resolve_patcher_dll_path(app_handle)?;
    tracing::info!("Using patcher DLL: {}", dll_path.display());
    tracing::info!("Starting patcher with config path: {}", config.config_path);

    patcher_state.stop_flag.store(false, Ordering::SeqCst);
    let stop_flag = Arc::clone(&patcher_state.stop_flag);
    let config_path = config.config_path.clone();
    let log_file = config.log_file.clone();
    let timeout_ms = config.timeout_ms.unwrap_or(DEFAULT_HOOK_TIMEOUT_MS);

    let handle = thread::spawn(move || {
        match run_patcher_loop(&dll_path, &config_path, log_file.as_deref(), timeout_ms, &stop_flag)
        {
            Ok(()) => tracing::info!("Patcher loop completed successfully"),
            Err(PatcherLoopError::Stopped) => tracing::info!("Patcher stopped by request"),
            Err(e) => tracing::error!("Patcher loop error: {}", e),
        }
        tracing::info!("Patcher thread exiting");
    });

    patcher_state.thread_handle = Some(handle);
    patcher_state.config_path = Some(config.config_path);

    Ok(())
}

fn run_patcher_loop(
    dll_path: &PathBuf,
    config_path: &str,
    log_file: Option<&str>,
    timeout_ms: u32,
    stop_flag: &AtomicBool,
) -> Result<(), PatcherLoopError> {
    let api = PatcherApi::load(dll_path)?;

    api.init()?;
    api.set_config(config_path)?;
    api.set_log_level(CSLogLevel::Debug)?;

    if let Some(log_path) = log_file {
        api.set_log_file(log_path)?;
    }

    tracing::info!("Patcher initialized, waiting for League process...");

    let tid = loop {
        if stop_flag.load(Ordering::SeqCst) {
            return Err(PatcherLoopError::Stopped);
        }
        match api.find() {
            Some(tid) => break tid.get(),
            None => api.sleep(100),
        }
    };

    tracing::info!("Found League process, thread id: {}", tid);

    let count_before = api.hook_count();
    let hook = api.hook_begin(tid);
    if hook == 0 {
        return Err(PatcherLoopError::HookFailed);
    }

    let mut time_remaining = timeout_ms as i64;
    loop {
        if stop_flag.load(Ordering::SeqCst) {
            api.hook_end(tid, hook);
            return Err(PatcherLoopError::Stopped);
        }

        if time_remaining <= 0 {
            api.hook_end(tid, hook);
            return Err(PatcherLoopError::HookTimeout);
        }

        api.hook_continue(tid, hook);
        api.sleep(HOOK_STEP_MS);

        if api.hook_count() != count_before {
            tracing::info!("Hooks applied successfully");
            api.hook_end(tid, hook);
            break;
        }

        time_remaining -= HOOK_STEP_MS as i64;
    }

    tracing::info!("Hook session completed");
    Ok(())
}

/// Stop the running patcher.
#[tauri::command]
pub fn stop_patcher(state: State<PatcherState>) -> IpcResult<()> {
    stop_patcher_inner(&state).into()
}

fn stop_patcher_inner(state: &State<PatcherState>) -> AppResult<()> {
    let mut patcher_state = state
        .0
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?;

    if !patcher_state.is_running() {
        return Err(AppError::Other("Patcher is not running".to_string()));
    }

    tracing::info!("Stopping patcher...");

    patcher_state.stop_flag.store(true, Ordering::SeqCst);

    if let Some(handle) = patcher_state.thread_handle.take() {
        drop(patcher_state);

        match handle.join() {
            Ok(()) => tracing::info!("Patcher thread joined successfully"),
            Err(_) => tracing::error!("Patcher thread panicked"),
        }
    }

    let mut patcher_state = state
        .0
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?;
    patcher_state.config_path = None;

    Ok(())
}

/// Get the current status of the patcher.
#[tauri::command]
pub fn get_patcher_status(state: State<PatcherState>) -> IpcResult<PatcherStatus> {
    get_patcher_status_inner(&state).into()
}

fn get_patcher_status_inner(state: &State<PatcherState>) -> AppResult<PatcherStatus> {
    let patcher_state = state
        .0
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?;

    let running = patcher_state.is_running();

    Ok(PatcherStatus {
        running,
        config_path: if running {
            patcher_state.config_path.clone()
        } else {
            None
        },
    })
}
