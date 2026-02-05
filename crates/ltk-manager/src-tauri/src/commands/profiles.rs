use crate::error::{AppError, AppResult, IpcResult, MutexResultExt};
use crate::mods::{
    create_profile, delete_profile, get_active_profile_info, get_profiles, rename_profile,
    switch_profile, Profile,
};
use crate::patcher::PatcherState;
use crate::state::SettingsState;
use tauri::{AppHandle, State};

/// Get all profiles.
#[tauri::command]
pub fn list_mod_profiles(
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<Vec<Profile>> {
    list_mod_profiles_inner(&app_handle, &settings).into()
}

fn list_mod_profiles_inner(
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<Vec<Profile>> {
    let settings = settings.0.lock().mutex_err()?.clone();
    get_profiles(app_handle, &settings)
}

/// Get the currently active profile.
#[tauri::command]
pub fn get_active_mod_profile(
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<Profile> {
    get_active_mod_profile_inner(&app_handle, &settings).into()
}

fn get_active_mod_profile_inner(
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<Profile> {
    let settings = settings.0.lock().mutex_err()?.clone();
    get_active_profile_info(app_handle, &settings)
}

/// Create a new profile with the given name.
#[tauri::command]
pub fn create_mod_profile(
    name: String,
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<Profile> {
    create_mod_profile_inner(name, &app_handle, &settings).into()
}

fn create_mod_profile_inner(
    name: String,
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<Profile> {
    let settings = settings.0.lock().mutex_err()?.clone();
    create_profile(app_handle, &settings, name)
}

/// Delete a profile by ID.
#[tauri::command]
pub fn delete_mod_profile(
    profile_id: String,
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<()> {
    delete_mod_profile_inner(profile_id, &app_handle, &settings).into()
}

fn delete_mod_profile_inner(
    profile_id: String,
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<()> {
    let settings = settings.0.lock().mutex_err()?.clone();
    delete_profile(app_handle, &settings, profile_id)
}

/// Switch to a different profile.
/// Returns an error if the patcher is currently running.
#[tauri::command]
pub fn switch_mod_profile(
    profile_id: String,
    app_handle: AppHandle,
    settings: State<SettingsState>,
    patcher_state: State<PatcherState>,
) -> IpcResult<Profile> {
    switch_mod_profile_inner(profile_id, &app_handle, &settings, &patcher_state).into()
}

fn switch_mod_profile_inner(
    profile_id: String,
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
    patcher_state: &State<PatcherState>,
) -> AppResult<Profile> {
    // Check if patcher is running
    let patcher = patcher_state.0.lock().mutex_err()?;
    if patcher.is_running() {
        return Err(AppError::Other(
            "Cannot switch profiles while patcher is running. Please stop the patcher first."
                .to_string(),
        ));
    }
    drop(patcher);

    let settings = settings.0.lock().mutex_err()?.clone();
    switch_profile(app_handle, &settings, profile_id)
}

/// Rename a profile.
#[tauri::command]
pub fn rename_mod_profile(
    profile_id: String,
    new_name: String,
    app_handle: AppHandle,
    settings: State<SettingsState>,
) -> IpcResult<Profile> {
    rename_mod_profile_inner(profile_id, new_name, &app_handle, &settings).into()
}

fn rename_mod_profile_inner(
    profile_id: String,
    new_name: String,
    app_handle: &AppHandle,
    settings: &State<SettingsState>,
) -> AppResult<Profile> {
    let settings = settings.0.lock().mutex_err()?.clone();
    rename_profile(app_handle, &settings, profile_id, new_name)
}
