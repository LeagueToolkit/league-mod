use crate::error::{AppError, AppResult, IpcResult, MutexResultExt};
use crate::legacy_patcher::api::PATCHER_DLL_NAME;
use crate::legacy_patcher::runner::{
    run_legacy_patcher_loop, LegacyPatcherLoopError, DEFAULT_HOOK_TIMEOUT_MS,
};
use crate::overlay;
use crate::patcher::PatcherState;
use crate::state::SettingsState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use tauri::{AppHandle, Manager, State};

/// Configuration for starting the patcher.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatcherConfig {
    /// Optional log file path.
    pub log_file: Option<String>,
    /// Timeout in milliseconds for hook initialization. Defaults to 5 minutes.
    pub timeout_ms: Option<u32>,
    /// Optional legacy patcher flags (matches `cslol_set_flags`).
    ///
    /// If not provided, defaults to 0 (equivalent to `--opts:none` in cslol-tools).
    pub flags: Option<u64>,
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

/// Resolve the path to the patcher DLL from bundled resources.
fn resolve_patcher_dll_path(app_handle: &AppHandle) -> AppResult<PathBuf> {
    let resource_path = app_handle
        .path()
        .resource_dir()
        .map_err(|e| AppError::Other(format!("Failed to get resource directory: {}", e)))?
        .join(PATCHER_DLL_NAME);

    if resource_path.exists() {
        tracing::info!(
            "Resolved patcher DLL from resource_dir: {}",
            resource_path.display()
        );
        return Ok(resource_path);
    }

    // Fallback for development: check next to executable
    let dev_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .map(|p| p.join(PATCHER_DLL_NAME));

    if let Some(ref path) = dev_path {
        if path.exists() {
            tracing::info!(
                "Resolved patcher DLL next to executable: {}",
                path.display()
            );
            return Ok(path.clone());
        }
    }

    // Fallback for `tauri dev`: use the checked-in resources folder from the crate.
    // (`resource_dir()` during dev often points at `target/debug/`, but resources may not be copied there.)
    let manifest_resource_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join(PATCHER_DLL_NAME);
    if manifest_resource_path.exists() {
        tracing::info!(
            "Resolved patcher DLL from CARGO_MANIFEST_DIR resources: {}",
            manifest_resource_path.display()
        );
        return Ok(manifest_resource_path);
    }

    Err(AppError::Other(format!(
        "Patcher DLL not found. Tried:\n - {}\n - {}\n - {}",
        resource_path.display(),
        dev_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unavailable>".to_string()),
        manifest_resource_path.display(),
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
    settings: State<SettingsState>,
) -> IpcResult<()> {
    let result = start_patcher_inner(config, &app_handle, &state, &settings);
    if let Err(ref e) = result {
        tracing::error!(error = ?e, "Start patcher failed");
    }
    result.into()
}

fn start_patcher_inner(
    config: PatcherConfig,
    app_handle: &AppHandle,
    state: &State<PatcherState>,
    settings: &State<SettingsState>,
) -> AppResult<()> {
    let mut patcher_state = state.0.lock().mutex_err()?;

    if patcher_state.is_running() {
        return Err(AppError::Other("Patcher is already running".to_string()));
    }

    tracing::info!("Start patcher requested (legacy DLL mode)");
    let dll_path = resolve_patcher_dll_path(app_handle)?;
    tracing::info!("Using patcher DLL: {}", dll_path.display());

    patcher_state.stop_flag.store(false, Ordering::SeqCst);
    let stop_flag = Arc::clone(&patcher_state.stop_flag);
    let log_file = config.log_file.clone();
    let timeout_ms = config.timeout_ms.unwrap_or(DEFAULT_HOOK_TIMEOUT_MS);
    let flags = config.flags.unwrap_or(0);
    let settings_snapshot = settings.0.lock().mutex_err()?.clone();

    tracing::info!(
        "Settings snapshot: league_path={} mod_storage_path={}",
        settings_snapshot
            .league_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unset>".to_string()),
        settings_snapshot
            .mod_storage_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unset>".to_string())
    );

    // Build/reuse overlay before starting the patcher thread.
    // The returned directory is used as the legacy patcher prefix, so paths like
    // `DATA/FINAL/.../*.wad.client` resolve to `<overlayRoot>/DATA/FINAL/.../*.wad.client`.
    let overlay_root = overlay::ensure_overlay(app_handle, &settings_snapshot)?;
    tracing::info!("Using overlay root: {}", overlay_root.display());

    // Legacy patcher (cslol-dll.dll) concatenates the prefix directly with filenames
    // like "DATA/FINAL/..." without adding a separator. Ensure trailing backslash.
    let mut overlay_root_str = overlay_root.display().to_string();
    if !overlay_root_str.ends_with('\\') && !overlay_root_str.ends_with('/') {
        overlay_root_str.push('\\');
    }
    let overlay_root_for_thread = overlay_root_str.clone();

    let handle = thread::spawn(move || {
        match run_legacy_patcher_loop(
            &dll_path,
            &overlay_root_for_thread,
            log_file.as_deref(),
            timeout_ms,
            flags,
            &stop_flag,
        ) {
            Ok(()) => tracing::info!("Patcher loop completed successfully"),
            Err(LegacyPatcherLoopError::Stopped) => tracing::info!("Patcher stopped by request"),
            Err(e) => tracing::error!("Patcher loop error: {}", e),
        }
        tracing::info!("Patcher thread exiting");
    });

    patcher_state.thread_handle = Some(handle);
    patcher_state.config_path = Some(overlay_root_str);

    Ok(())
}

/// Stop the running patcher.
#[tauri::command]
pub fn stop_patcher(state: State<PatcherState>) -> IpcResult<()> {
    stop_patcher_inner(&state).into()
}

fn stop_patcher_inner(state: &State<PatcherState>) -> AppResult<()> {
    let mut patcher_state = state.0.lock().mutex_err()?;

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

    let mut patcher_state = state.0.lock().mutex_err()?;
    patcher_state.config_path = None;

    Ok(())
}

/// Get the current status of the patcher.
#[tauri::command]
pub fn get_patcher_status(state: State<PatcherState>) -> IpcResult<PatcherStatus> {
    get_patcher_status_inner(&state).into()
}

fn get_patcher_status_inner(state: &State<PatcherState>) -> AppResult<PatcherStatus> {
    let patcher_state = state.0.lock().mutex_err()?;

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
