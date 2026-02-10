use crate::error::{AppError, AppResult};
use crate::state::{get_app_data_dir, Settings};
use chrono::{DateTime, Utc};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_modpkg::Modpkg;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use uuid::Uuid;

/// A mod profile for organizing different mod configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Unique identifier (UUID)
    pub id: String,
    /// User-friendly name
    pub name: String,
    /// List of mod IDs enabled in this profile
    pub enabled_mods: Vec<String>,
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
    pub file_path: String,
    pub layers: Vec<ModLayer>,
    /// Path to thumbnail image if exists (thumbnail.webp in mod dir)
    pub thumbnail_path: Option<String>,
    /// Directory where the mod is installed
    pub mod_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIndex {
    /// Installed mods (shared across all profiles)
    mods: Vec<LibraryModEntry>,
    /// All profiles
    profiles: Vec<Profile>,
    /// Currently active profile ID
    active_profile_id: String,
}

impl Default for LibraryIndex {
    fn default() -> Self {
        let default_profile = Profile {
            id: Uuid::new_v4().to_string(),
            name: "Default".to_string(),
            enabled_mods: Vec::new(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryModEntry {
    id: String,
    installed_at: DateTime<Utc>,
    /// Original file path the mod was installed from.
    file_path: String,
    /// Directory containing the installed mod project (mod.config.json, content/, etc).
    mod_dir: PathBuf,
    /// Path to the stored mod archive file (for overlay building).
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_path: Option<PathBuf>,
}

/// Information returned by `inspect_modpkg`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpkgInfo {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub layers: Vec<LayerInfo>,
    pub file_count: u64,
    pub total_size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    pub name: String,
    pub priority: i32,
    pub description: Option<String>,
    pub file_count: u64,
}

pub fn get_installed_mods(
    app_handle: &AppHandle,
    settings: &Settings,
) -> AppResult<Vec<InstalledMod>> {
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    // Get active profile to check enabled mods
    let active_profile_id = index.active_profile_id.clone();
    let active_profile = index
        .profiles
        .iter()
        .find(|p| p.id == active_profile_id)
        .ok_or_else(|| AppError::Other("Active profile not found".to_string()))?;

    // Deterministic ordering: by installed_at then id.
    index
        .mods
        .sort_by(|a, b| a.installed_at.cmp(&b.installed_at).then(a.id.cmp(&b.id)));

    let mut result = Vec::new();
    for entry in &index.mods {
        let enabled = active_profile.enabled_mods.contains(&entry.id);
        match read_installed_mod(entry, enabled) {
            Ok(m) => result.push(m),
            Err(e) => {
                tracing::warn!("Skipping broken mod entry {}: {}", entry.id, e);
            }
        }
    }

    Ok(result)
}

pub fn install_mod_from_package(
    app_handle: &AppHandle,
    settings: &Settings,
    file_path: &str,
) -> AppResult<InstalledMod> {
    let file_path = PathBuf::from(file_path);
    if !file_path.exists() {
        return Err(AppError::InvalidPath(file_path.display().to_string()));
    }

    let (storage_dir, _mods_dir) = resolve_storage_dirs(app_handle, settings)?;

    // Create archives and metadata directories
    let archives_dir = storage_dir.join("archives");
    let metadata_dir = storage_dir.join("metadata");
    fs::create_dir_all(&archives_dir)?;
    fs::create_dir_all(&metadata_dir)?;

    let id = Uuid::new_v4().to_string();

    let extension = file_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let installed_at = Utc::now();

    // Copy archive to archives directory
    let archive_filename = format!("{}.{}", id, extension);
    let archive_path = archives_dir.join(&archive_filename);
    fs::copy(&file_path, &archive_path)?;
    tracing::info!(
        "Copied mod archive from {} to {}",
        file_path.display(),
        archive_path.display()
    );

    // Extract only metadata to metadata directory
    let mod_metadata_dir = metadata_dir.join(&id);
    fs::create_dir_all(&mod_metadata_dir)?;

    if extension == "fantome" {
        extract_fantome_metadata(&archive_path, &mod_metadata_dir)?;
    } else {
        // Default: treat as modpkg
        extract_modpkg_metadata(&archive_path, &mod_metadata_dir)?;
    }

    // Add to index and persist.
    let mut index = load_library_index(&storage_dir)?;

    // Add mod entry to shared mods list
    let entry = LibraryModEntry {
        id: id.clone(),
        installed_at,
        file_path: file_path.display().to_string(),
        mod_dir: mod_metadata_dir.clone(),
        archive_path: Some(archive_path.clone()),
    };
    index.mods.push(entry.clone());

    // Enable in active profile
    let active_profile_id = index.active_profile_id.clone();
    if let Some(profile) = index
        .profiles
        .iter_mut()
        .find(|p| p.id == active_profile_id)
    {
        profile.enabled_mods.push(id.clone());
    }

    save_library_index(&storage_dir, &index)?;

    // Return materialized InstalledMod (enabled = true since we just enabled it)
    read_installed_mod(&entry, true)
}

pub fn toggle_mod_enabled(
    app_handle: &AppHandle,
    settings: &Settings,
    mod_id: &str,
    enabled: bool,
) -> AppResult<()> {
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    // Validate mod exists
    if !index.mods.iter().any(|m| m.id == mod_id) {
        return Err(AppError::ModNotFound(mod_id.to_string()));
    }

    // Update active profile's enabled mods
    let active_profile_id = index.active_profile_id.clone();
    let profile = index
        .profiles
        .iter_mut()
        .find(|p| p.id == active_profile_id)
        .ok_or_else(|| AppError::Other("Active profile not found".to_string()))?;

    if enabled {
        if !profile.enabled_mods.contains(&mod_id.to_string()) {
            profile.enabled_mods.push(mod_id.to_string());
        }
    } else {
        profile.enabled_mods.retain(|id| id != mod_id);
    }

    save_library_index(&storage_dir, &index)?;
    Ok(())
}

pub fn uninstall_mod_by_id(
    app_handle: &AppHandle,
    settings: &Settings,
    mod_id: &str,
) -> AppResult<()> {
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    let Some(pos) = index.mods.iter().position(|m| m.id == mod_id) else {
        return Err(AppError::ModNotFound(mod_id.to_string()));
    };

    let entry = index.mods.remove(pos);

    // Delete metadata directory
    if entry.mod_dir.exists() {
        fs::remove_dir_all(&entry.mod_dir)?;
    }

    // Delete archive file
    if let Some(archive_path) = &entry.archive_path {
        if archive_path.exists() {
            fs::remove_file(archive_path)?;
            tracing::info!("Deleted mod archive at {}", archive_path.display());
        }
    }

    save_library_index(&storage_dir, &index)?;
    Ok(())
}

pub fn inspect_modpkg_file(file_path: &str) -> AppResult<ModpkgInfo> {
    let file_path = Path::new(file_path);
    let file = std::fs::File::open(file_path)?;
    let mut modpkg =
        Modpkg::mount_from_reader(file).map_err(|e| AppError::Modpkg(e.to_string()))?;

    let metadata = modpkg
        .load_metadata()
        .map_err(|e| AppError::Modpkg(e.to_string()))?;

    let authors = metadata
        .authors
        .iter()
        .map(|a| a.name.clone())
        .collect::<Vec<_>>();
    let mut file_count: u64 = 0;
    let mut total_size: u64 = 0;

    // Count content chunks (exclude meta folder paths for file_count/size)
    for ((path_hash, _layer_hash), chunk) in &modpkg.chunks {
        let path = modpkg
            .chunk_paths
            .get(path_hash)
            .map(String::as_str)
            .unwrap_or("");
        if path.starts_with("_meta_/") {
            continue;
        }
        file_count += 1;
        total_size += chunk.uncompressed_size;
    }

    // Layer counts: derive from header layer list.
    let mut layer_counts: BTreeMap<String, u64> = BTreeMap::new();
    for (path_hash, layer_hash) in modpkg.chunks.keys() {
        let path = modpkg
            .chunk_paths
            .get(path_hash)
            .map(String::as_str)
            .unwrap_or("");
        if path.starts_with("_meta_/") {
            continue;
        }
        if let Some(layer) = modpkg.layers.get(layer_hash) {
            *layer_counts.entry(layer.name.clone()).or_insert(0) += 1;
        }
    }

    let mut layers = Vec::new();
    for layer in modpkg.layers.values() {
        let count = layer_counts.get(&layer.name).copied().unwrap_or(0);
        let desc = metadata
            .layers
            .iter()
            .find(|l| l.name == layer.name)
            .and_then(|l| l.description.clone());
        layers.push(LayerInfo {
            name: layer.name.clone(),
            priority: layer.priority,
            description: desc,
            file_count: count,
        });
    }
    layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

    Ok(ModpkgInfo {
        name: metadata.name,
        display_name: metadata.display_name,
        version: metadata.version.to_string(),
        description: metadata.description,
        authors,
        layers,
        file_count,
        total_size,
    })
}

pub(crate) fn resolve_storage_dirs(
    app_handle: &AppHandle,
    settings: &Settings,
) -> AppResult<(PathBuf, PathBuf)> {
    let storage_dir = settings
        .mod_storage_path
        .clone()
        .or_else(|| get_app_data_dir(app_handle).map(|d| d.join("mods")))
        .ok_or_else(|| AppError::Other("Failed to resolve mod storage directory".to_string()))?;

    let mods_dir = storage_dir.join("mods");
    Ok((storage_dir, mods_dir))
}

fn library_index_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join("library.json")
}

fn load_library_index(storage_dir: &Path) -> AppResult<LibraryIndex> {
    fs::create_dir_all(storage_dir)?;
    let path = library_index_path(storage_dir);
    if !path.exists() {
        return Ok(LibraryIndex::default());
    }
    let contents = fs::read_to_string(path)?;
    serde_json::from_str(&contents).map_err(AppError::from)
}

fn save_library_index(storage_dir: &Path, index: &LibraryIndex) -> AppResult<()> {
    fs::create_dir_all(storage_dir)?;
    let path = library_index_path(storage_dir);
    let contents = serde_json::to_string_pretty(index)?;
    fs::write(path, contents)?;
    Ok(())
}

// ============================================================================
// Profile Helper Functions
// ============================================================================

fn get_active_profile(index: &LibraryIndex) -> AppResult<&Profile> {
    index
        .profiles
        .iter()
        .find(|p| p.id == index.active_profile_id)
        .ok_or_else(|| AppError::Other("Active profile not found".to_string()))
}

fn get_profile_by_id<'a>(index: &'a LibraryIndex, profile_id: &str) -> AppResult<&'a Profile> {
    index
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| AppError::Other(format!("Profile {} not found", profile_id)))
}

