use crate::error::{AppError, AppResult};
use crate::state::Settings;
use chrono::Utc;
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_modpkg::Modpkg;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use uuid::Uuid;

use super::{
    load_library_index, resolve_storage_dir, save_library_index, BulkInstallError,
    BulkInstallResult, InstallProgress, InstalledMod, LibraryIndex, LibraryModEntry,
    ModArchiveFormat, ModLayer,
};
use tauri::Emitter;

pub fn get_installed_mods(
    app_handle: &AppHandle,
    settings: &Settings,
) -> AppResult<Vec<InstalledMod>> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let index = load_library_index(&storage_dir)?;

    // Get active profile to check enabled mods
    let active_profile_id = index.active_profile_id.clone();
    let active_profile = index
        .profiles
        .iter()
        .find(|p| p.id == active_profile_id)
        .ok_or_else(|| AppError::Other("Active profile not found".to_string()))?;

    let enabled_set: std::collections::HashSet<&str> = active_profile
        .enabled_mods
        .iter()
        .map(|s| s.as_str())
        .collect();

    let mut result = Vec::new();
    for mod_id in &active_profile.mod_order {
        let Some(entry) = index.mods.iter().find(|m| &m.id == mod_id) else {
            continue;
        };
        let enabled = enabled_set.contains(mod_id.as_str());
        match read_installed_mod(entry, enabled, &storage_dir) {
            Ok(m) => result.push(m),
            Err(e) => {
                tracing::warn!("Skipping broken mod entry {}: {}", entry.id, e);
            }
        }
    }

    Ok(result)
}

/// Reorder all mods for the active profile.
/// The provided `mod_ids` must exactly match all installed mod IDs.
/// The `enabled_mods` order is derived from the new display order.
pub fn reorder_mods(
    app_handle: &AppHandle,
    settings: &Settings,
    mod_ids: Vec<String>,
) -> AppResult<()> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    let active_profile_id = index.active_profile_id.clone();
    let profile = index
        .profiles
        .iter_mut()
        .find(|p| p.id == active_profile_id)
        .ok_or_else(|| AppError::Other("Active profile not found".to_string()))?;

    // Validate that the provided IDs exactly match all installed mods
    let mut installed_sorted: Vec<&str> = index.mods.iter().map(|m| m.id.as_str()).collect();
    installed_sorted.sort();
    let mut new_sorted: Vec<&str> = mod_ids.iter().map(|s| s.as_str()).collect();
    new_sorted.sort();

    if installed_sorted != new_sorted {
        return Err(AppError::ValidationFailed(
            "Provided mod IDs do not match the installed mods".to_string(),
        ));
    }

    // Derive enabled_mods order from new display order
    let enabled_set: std::collections::HashSet<&str> =
        profile.enabled_mods.iter().map(|s| s.as_str()).collect();
    profile.enabled_mods = mod_ids
        .iter()
        .filter(|id| enabled_set.contains(id.as_str()))
        .cloned()
        .collect();

    profile.mod_order = mod_ids;
    save_library_index(&storage_dir, &index)?;

    if let Err(e) = crate::overlay::invalidate_overlay(app_handle, settings) {
        tracing::warn!("Failed to invalidate overlay after reordering mods: {}", e);
    }

    Ok(())
}

pub fn install_mod_from_package(
    app_handle: &AppHandle,
    settings: &Settings,
    file_path: &str,
) -> AppResult<InstalledMod> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;

    let mut index = load_library_index(&storage_dir)?;

    let (_entry, installed_mod) = install_single_mod_to_index(&storage_dir, &mut index, file_path)?;

    save_library_index(&storage_dir, &index)?;

    if let Err(e) = crate::overlay::invalidate_overlay(app_handle, settings) {
        tracing::warn!("Failed to invalidate overlay after installing mod: {}", e);
    }

    Ok(installed_mod)
}

/// Install multiple mods in a single batch operation.
///
/// Loads `library.json` once, installs each mod, saves once, and invalidates
/// the overlay once. Emits `"install-progress"` events per file.
pub fn install_mods_from_packages(
    app_handle: &AppHandle,
    settings: &Settings,
    file_paths: &[String],
) -> AppResult<BulkInstallResult> {
    if file_paths.is_empty() {
        return Ok(BulkInstallResult {
            installed: Vec::new(),
            failed: Vec::new(),
        });
    }

    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    let total = file_paths.len();
    let mut installed = Vec::new();
    let mut failed = Vec::new();

    for (i, file_path) in file_paths.iter().enumerate() {
        let file_name = Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(file_path)
            .to_string();

        let _ = app_handle.emit(
            "install-progress",
            InstallProgress {
                current: i + 1,
                total,
                current_file: file_name.clone(),
            },
        );

        match install_single_mod_to_index(&storage_dir, &mut index, file_path) {
            Ok((_entry, mod_info)) => {
                installed.push(mod_info);
            }
            Err(e) => {
                tracing::warn!("Failed to install {}: {}", file_path, e);
                failed.push(BulkInstallError {
                    file_path: file_path.clone(),
                    file_name,
                    message: e.to_string(),
                });
            }
        }
    }

    save_library_index(&storage_dir, &index)?;

    if let Err(e) = crate::overlay::invalidate_overlay(app_handle, settings) {
        tracing::warn!("Failed to invalidate overlay after bulk install: {}", e);
    }

    Ok(BulkInstallResult { installed, failed })
}

