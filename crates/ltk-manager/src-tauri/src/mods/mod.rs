mod inspect;
mod library;
mod profiles;

pub use inspect::{inspect_modpkg_file, ModpkgInfo};
pub(crate) use library::get_enabled_mods_for_overlay;
pub use library::{
    get_installed_mods, get_mod_thumbnail_data, install_mod_from_package,
    install_mods_from_packages, reorder_mods, toggle_mod_enabled, uninstall_mod_by_id,
};
pub use profiles::{
    create_profile, delete_profile, get_active_profile_info, get_profiles, rename_profile,
    switch_profile,
};

use crate::error::{AppError, AppResult};
use crate::state::{get_app_data_dir, Settings};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use uuid::Uuid;

/// Slugified profile name used as the filesystem directory name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileSlug(pub String);

impl ProfileSlug {
    /// Create a slug from a profile name. Returns `None` if the name produces an empty slug.
    pub fn from_name(name: &str) -> Option<Self> {
        let s = slug::slugify(name);
        if s.is_empty() {
            None
        } else {
            Some(Self(s))
        }
    }

    /// Check whether this slug is unique among profiles in the index.
    pub fn is_unique_in(&self, index: &LibraryIndex, exclude_id: Option<&str>) -> bool {
        !index
            .profiles
            .iter()
            .any(|p| p.slug == *self && exclude_id.is_none_or(|id| p.id != id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProfileSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for ProfileSlug {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// A mod profile for organizing different mod configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Unique identifier (UUID)
    pub id: String,
    /// User-friendly name
    pub name: String,
    /// Slugified name used as the filesystem directory name
    #[serde(default)]
    pub slug: ProfileSlug,
    /// List of mod IDs enabled in this profile (maintains overlay priority order)
    pub enabled_mods: Vec<String>,
    /// Display order of all mods (enabled and disabled) in the UI
    pub mod_order: Vec<String>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last time this profile was used/switched to
    pub last_used: DateTime<Utc>,
}

/// A mod layer shown in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLayer {
    pub name: String,
    pub priority: i32,
    pub enabled: bool,
}

/// A mod entry shown in the UI Library.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub enabled: bool,
    pub installed_at: DateTime<Utc>,
    pub layers: Vec<ModLayer>,
    /// Directory where the mod is installed
    pub mod_dir: String,
}

/// Result of a bulk mod install operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkInstallResult {
    pub installed: Vec<InstalledMod>,
    pub failed: Vec<BulkInstallError>,
}

/// Error info for a single file that failed during bulk install.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkInstallError {
    pub file_path: String,
    pub file_name: String,
    pub message: String,
}

/// Progress event emitted per-file during bulk mod install.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub current: usize,
    pub total: usize,
    pub current_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryIndex {
    pub(super) mods: Vec<LibraryModEntry>,
    pub(super) profiles: Vec<Profile>,
    pub(crate) active_profile_id: String,
}

impl Default for LibraryIndex {
    fn default() -> Self {
        let default_profile = Profile {
            id: Uuid::new_v4().to_string(),
            name: "Default".to_string(),
            slug: ProfileSlug::from("default".to_string()),
            enabled_mods: Vec::new(),
            mod_order: Vec::new(),
            created_at: Utc::now(),
            last_used: Utc::now(),
        };
        let active_profile_id = default_profile.id.clone();

        Self {
            mods: Vec::new(),
            profiles: vec![default_profile],
            active_profile_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum ModArchiveFormat {
    Modpkg,
    Fantome,
}

impl ModArchiveFormat {
    /// File extension for this format.
    pub(super) fn extension(self) -> &'static str {
        match self {
            ModArchiveFormat::Modpkg => "modpkg",
            ModArchiveFormat::Fantome => "fantome",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LibraryModEntry {
    pub(super) id: String,
    pub(super) installed_at: DateTime<Utc>,
    pub(super) format: ModArchiveFormat,
}

impl LibraryModEntry {
    /// Directory containing extracted metadata (mod.config.json, thumbnail, etc).
    pub(super) fn metadata_dir(&self, storage_dir: &Path) -> PathBuf {
        storage_dir.join("metadata").join(&self.id)
    }

    /// Path to the stored mod archive file.
    pub(super) fn archive_path(&self, storage_dir: &Path) -> PathBuf {
        storage_dir
            .join("archives")
            .join(format!("{}.{}", self.id, self.format.extension()))
    }
}

pub(crate) fn resolve_storage_dir(
    app_handle: &AppHandle,
    settings: &Settings,
) -> AppResult<PathBuf> {
    settings
        .mod_storage_path
        .clone()
        .or_else(|| get_app_data_dir(app_handle))
        .ok_or_else(|| AppError::Other("Failed to resolve mod storage directory".to_string()))
}

pub(super) fn library_index_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join("library.json")
}

/// Load the library index from disk.
/// Creates the index file if it doesn't exist.
pub(crate) fn load_library_index(storage_dir: &Path) -> AppResult<LibraryIndex> {
    fs::create_dir_all(storage_dir)?;

    let path = library_index_path(storage_dir);
    if !path.exists() {
        return Ok(LibraryIndex::default());
    }

    serde_json::from_str(&fs::read_to_string(path)?).map_err(AppError::from)
}

pub(super) fn save_library_index(storage_dir: &Path, index: &LibraryIndex) -> AppResult<()> {
    fs::create_dir_all(storage_dir)?;
    let path = library_index_path(storage_dir);
    let contents = serde_json::to_string_pretty(index)?;
    fs::write(path, contents)?;
    Ok(())
}

pub(crate) fn get_active_profile(index: &LibraryIndex) -> AppResult<&Profile> {
    index
        .profiles
        .iter()
        .find(|p| p.id == index.active_profile_id)
        .ok_or_else(|| AppError::Other("Active profile not found".to_string()))
}

pub(super) fn get_profile_by_id<'a>(
    index: &'a LibraryIndex,
    profile_id: &str,
) -> AppResult<&'a Profile> {
    index
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| AppError::Other(format!("Profile {} not found", profile_id)))
}

pub(super) fn resolve_profile_dirs(
    storage_dir: &Path,
    profile_slug: &ProfileSlug,
) -> (PathBuf, PathBuf) {
    let profile_dir = storage_dir.join("profiles").join(profile_slug.as_str());
    let overlay_dir = profile_dir.join("overlay");
    let cache_dir = profile_dir.join("cache");
    (overlay_dir, cache_dir)
}
