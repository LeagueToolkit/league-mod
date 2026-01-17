use crate::error::{AppError, AppResult, IpcResult};
use crate::mods::{
    inspect_modpkg_file, install_mod_from_package, toggle_mod_enabled, uninstall_mod_by_id,
    InstalledMod, ModpkgInfo,
};
use crate::state::SettingsState;
use tauri::{AppHandle, State};

/// Get all installed mods from the mod library.
#[tauri::command]
pub fn get_installed_mods(app_handle: AppHandle, settings: State<SettingsState>) -> IpcResult<Vec<InstalledMod>> {
    get_installed_mods_inner(&app_handle, &settings).into()
}

fn get_installed_mods_inner(app_handle: &AppHandle, settings: &State<SettingsState>) -> AppResult<Vec<InstalledMod>> {
    let settings = settings
        .0
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?
        .clone();

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
    let settings = settings
        .0
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?
        .clone();
    install_mod_from_package(app_handle, &settings, &file_path)
}

/// Uninstall a mod by id.
#[tauri::command]
pub fn uninstall_mod(mod_id: String, app_handle: AppHandle, settings: State<SettingsState>) -> IpcResult<()> {
    uninstall_mod_inner(mod_id, &app_handle, &settings).into()
}

fn uninstall_mod_inner(mod_id: String, app_handle: &AppHandle, settings: &State<SettingsState>) -> AppResult<()> {
    let settings = settings
        .0
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?
        .clone();
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
    let settings = settings
        .0
        .lock()
        .map_err(|e| AppError::InternalState(e.to_string()))?
        .clone();
    toggle_mod_enabled(app_handle, &settings, &mod_id, enabled)
}

/// Inspect a `.modpkg` file and return its metadata.
#[tauri::command]
pub fn inspect_modpkg(file_path: String) -> IpcResult<ModpkgInfo> {
    inspect_modpkg_file(&file_path).into()
}