/// Core install logic for a single mod file.
///
/// Copies the archive, extracts metadata, and adds the mod to the index.
/// Does NOT load/save the index or invalidate the overlay.
fn install_single_mod_to_index(
    storage_dir: &Path,
    index: &mut LibraryIndex,
    file_path: &str,
) -> AppResult<(LibraryModEntry, InstalledMod)> {
    let file_path = PathBuf::from(file_path);
    if !file_path.exists() {
        return Err(AppError::InvalidPath(file_path.display().to_string()));
    }

    let archives_dir = storage_dir.join("archives");
    let metadata_dir = storage_dir.join("metadata");
    fs::create_dir_all(&archives_dir)?;
    fs::create_dir_all(&metadata_dir)?;

    let id = Uuid::new_v4().to_string();

    let format = match file_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("fantome") => ModArchiveFormat::Fantome,
        _ => ModArchiveFormat::Modpkg,
    };

    let installed_at = Utc::now();

    // Copy archive to archives directory
    let archive_filename = format!("{}.{}", id, format.extension());
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

    match format {
        ModArchiveFormat::Fantome => extract_fantome_metadata(&archive_path, &mod_metadata_dir)?,
        ModArchiveFormat::Modpkg => extract_modpkg_metadata(&archive_path, &mod_metadata_dir)?,
    }

    let entry = LibraryModEntry {
        id: id.clone(),
        installed_at,
        format,
    };
    index.mods.push(entry.clone());

    // Enable in active profile and add to display order
    let active_profile_id = index.active_profile_id.clone();
    if let Some(profile) = index
        .profiles
        .iter_mut()
        .find(|p| p.id == active_profile_id)
    {
        profile.enabled_mods.insert(0, id.clone());
        profile.mod_order.insert(0, id.clone());
    }

    let installed_mod = read_installed_mod(&entry, true, storage_dir)?;
    Ok((entry, installed_mod))
}

pub fn toggle_mod_enabled(
    app_handle: &AppHandle,
    settings: &Settings,
    mod_id: &str,
    enabled: bool,
) -> AppResult<()> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
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
            // Insert at position preserving relative order from mod_order
            let insert_pos =
                if let Some(order_pos) = profile.mod_order.iter().position(|id| id == mod_id) {
                    profile
                        .enabled_mods
                        .iter()
                        .position(|id| {
                            profile
                                .mod_order
                                .iter()
                                .position(|oid| oid == id)
                                .is_none_or(|p| p > order_pos)
                        })
                        .unwrap_or(profile.enabled_mods.len())
                } else {
                    0
                };
            profile.enabled_mods.insert(insert_pos, mod_id.to_string());
        }
    } else {
        profile.enabled_mods.retain(|id| id != mod_id);
    }

    save_library_index(&storage_dir, &index)?;

    if let Err(e) = crate::overlay::invalidate_overlay(app_handle, settings) {
        tracing::warn!("Failed to invalidate overlay after toggling mod: {}", e);
    }

    Ok(())
}

pub fn uninstall_mod_by_id(
    app_handle: &AppHandle,
    settings: &Settings,
    mod_id: &str,
) -> AppResult<()> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    let Some(pos) = index.mods.iter().position(|m| m.id == mod_id) else {
        return Err(AppError::ModNotFound(mod_id.to_string()));
    };

    let entry = index.mods.remove(pos);

    // Remove from all profiles' mod_order and enabled_mods
    for profile in &mut index.profiles {
        profile.mod_order.retain(|id| id != mod_id);
        profile.enabled_mods.retain(|id| id != mod_id);
    }

    // Delete metadata directory
    let metadata_dir = entry.metadata_dir(&storage_dir);
    if metadata_dir.exists() {
        fs::remove_dir_all(&metadata_dir)?;
    }

    // Delete archive file
    let archive_path = entry.archive_path(&storage_dir);
    if archive_path.exists() {
        fs::remove_file(&archive_path)?;
        tracing::info!("Deleted mod archive at {}", archive_path.display());
    }

    save_library_index(&storage_dir, &index)?;

    if let Err(e) = crate::overlay::invalidate_overlay(app_handle, settings) {
        tracing::warn!("Failed to invalidate overlay after uninstalling mod: {}", e);
    }

    Ok(())
}

