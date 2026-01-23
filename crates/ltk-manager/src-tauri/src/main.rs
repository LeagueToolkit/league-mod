#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use tauri::Manager;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;
mod error;
mod legacy_patcher;
mod mods;
mod overlay;
pub mod patcher;
mod state;
mod utils;

use patcher::PatcherState;
use state::SettingsState;

/// Perform first-run initialization:
/// - If league_path is not set, attempt auto-detection
/// - If auto-detection succeeds, save the path
fn initialize_first_run(app_handle: &tauri::AppHandle, settings_state: &SettingsState) {
    let mut settings = match settings_state.0.lock() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to lock settings: {}", e);
            return;
        }
    };

    // Skip if league path is already configured
    if settings.league_path.is_some() {
        tracing::info!("League path already configured, skipping auto-detection");
        return;
    }

    tracing::info!("Attempting auto-detection of League installation...");

    // Use ltk_mod_core for detection
    if let Some(exe_path) = ltk_mod_core::auto_detect_league_path() {
        let path = std::path::Path::new(exe_path.as_str());

        // Navigate from "Game/League of Legends.exe" to installation root
        if let Some(install_root) = path.parent().and_then(|p| p.parent()) {
            tracing::info!("Auto-detected League at: {:?}", install_root);
            settings.league_path = Some(install_root.to_path_buf());
            settings.first_run_complete = true;

            // Persist the detected path
            if let Err(e) = state::save_settings_to_disk(app_handle, &settings) {
                tracing::error!("Failed to save auto-detected settings: {}", e);
            }
        }
    } else {
        tracing::info!("Auto-detection did not find League installation");
    }
}

fn main() {
    // Initialize logging (stdout + persistent file).
    //
    // If you need more/less verbosity at runtime, set `RUST_LOG`, e.g.:
    // - `RUST_LOG=ltk_manager=trace,tauri=info`
    // - `RUST_LOG=ltk_manager=debug,tauri=warn`
    let (_file_guard, log_path) = init_logging();

    tracing::info!("Starting LTK Manager");
    if let Some(p) = log_path {
        tracing::info!("Backend log file: {}", p.display());
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let app_handle = app.handle();

            // Create individual states
            let settings_state = SettingsState::new(app_handle);
            let patcher_state = PatcherState::new();

            // Run first-run initialization (auto-detect League path)
            initialize_first_run(app_handle, &settings_state);

            // Manage each state separately
            app.manage(settings_state);
            app.manage(patcher_state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // App
            commands::get_app_info,
            // Settings
            commands::get_settings,
            commands::save_settings,
            commands::auto_detect_league_path,
            commands::validate_league_path,
            commands::check_setup_required,
            // Mods
            commands::get_installed_mods,
            commands::install_mod,
            commands::uninstall_mod,
            commands::toggle_mod,
            commands::inspect_modpkg,
            // Patcher
            commands::start_patcher,
            commands::stop_patcher,
            commands::get_patcher_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn init_logging() -> (Option<WorkerGuard>, Option<std::path::PathBuf>) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "ltk_manager=debug,tauri=info".into());

    // Default to stdout logs even if file setup fails.
    let stdout_layer = tracing_subscriber::fmt::layer();

    // Best-effort: create a persistent log file under app data dir.
    let (file_guard, file_layer, log_path) = match default_log_dir() {
        Some(log_dir) => {
            if let Err(e) = std::fs::create_dir_all(&log_dir) {
                eprintln!(
                    "Failed to create log directory {}: {}",
                    log_dir.display(),
                    e
                );
                (None, None, None)
            } else {
                let file_appender = tracing_appender::rolling::never(&log_dir, "ltk-manager.log");
                let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
                let layer = tracing_subscriber::fmt::layer().with_writer(non_blocking);
                (
                    Some(guard),
                    Some(layer),
                    Some(log_dir.join("ltk-manager.log")),
                )
            }
        }
        None => (None, None, None),
    };

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer);
    if let Some(layer) = file_layer {
        registry.with(layer).init();
    } else {
        registry.init();
    }

    (file_guard, log_path)
}

fn default_log_dir() -> Option<std::path::PathBuf> {
    // Match `tauri.conf.json` identifier so logs sit next to `settings.json` on Windows.
    const IDENTIFIER: &str = "dev.leaguetoolkit.manager";

    if let Ok(appdata) = std::env::var("APPDATA") {
        return Some(
            std::path::PathBuf::from(appdata)
                .join(IDENTIFIER)
                .join("logs"),
        );
    }

    // Best-effort fallback for non-Windows environments.
    if let Ok(home) = std::env::var("HOME") {
        return Some(
            std::path::PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(IDENTIFIER)
                .join("logs"),
        );
    }

    Some(std::env::temp_dir().join("ltk-manager"))
}
