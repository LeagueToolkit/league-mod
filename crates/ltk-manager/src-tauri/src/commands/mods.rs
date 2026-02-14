use crate::error::{AppResult, IpcResult, MutexResultExt};
use crate::mods::{
    inspect_modpkg_file, install_mod_from_package, reorder_mods as reorder_mods_inner_fn,
    toggle_mod_enabled, uninstall_mod_by_id, InstalledMod, ModpkgInfo,
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

/// Reorder the enabled mods in the active profile.
#[tauri::command]
pub fn reorder_mods(
    mod_ids: Vec<String>,
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<()> {
    reorder_mods_command_inner(mod_ids, &app_handle, &settings).into()
}

fn reorder_mods_command_inner(
    mod_ids: Vec<String>,
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<()> {
    let settings = settings.0.lock().mutex_err()?.clone();
    reorder_mods_inner_fn(app_handle, &settings, mod_ids)
}

/// Inspect a `.modpkg` file and return its metadata.
#[tauri::command]
pub fn inspect_modpkg(file_path: String) -> IpcResult<ModpkgInfo> {
    inspect_modpkg_file(&file_path).into()
}

/// Get a mod's thumbnail as a base64 data URL, loaded on-the-fly from the archive.
/// Returns `null` if the mod has no thumbnail.
#[tauri::command]
pub fn get_mod_thumbnail(
    mod_id: String,
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<Option<String>> {
    get_mod_thumbnail_inner(&mod_id, &app_handle, &settings).into()
}

fn get_mod_thumbnail_inner(
    mod_id: &str,
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<Option<String>> {
    let settings = settings.0.lock().mutex_err()?.clone();
    crate::mods::get_mod_thumbnail_data(app_handle, &settings, mod_id)
}

/// Get the mod storage directory path.
#[tauri::command]
pub fn get_storage_directory(
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<String> {
    get_storage_directory_inner(&app_handle, &settings).into()
}

fn get_storage_directory_inner(
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<String> {
    let settings = settings.0.lock().mutex_err()?.clone();
    let storage_dir = crate::mods::resolve_storage_dir(app_handle, &settings)?;
    Ok(storage_dir.display().to_string())
}