/// Load a mod's thumbnail on-the-fly from its archive and return as a base64 data URL.
/// Returns `None` if the mod has no thumbnail.
pub fn get_mod_thumbnail_data(
    app_handle: &AppHandle,
    settings: &Settings,
    mod_id: &str,
) -> AppResult<Option<String>> {
    use base64::Engine;

    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let index = load_library_index(&storage_dir)?;

    let entry = index
        .mods
        .iter()
        .find(|m| m.id == mod_id)
        .ok_or_else(|| AppError::ModNotFound(mod_id.to_string()))?;

    let archive_path = entry.archive_path(&storage_dir);
    if !archive_path.exists() {
        return Err(AppError::InvalidPath(archive_path.display().to_string()));
    }

    if entry.format == ModArchiveFormat::Fantome {
        use std::io::Read;
        use zip::ZipArchive;

        let file = std::fs::File::open(&archive_path)?;
        let mut archive = ZipArchive::new(file)
            .map_err(|e| AppError::Other(format!("Failed to open fantome archive: {}", e)))?;

        for i in 0..archive.len() {
            let name = archive
                .by_index(i)
                .map_err(|e| AppError::Other(format!("Failed to read archive entry: {}", e)))?
                .name()
                .to_lowercase();

            if name == "meta/image.png" {
                let mut file = archive
                    .by_index(i)
                    .map_err(|e| AppError::Other(format!("Failed to read thumbnail: {}", e)))?;
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer)?;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&buffer);
                return Ok(Some(format!("data:image/png;base64,{}", b64)));
            }
        }

        Ok(None)
    } else {
        let file = std::fs::File::open(&archive_path)?;
        let mut modpkg =
            Modpkg::mount_from_reader(file).map_err(|e| AppError::Modpkg(e.to_string()))?;

        match modpkg.load_thumbnail() {
            Ok(thumbnail_bytes) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&thumbnail_bytes);
                Ok(Some(format!("data:image/webp;base64,{}", b64)))
            }
            Err(_) => Ok(None),
        }
    }
}

pub(crate) fn get_enabled_mods_for_overlay(
    app_handle: &AppHandle,
    settings: &Settings,
) -> AppResult<(String, Vec<ltk_overlay::EnabledMod>)> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
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

        let archive_path = entry.archive_path(&storage_dir);

        if !archive_path.exists() {
            tracing::warn!(
                "Archive not found for mod {}: {}",
                entry.id,
                archive_path.display()
            );
            continue;
        }

        tracing::info!(
            "Creating content provider for mod {} from archive {}",
            entry.id,
            archive_path.display()
        );

        let content: Box<dyn ltk_overlay::ModContentProvider> = match entry.format {
            ModArchiveFormat::Fantome => {
                let file = std::fs::File::open(&archive_path)?;
                let provider =
                    crate::overlay::fantome_content::FantomeContent::new(file).map_err(|e| {
                        AppError::Other(format!("Failed to open fantome archive: {}", e))
                    })?;
                Box::new(provider)
            }
            ModArchiveFormat::Modpkg => {
                let file = std::fs::File::open(&archive_path)?;
                let modpkg = ltk_modpkg::Modpkg::mount_from_reader(file)
                    .map_err(|e| AppError::Modpkg(e.to_string()))?;
                Box::new(crate::overlay::modpkg_content::ModpkgContent::new(modpkg))
            }
        };

        enabled_mods.push(ltk_overlay::EnabledMod {
            id: entry.id.clone(),
            content,
        });
    }

    Ok((active_profile_id, enabled_mods))
}

fn read_installed_mod(
    entry: &LibraryModEntry,
    enabled: bool,
    storage_dir: &Path,
) -> AppResult<InstalledMod> {
    let mod_dir = entry.metadata_dir(storage_dir);
    let project = load_mod_project(&mod_dir)?;
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

    Ok(InstalledMod {
        id: entry.id.clone(),
        name: project.name,
        display_name: project.display_name,
        version: project.version,
        description: Some(project.description).filter(|s| !s.is_empty()),
        authors,
        enabled,
        installed_at: entry.installed_at,
        layers,
        mod_dir: mod_dir.display().to_string(),
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
        layers.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.name.cmp(&b.name))
        });
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

    // Extract README if present
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| AppError::Other(format!("Failed to read archive entry: {}", e)))?;
        let name = file.name().to_string();

        if name.eq_ignore_ascii_case("META/readme.md") || name.eq_ignore_ascii_case("readme.md") {
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            fs::write(metadata_dir.join("README.md"), contents)?;
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

    // Optional meta: README only (thumbnails loaded on-the-fly from archive)
    if let Ok(readme_bytes) = modpkg.load_readme() {
        let _ = fs::write(metadata_dir.join("README.md"), readme_bytes);
    }

    tracing::info!("Extracted modpkg metadata to {}", metadata_dir.display());

    Ok(())
}
