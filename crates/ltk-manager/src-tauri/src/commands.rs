use crate::error::{AppError, AppResult, IpcResult};
use crate::state::{AppState, InstalledMod, Settings};
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

/// Get basic app information
#[tauri::command]
pub fn get_app_info() -> IpcResult<AppInfo> {
    IpcResult::ok(AppInfo {
        name: "LTK Manager".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Get current settings
#[tauri::command]
pub fn get_settings(state: State<AppState>) -> IpcResult<Settings> {
    get_settings_inner(&state).into()
}

fn get_settings_inner(state: &State<AppState>) -> AppResult<Settings> {
    let settings = state
        .settings
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?;
    Ok(settings.clone())
}

/// Save settings
#[tauri::command]
pub fn save_settings(settings: Settings, state: State<AppState>) -> IpcResult<()> {
    save_settings_inner(settings, &state).into()
}

fn save_settings_inner(settings: Settings, state: &State<AppState>) -> AppResult<()> {
    let mut current = state
        .settings
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?;
    *current = settings;
    // TODO: Persist to disk
    Ok(())
}

/// Auto-detect League of Legends installation path
#[tauri::command]
pub fn auto_detect_league_path() -> IpcResult<Option<PathBuf>> {
    IpcResult::ok(auto_detect_league_path_inner())
}

fn auto_detect_league_path_inner() -> Option<PathBuf> {
    // Common installation paths on Windows
    let common_paths = [
        r"C:\Riot Games\League of Legends",
        r"D:\Riot Games\League of Legends",
        r"C:\Program Files\Riot Games\League of Legends",
        r"C:\Program Files (x86)\Riot Games\League of Legends",
    ];

    for path in common_paths {
        let path = PathBuf::from(path);
        let exe_path = path.join("Game").join("League of Legends.exe");
        if exe_path.exists() {
            tracing::info!("Found League installation at: {:?}", path);
            return Some(path);
        }
    }

    tracing::warn!("Could not auto-detect League installation");
    None
}

/// Validate a League installation path
#[tauri::command]
pub fn validate_league_path(path: PathBuf) -> IpcResult<bool> {
    let exe_path = path.join("Game").join("League of Legends.exe");
    IpcResult::ok(exe_path.exists())
}

/// Get list of installed mods
#[tauri::command]
pub fn get_installed_mods(state: State<AppState>) -> IpcResult<Vec<InstalledMod>> {
    get_installed_mods_inner(&state).into()
}

fn get_installed_mods_inner(state: &State<AppState>) -> AppResult<Vec<InstalledMod>> {
    let mods = state
        .installed_mods
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?;
    Ok(mods.clone())
}

/// Install a mod from a .modpkg file
#[tauri::command]
pub fn install_mod(file_path: PathBuf, state: State<AppState>) -> IpcResult<InstalledMod> {
    install_mod_inner(file_path, &state).into()
}

fn install_mod_inner(file_path: PathBuf, state: &State<AppState>) -> AppResult<InstalledMod> {
    tracing::info!("Installing mod from: {:?}", file_path);

    if !file_path.exists() {
        return Err(AppError::InvalidPath(file_path.display().to_string()));
    }

    // TODO: Use ltk_modpkg to read and install the mod
    // For now, return a placeholder
    let installed_mod = InstalledMod {
        id: uuid::Uuid::new_v4().to_string(),
        name: file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string(),
        display_name: file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown Mod")
            .to_string(),
        version: "1.0.0".to_string(),
        description: None,
        authors: vec![],
        enabled: true,
        installed_at: chrono::Utc::now().to_rfc3339(),
        file_path,
        layers: vec![],
    };

    let mut mods = state
        .installed_mods
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?;
    mods.push(installed_mod.clone());

    Ok(installed_mod)
}

/// Uninstall a mod
#[tauri::command]
pub fn uninstall_mod(mod_id: String, state: State<AppState>) -> IpcResult<()> {
    uninstall_mod_inner(mod_id, &state).into()
}

fn uninstall_mod_inner(mod_id: String, state: &State<AppState>) -> AppResult<()> {
    tracing::info!("Uninstalling mod: {}", mod_id);

    let mut mods = state
        .installed_mods
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?;

    let initial_len = mods.len();
    mods.retain(|m| m.id != mod_id);

    if mods.len() == initial_len {
        return Err(AppError::ModNotFound(mod_id));
    }

    Ok(())
}

/// Toggle a mod's enabled state
#[tauri::command]
pub fn toggle_mod(mod_id: String, enabled: bool, state: State<AppState>) -> IpcResult<()> {
    toggle_mod_inner(mod_id, enabled, &state).into()
}

fn toggle_mod_inner(mod_id: String, enabled: bool, state: &State<AppState>) -> AppResult<()> {
    tracing::info!("Toggling mod {} to enabled={}", mod_id, enabled);

    let mut mods = state
        .installed_mods
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?;

    let mod_entry = mods
        .iter_mut()
        .find(|m| m.id == mod_id)
        .ok_or(AppError::ModNotFound(mod_id))?;

    mod_entry.enabled = enabled;
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpkgInfo {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub layers: Vec<LayerInfo>,
    pub file_count: usize,
    pub total_size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    pub name: String,
    pub priority: i32,
    pub description: Option<String>,
    pub file_count: usize,
}

/// Inspect a .modpkg file without installing
#[tauri::command]
pub fn inspect_modpkg(file_path: PathBuf) -> IpcResult<ModpkgInfo> {
    inspect_modpkg_inner(file_path).into()
}

fn inspect_modpkg_inner(file_path: PathBuf) -> AppResult<ModpkgInfo> {
    tracing::info!("Inspecting modpkg: {:?}", file_path);

    if !file_path.exists() {
        return Err(AppError::InvalidPath(file_path.display().to_string()));
    }

    // TODO: Use ltk_modpkg to read the actual mod info
    // For now, return placeholder data
    Ok(ModpkgInfo {
        name: file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string(),
        display_name: "Sample Mod".to_string(),
        version: "1.0.0".to_string(),
        description: Some("A sample mod description".to_string()),
        authors: vec!["Unknown Author".to_string()],
        layers: vec![LayerInfo {
            name: "base".to_string(),
            priority: 0,
            description: Some("Base layer".to_string()),
            file_count: 0,
        }],
        file_count: 0,
        total_size: 0,
    })
}
