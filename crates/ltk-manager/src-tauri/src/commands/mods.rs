use crate::error::{AppResult, IpcResult, MutexResultExt};
use crate::mods::{
    inspect_modpkg_file, install_mod_from_package, toggle_mod_enabled, uninstall_mod_by_id,
    InstalledMod, ModpkgInfo,
};
use crate::state::SettingsState;
use tauri::{AppHandle, State};

/// Get all installed mods from the mod library.
#[tauri::command]
pub fn get_installed_mods(
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<Vec<InstalledMod>> {
    get_installed_mods_inner(&app_handle, &settings).into()
}

fn get_installed_mods_inner(
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<Vec<InstalledMod>> {
    let settings = settings.0.lock().mutex_err()?.clone();

    crate::mods::get_installed_mods(app_handle, &settings)
}

/// Install a mod from a `.modpkg` or `.fantome` file into `modStoragePath`.
#[tauri::command]
pub fn install_mod(
    file_path: String,
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<InstalledMod> {
    install_mod_inner(file_path, &app_handle, &settings).into()
}

fn install_mod_inner(
    file_path: String,
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<InstalledMod> {
    let settings = settings.0.lock().mutex_err()?.clone();
    install_mod_from_package(app_handle, &settings, &file_path)
}

/// Uninstall a mod by id.
#[tauri::command]
pub fn uninstall_mod(
    mod_id: String,
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<()> {
    uninstall_mod_inner(mod_id, &app_handle, &settings).into()
}

fn uninstall_mod_inner(
    mod_id: String,
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<()> {
    let settings = settings.0.lock().mutex_err()?.clone();
    uninstall_mod_by_id(app_handle, &settings, &mod_id)
}

/// Toggle a mod's enabled state.
#[tauri::command]
pub fn toggle_mod(
    mod_id: String,
    enabled: bool,
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<()> {
    toggle_mod_inner(mod_id, enabled, &app_handle, &settings).into()
}

fn toggle_mod_inner(
    mod_id: String,
    enabled: bool,
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<()> {
    let settings = settings.0.lock().mutex_err()?.clone();
    toggle_mod_enabled(app_handle, &settings, &mod_id, enabled)
}

/// Inspect a `.modpkg` file and return its metadata.
#[tauri::command]
pub fn inspect_modpkg(file_path: String) -> IpcResult<ModpkgInfo> {
    inspect_modpkg_file(&file_path).into()
}

/// Get a mod's thumbnail as a base64 data URL.
#[tauri::command]
pub fn get_mod_thumbnail(thumbnail_path: String) -> IpcResult<String> {
    get_mod_thumbnail_inner(&thumbnail_path).into()
}

fn get_mod_thumbnail_inner(thumbnail_path: &str) -> crate::error::AppResult<String> {
    use base64::Engine;
    use std::fs;
    
    let path = std::path::Path::new(thumbnail_path);
    if !path.exists() {
        return Err(crate::error::AppError::InvalidPath(thumbnail_path.to_string()));
    }
    
    let data = fs::read(path)?;
    let base64_data = base64::engine::general_purpose::STANDARD.encode(&data);
    
    // Determine content type
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("webp");
    let content_type = match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "image/webp",
    };
    
    Ok(format!("data:{};base64,{}", content_type, base64_data))
}