fn resolve_profile_dirs(storage_dir: &Path, profile_id: &str) -> (PathBuf, PathBuf) {
    let profile_dir = storage_dir.join("profiles").join(profile_id);
    let overlay_dir = profile_dir.join("overlay");
    let cache_dir = profile_dir.join("cache");
    (overlay_dir, cache_dir)
}

// ============================================================================
// Public Profile Management Functions
// ============================================================================

/// Create a new profile.
pub fn create_profile(
    app_handle: &AppHandle,
    settings: &Settings,
    name: String,
) -> AppResult<Profile> {
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
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

    let profile = Profile {
        id: Uuid::new_v4().to_string(),
        name,
        enabled_mods: Vec::new(),
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
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
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
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
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
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
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
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
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
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
    let index = load_library_index(&storage_dir)?;
    let profile = get_active_profile(&index)?;
    Ok(profile.clone())
}

// ============================================================================
// Overlay Functions
// ============================================================================

pub(crate) fn get_enabled_mods_for_overlay(
    app_handle: &AppHandle,
    settings: &Settings,
) -> AppResult<(String, Vec<ltk_overlay::EnabledMod>)> {
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
    let index = load_library_index(&storage_dir)?;

    // Get active profile
    let active_profile_id = index.active_profile_id.clone();
    let active_profile = index
        .profiles
        .iter()
        .find(|p| p.id == active_profile_id)
        .ok_or_else(|| AppError::Other("Active profile not found".to_string()))?;

    let mut enabled_mods = Vec::new();

    // Process mods in the order they appear in enabled_mods list (maintains priority)
    for mod_id in &active_profile.enabled_mods {
        let Some(entry) = index.mods.iter().find(|m| &m.id == mod_id) else {
            tracing::warn!("Mod {} in profile but not found in library", mod_id);
            continue;
        };

        let Some(archive_path) = &entry.archive_path else {
            tracing::warn!("Mod {} has no archive path, skipping", entry.id);
            continue;
        };

        if !archive_path.exists() {
            tracing::warn!(
                "Archive not found for mod {}: {}",
                entry.id,
                archive_path.display()
            );
            continue;
        }

        let extension = archive_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        tracing::info!(
            "Creating content provider for mod {} from archive {}",
            entry.id,
            archive_path.display()
        );

        let content: Box<dyn ltk_overlay::ModContentProvider> = if extension == "fantome" {
            let file = std::fs::File::open(archive_path)?;
            let provider = crate::overlay::fantome_content::FantomeContent::new(file)
                .map_err(|e| AppError::Other(format!("Failed to open fantome archive: {}", e)))?;
            Box::new(provider)
        } else {
            let file = std::fs::File::open(archive_path)?;
            let modpkg = ltk_modpkg::Modpkg::mount_from_reader(file)
                .map_err(|e| AppError::Modpkg(e.to_string()))?;
            Box::new(crate::overlay::modpkg_content::ModpkgContent::new(modpkg))
        };

        enabled_mods.push(ltk_overlay::EnabledMod {
            id: entry.id.clone(),
            content,
            priority: 0,
        });
    }

    Ok((active_profile_id, enabled_mods))
}

fn read_installed_mod(entry: &LibraryModEntry, enabled: bool) -> AppResult<InstalledMod> {
    let project = load_mod_project(&entry.mod_dir)?;
    let authors = project
        .authors
        .iter()
        .map(|a| match a {
            ltk_mod_project::ModProjectAuthor::Name(name) => name.clone(),
            ltk_mod_project::ModProjectAuthor::Role { name, role: _ } => name.clone(),
        })
        .collect::<Vec<_>>();

    let layers = project
        .layers
        .iter()
        .map(|l| ModLayer {
            name: l.name.clone(),
            priority: l.priority,
            enabled: true,
        })
        .collect::<Vec<_>>();

    // Check for thumbnail.webp
    let thumbnail_path = entry.mod_dir.join("thumbnail.webp");
    let thumbnail_path = if thumbnail_path.exists() {
        Some(thumbnail_path.display().to_string())
    } else {
        None
    };

    Ok(InstalledMod {
        id: entry.id.clone(),
        name: project.name,
        display_name: project.display_name,
        version: project.version,
        description: Some(project.description).filter(|s| !s.is_empty()),
        authors,
        enabled,
        installed_at: entry.installed_at,
        file_path: entry.file_path.clone(),
        layers,
        thumbnail_path,
        mod_dir: entry.mod_dir.display().to_string(),
    })
}

fn load_mod_project(mod_dir: &Path) -> AppResult<ModProject> {
    let config_path = mod_dir.join("mod.config.json");
    let contents = fs::read_to_string(&config_path).map_err(|e| {
        AppError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read {}: {}", config_path.display(), e),
        ))
    })?;
    serde_json::from_str(&contents).map_err(AppError::from)
}

