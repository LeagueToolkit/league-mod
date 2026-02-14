use crate::error::{AppError, AppResult};
use crate::state::Settings;
use chrono::Utc;
use std::fs;
use tauri::AppHandle;
use uuid::Uuid;

use super::{
    get_active_profile, get_profile_by_id, load_library_index, resolve_profile_dirs,
    resolve_storage_dir, save_library_index, Profile,
};

/// Create a new profile.
pub fn create_profile(
    app_handle: &AppHandle,
    settings: &Settings,
    name: String,
) -> AppResult<Profile> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    // Validate name is not empty or whitespace
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::Other("Profile name cannot be empty".to_string()));
    }

    // Validate unique name
    if index.profiles.iter().any(|p| p.name == name) {
        return Err(AppError::Other(format!(
            "Profile '{}' already exists",
            name
        )));
    }

    // New profiles get all installed mods in their display order
    let mod_order: Vec<String> = index.mods.iter().map(|m| m.id.clone()).collect();

    let profile = Profile {
        id: Uuid::new_v4().to_string(),
        name,
        enabled_mods: Vec::new(),
        mod_order,
        created_at: Utc::now(),
        last_used: Utc::now(),
    };

    // Create profile directories
    let (overlay_dir, cache_dir) = resolve_profile_dirs(&storage_dir, &profile.id);
    fs::create_dir_all(&overlay_dir)?;
    fs::create_dir_all(&cache_dir)?;

    index.profiles.push(profile.clone());
    save_library_index(&storage_dir, &index)?;

    tracing::info!("Created profile: {} (id={})", profile.name, profile.id);
    Ok(profile)
}

/// Delete a profile by ID.
pub fn delete_profile(
    app_handle: &AppHandle,
    settings: &Settings,
    profile_id: String,
) -> AppResult<()> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    // Validate profile exists and check constraints
    let profile = get_profile_by_id(&index, &profile_id)?;

    // Cannot delete Default profile
    if profile.name == "Default" {
        return Err(AppError::Other("Cannot delete Default profile".to_string()));
    }

    // Cannot delete active profile
    if profile_id == index.active_profile_id {
        return Err(AppError::Other(
            "Cannot delete active profile. Switch to another profile first.".to_string(),
        ));
    }

    // Remove from index
    index.profiles.retain(|p| p.id != profile_id);

    // Delete profile directories
    let profile_dir = storage_dir.join("profiles").join(&profile_id);
    if profile_dir.exists() {
        fs::remove_dir_all(&profile_dir)?;
        tracing::info!("Deleted profile directory: {}", profile_dir.display());
    }

    save_library_index(&storage_dir, &index)?;

    tracing::info!("Deleted profile: {}", profile_id);
    Ok(())
}

/// Switch to a different profile.
pub fn switch_profile(
    app_handle: &AppHandle,
    settings: &Settings,
    profile_id: String,
) -> AppResult<Profile> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    // Validate profile exists
    get_profile_by_id(&index, &profile_id)?;

    // Update active profile
    index.active_profile_id = profile_id.clone();

    // Update last_used timestamp
    if let Some(profile) = index.profiles.iter_mut().find(|p| p.id == profile_id) {
        profile.last_used = Utc::now();
        let result = profile.clone();
        save_library_index(&storage_dir, &index)?;

        tracing::info!("Switched to profile: {} (id={})", result.name, result.id);
        Ok(result)
    } else {
        Err(AppError::Other(
            "Profile not found after validation".to_string(),
        ))
    }
}

/// Get all profiles.
pub fn get_profiles(app_handle: &AppHandle, settings: &Settings) -> AppResult<Vec<Profile>> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let index = load_library_index(&storage_dir)?;
    Ok(index.profiles.clone())
}

/// Rename a profile.
pub fn rename_profile(
    app_handle: &AppHandle,
    settings: &Settings,
    profile_id: String,
    new_name: String,
) -> AppResult<Profile> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    // Validate name is not empty or whitespace
    let new_name = new_name.trim().to_string();
    if new_name.is_empty() {
        return Err(AppError::Other("Profile name cannot be empty".to_string()));
    }

    // Check for duplicate names
    if index
        .profiles
        .iter()
        .any(|p| p.id != profile_id && p.name == new_name)
    {
        return Err(AppError::Other(format!(
            "Profile '{}' already exists",
            new_name
        )));
    }

    // Cannot rename Default profile
    let profile = index
        .profiles
        .iter_mut()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| AppError::Other("Profile not found".to_string()))?;

    if profile.name == "Default" {
        return Err(AppError::Other("Cannot rename Default profile".to_string()));
    }

    profile.name = new_name;
    let result = profile.clone();
    save_library_index(&storage_dir, &index)?;

    tracing::info!("Renamed profile {} to: {}", profile_id, result.name);
    Ok(result)
}

/// Get the active profile.
pub fn get_active_profile_info(app_handle: &AppHandle, settings: &Settings) -> AppResult<Profile> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let index = load_library_index(&storage_dir)?;
    let profile = get_active_profile(&index)?;
    Ok(profile.clone())
}
