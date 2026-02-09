use crate::error::{AppError, AppErrorResponse, AppResult, IpcResult, MutexResultExt};
use crate::legacy_patcher::api::PATCHER_DLL_NAME;
use crate::legacy_patcher::runner::{
    run_legacy_patcher_loop, LegacyPatcherLoopError, DEFAULT_HOOK_TIMEOUT_MS,
};
use crate::overlay;
use crate::patcher::{PatcherPhase, PatcherState};
use crate::state::SettingsState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use tauri::{AppHandle, Emitter, Manager, State};

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
    /// Current phase of the patcher lifecycle.
    pub phase: PatcherPhase,
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
/// Returns immediately after spawning a background thread that builds the overlay
/// and then runs the patcher loop. Progress is reported via events.
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
    // Lock briefly: check state, set phase, clone what we need for the thread
    let (stop_flag, state_arc) = {
        let mut patcher_state = state.0.lock().mutex_err()?;

        if patcher_state.is_running() {
            return Err(AppError::Other("Patcher is already running".to_string()));
        }

        patcher_state.stop_flag.store(false, Ordering::SeqCst);
        patcher_state.phase = PatcherPhase::Building;

        (Arc::clone(&patcher_state.stop_flag), Arc::clone(&state.0))
    };

    tracing::info!("Start patcher requested (legacy DLL mode)");

    // Resolve DLL path and snapshot settings — quick operations done on the calling thread
    let dll_path = resolve_patcher_dll_path(app_handle)?;
    tracing::info!("Using patcher DLL: {}", dll_path.display());

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

    let app_handle_clone = app_handle.clone();

    let handle = thread::spawn(move || {
        // Phase 1: Build overlay (the slow part)
        let overlay_root = match overlay::ensure_overlay(&app_handle_clone, &settings_snapshot) {
            Ok(root) => root,
            Err(e) => {
                tracing::error!(error = ?e, "Overlay build failed");
                let error_response: AppErrorResponse = e.into();
                let _ = app_handle_clone.emit("patcher-error", &error_response);
                if let Ok(mut s) = state_arc.lock() {
                    s.phase = PatcherPhase::Idle;
                }
                return;
            }
        };

        // Check stop flag between build and patcher loop
        if stop_flag.load(Ordering::SeqCst) {
            tracing::info!("Stop requested after overlay build, exiting");
            if let Ok(mut s) = state_arc.lock() {
                s.phase = PatcherPhase::Idle;
            }
            return;
        }

        tracing::info!("Using overlay root: {}", overlay_root.display());

        // Legacy patcher concatenates the prefix directly with filenames
        // like "DATA/FINAL/..." without adding a separator. Ensure trailing backslash.
        let mut overlay_root_str = overlay_root.display().to_string();
        if !overlay_root_str.ends_with('\\') && !overlay_root_str.ends_with('/') {
            overlay_root_str.push('\\');
        }

        // Phase 2: Run patcher loop
        {
            if let Ok(mut s) = state_arc.lock() {
                s.phase = PatcherPhase::Patching;
                s.config_path = Some(overlay_root_str.clone());
            }
        }

        match run_legacy_patcher_loop(
            &dll_path,
            &overlay_root_str,
            log_file.as_deref(),
            timeout_ms,
            flags,
            &stop_flag,
        ) {
            Ok(()) => tracing::info!("Patcher loop completed successfully"),
            Err(LegacyPatcherLoopError::Stopped) => tracing::info!("Patcher stopped by request"),
            Err(e) => tracing::error!("Patcher loop error: {}", e),
        }

        // Cleanup
        if let Ok(mut s) = state_arc.lock() {
            s.phase = PatcherPhase::Idle;
            s.config_path = None;
        }
        tracing::info!("Patcher thread exiting");
    });

    // Store thread handle
    let mut patcher_state = state.0.lock().mutex_err()?;
    patcher_state.thread_handle = Some(handle);

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
    patcher_state.phase = PatcherPhase::Idle;

    Ok(())
}

/// Get the current status of the patcher.
#[tauri::command]
pub fn get_patcher_status(state: State<PatcherState>) -> IpcResult<PatcherStatus> {
    get_patcher_status_inner(&state).into()
}

fn get_patcher_status_inner(state: &State<PatcherState>) -> AppResult<PatcherStatus> {
    let mut patcher_state = state.0.lock().mutex_err()?;

    let running = patcher_state.is_running();

    // Defensive reset: if the thread has died but phase wasn't reset (e.g. panic),
    // correct it so the UI doesn't get stuck.
    if !running && patcher_state.phase != PatcherPhase::Idle {
        tracing::warn!(
            "Patcher thread dead but phase was {:?}, resetting to Idle",
            patcher_state.phase
        );
        patcher_state.phase = PatcherPhase::Idle;
        patcher_state.config_path = None;
    }

    Ok(PatcherStatus {
        running,
        config_path: if running {
            patcher_state.config_path.clone()
        } else {
            None
        },
        phase: patcher_state.phase,
    })
}