fn extract_fantome_metadata(file_path: &Path, metadata_dir: &Path) -> AppResult<()> {
    use std::io::Read;
    use zip::ZipArchive;

    let file = std::fs::File::open(file_path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| AppError::Other(format!("Failed to open fantome archive: {}", e)))?;

    // Read metadata from info.json
    let mut info_content = String::new();
    let mut found_metadata = false;

    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| AppError::Other(format!("Failed to read archive entry: {}", e)))?;
        let name = file.name().to_lowercase();

        if name == "meta/info.json" {
            drop(file);
            let mut info_file = archive
                .by_index(i)
                .map_err(|e| AppError::Other(format!("Failed to read info.json: {}", e)))?;
            info_file
                .read_to_string(&mut info_content)
                .map_err(|e| AppError::Other(format!("Failed to read info.json content: {}", e)))?;
            found_metadata = true;
            break;
        }
    }

    if !found_metadata {
        return Err(AppError::Other(
            "Missing META/info.json in fantome archive".to_string(),
        ));
    }

    // Parse metadata
    let info_content = info_content.trim_start_matches('\u{feff}').trim();
    let info: ltk_fantome::FantomeInfo = serde_json::from_str(info_content)
        .map_err(|e| AppError::Other(format!("Failed to parse info.json: {}", e)))?;

    // Build layers from Fantome info, preserving string overrides
    let layers = if info.layers.is_empty() {
        ltk_mod_project::default_layers()
    } else {
        let mut layers: Vec<ltk_mod_project::ModProjectLayer> = info
            .layers
            .into_values()
            .map(|layer_info| ltk_mod_project::ModProjectLayer {
                name: layer_info.name,
                priority: layer_info.priority,
                description: None,
                string_overrides: layer_info.string_overrides,
            })
            .collect();
        // Ensure base layer exists
        if !layers.iter().any(|l| l.name == "base") {
            layers.insert(0, ltk_mod_project::ModProjectLayer::base());
        }
        layers.sort_by(|a, b| a.priority.cmp(&b.priority));
        layers
    };

    // Create mod.config.json from metadata
    let project = ModProject {
        name: slug::slugify(&info.name),
        display_name: info.name,
        version: info.version,
        description: info.description,
        authors: vec![ltk_mod_project::ModProjectAuthor::Name(info.author)],
        license: None,
        transformers: Vec::new(),
        layers,
        thumbnail: None,
    };

    let config_path = metadata_dir.join("mod.config.json");
    fs::write(config_path, serde_json::to_string_pretty(&project)?)?;

    // Extract README.md and thumbnail if present
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| AppError::Other(format!("Failed to read archive entry: {}", e)))?;
        let name = file.name().to_string();
        let name_lower = name.to_lowercase();

        // Extract README
        if name.eq_ignore_ascii_case("META/readme.md") || name.eq_ignore_ascii_case("readme.md") {
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            fs::write(metadata_dir.join("README.md"), contents)?;
        }
        // Extract thumbnail - Fantome uses META/image.png
        else if name_lower == "meta/image.png" {
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;

            // Try to load as any image format and convert to WebP
            if let Ok(img) = image::load_from_memory(&buffer) {
                let webp_path = metadata_dir.join("thumbnail.webp");
                if let Err(e) = img.save_with_format(&webp_path, image::ImageFormat::WebP) {
                    tracing::warn!("Failed to save thumbnail: {}", e);
                }
            }
        }
    }

    tracing::info!("Extracted fantome metadata to {}", metadata_dir.display());
    Ok(())
}

fn extract_modpkg_metadata(file_path: &Path, metadata_dir: &Path) -> AppResult<()> {
    let file = std::fs::File::open(file_path)?;
    let mut modpkg =
        Modpkg::mount_from_reader(file).map_err(|e| AppError::Modpkg(e.to_string()))?;

    // Build a mod project config from metadata/header layers (no content extraction).
    let metadata = modpkg
        .load_metadata()
        .map_err(|e| AppError::Modpkg(e.to_string()))?;

    // Use header layers as source of truth, preserving string overrides from metadata.
    let mut layers: Vec<ModProjectLayer> = modpkg
        .layers
        .values()
        .map(|l| {
            let meta_layer = metadata.layers.iter().find(|ml| ml.name == l.name);
            ModProjectLayer {
                name: l.name.clone(),
                priority: l.priority,
                description: meta_layer.and_then(|ml| ml.description.clone()),
                string_overrides: meta_layer
                    .map(|ml| ml.string_overrides.clone())
                    .unwrap_or_default(),
            }
        })
        .collect();
    layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

    // Ensure base exists.
    if !layers.iter().any(|l| l.name == "base") {
        layers.insert(0, ModProjectLayer::base());
    }

    let project = ModProject {
        name: metadata.name,
        display_name: metadata.display_name,
        version: metadata.version.to_string(),
        description: metadata.description.unwrap_or_default(),
        authors: metadata
            .authors
            .into_iter()
            .map(|a| ltk_mod_project::ModProjectAuthor::Name(a.name))
            .collect(),
        license: None,
        transformers: Vec::new(),
        layers,
        thumbnail: None,
    };

    let config_path = metadata_dir.join("mod.config.json");
    fs::write(config_path, serde_json::to_string_pretty(&project)?)?;

    // Optional meta: README + thumbnail.webp
    if let Ok(readme_bytes) = modpkg.load_readme() {
        let _ = fs::write(metadata_dir.join("README.md"), readme_bytes);
    }
    if let Ok(thumbnail_bytes) = modpkg.load_thumbnail() {
        let _ = fs::write(metadata_dir.join("thumbnail.webp"), thumbnail_bytes);
    }

    tracing::info!("Extracted modpkg metadata to {}", metadata_dir.display());

    Ok(())
}
