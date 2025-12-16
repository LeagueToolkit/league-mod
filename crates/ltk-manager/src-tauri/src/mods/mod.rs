use crate::error::{AppError, AppResult};
use crate::state::{get_app_data_dir, Settings};
use chrono::{DateTime, Utc};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_modpkg::{Modpkg, ModpkgExtractor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use uuid::Uuid;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIndex {
    version: u32,
    mods: Vec<LibraryModEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryModEntry {
    id: String,
    enabled: bool,
    installed_at: DateTime<Utc>,
    /// Original file path the mod was installed from.
    file_path: String,
    /// Directory containing the installed mod project (mod.config.json, content/, etc).
    mod_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct EnabledMod {
    pub id: String,
    pub mod_dir: PathBuf,
}

impl Default for LibraryIndex {
    fn default() -> Self {
        Self {
            version: 1,
            mods: Vec::new(),
        }
    }
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

pub fn get_installed_mods(app_handle: &AppHandle, settings: &Settings) -> AppResult<Vec<InstalledMod>> {
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    // Deterministic ordering: by installed_at then id.
    index.mods.sort_by(|a, b| a.installed_at.cmp(&b.installed_at).then(a.id.cmp(&b.id)));

    let mut result = Vec::new();
    for entry in &index.mods {
        match read_installed_mod(entry) {
            Ok(m) => result.push(m),
            Err(e) => {
                tracing::warn!("Skipping broken mod entry {}: {}", entry.id, e);
            }
        }
    }

    Ok(result)
}

pub fn install_mod_from_package(app_handle: &AppHandle, settings: &Settings, file_path: &str) -> AppResult<InstalledMod> {
    let file_path = PathBuf::from(file_path);
    if !file_path.exists() {
        return Err(AppError::InvalidPath(file_path.display().to_string()));
    }

    let (storage_dir, mods_dir) = resolve_storage_dirs(app_handle, settings)?;
    fs::create_dir_all(&mods_dir)?;

    let id = Uuid::new_v4().to_string();
    let mod_dir = mods_dir.join(&id);
    fs::create_dir_all(&mod_dir)?;

    let extension = file_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let installed_at = Utc::now();

    if extension == "fantome" {
        install_fantome_to_dir(&file_path, &mod_dir)?;
    } else {
        // Default: treat as modpkg (front-end currently filters modpkg; we’ll keep this forgiving).
        install_modpkg_to_dir(&file_path, &mod_dir)?;
    }

    // Add to index and persist.
    let mut index = load_library_index(&storage_dir)?;
    index.mods.push(LibraryModEntry {
        id: id.clone(),
        enabled: true,
        installed_at,
        file_path: file_path.display().to_string(),
        mod_dir: mod_dir.clone(),
    });
    save_library_index(&storage_dir, &index)?;

    // Return materialized InstalledMod
    read_installed_mod(&LibraryModEntry {
        id,
        enabled: true,
        installed_at,
        file_path: file_path.display().to_string(),
        mod_dir,
    })
}

pub fn toggle_mod_enabled(app_handle: &AppHandle, settings: &Settings, mod_id: &str, enabled: bool) -> AppResult<()> {
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    let Some(entry) = index.mods.iter_mut().find(|m| m.id == mod_id) else {
        return Err(AppError::ModNotFound(mod_id.to_string()));
    };
    entry.enabled = enabled;
    save_library_index(&storage_dir, &index)?;
    Ok(())
}

pub fn uninstall_mod_by_id(app_handle: &AppHandle, settings: &Settings, mod_id: &str) -> AppResult<()> {
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    let Some(pos) = index.mods.iter().position(|m| m.id == mod_id) else {
        return Err(AppError::ModNotFound(mod_id.to_string()));
    };

    let entry = index.mods.remove(pos);
    if entry.mod_dir.exists() {
        fs::remove_dir_all(&entry.mod_dir)?;
    }
    save_library_index(&storage_dir, &index)?;
    Ok(())
}

pub fn inspect_modpkg_file(file_path: &str) -> AppResult<ModpkgInfo> {
    let file_path = Path::new(file_path);
    let file = std::fs::File::open(file_path)?;
    let mut modpkg = Modpkg::mount_from_reader(file).map_err(|e| AppError::Modpkg(e.to_string()))?;

    let metadata = modpkg.load_metadata().map_err(|e| AppError::Modpkg(e.to_string()))?;

    let authors = metadata.authors.iter().map(|a| a.name.clone()).collect::<Vec<_>>();
    let mut file_count: u64 = 0;
    let mut total_size: u64 = 0;

    // Count content chunks (exclude meta folder paths for file_count/size)
    for ((path_hash, _layer_hash), chunk) in &modpkg.chunks {
        let path = modpkg.chunk_paths.get(path_hash).map(String::as_str).unwrap_or("");
        if path.starts_with("_meta_/") {
            continue;
        }
        file_count += 1;
        total_size += chunk.uncompressed_size as u64;
    }

    // Layer counts: derive from header layer list.
    let mut layer_counts: BTreeMap<String, u64> = BTreeMap::new();
    for ((path_hash, layer_hash), _chunk) in &modpkg.chunks {
        let path = modpkg.chunk_paths.get(path_hash).map(String::as_str).unwrap_or("");
        if path.starts_with("_meta_/") {
            continue;
        }
        if let Some(layer) = modpkg.layers.get(layer_hash) {
            *layer_counts.entry(layer.name.clone()).or_insert(0) += 1;
        }
    }

    let mut layers = Vec::new();
    for (_hash, layer) in &modpkg.layers {
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

fn resolve_storage_dirs(app_handle: &AppHandle, settings: &Settings) -> AppResult<(PathBuf, PathBuf)> {
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

pub(crate) fn get_enabled_mods_for_overlay(
    app_handle: &AppHandle,
    settings: &Settings,
) -> AppResult<Vec<EnabledMod>> {
    let (storage_dir, _) = resolve_storage_dirs(app_handle, settings)?;
    let mut index = load_library_index(&storage_dir)?;

    // Deterministic ordering: by installed_at then id.
    index.mods.sort_by(|a, b| a.installed_at.cmp(&b.installed_at).then(a.id.cmp(&b.id)));

    Ok(index
        .mods
        .into_iter()
        .filter(|m| m.enabled)
        .map(|m| EnabledMod {
            id: m.id,
            mod_dir: m.mod_dir,
        })
        .collect())
}

fn read_installed_mod(entry: &LibraryModEntry) -> AppResult<InstalledMod> {
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

    Ok(InstalledMod {
        id: entry.id.clone(),
        name: project.name,
        display_name: project.display_name,
        version: project.version,
        description: Some(project.description).filter(|s| !s.is_empty()),
        authors,
        enabled: entry.enabled,
        installed_at: entry.installed_at,
        file_path: entry.file_path.clone(),
        layers,
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

fn install_fantome_to_dir(file_path: &Path, mod_dir: &Path) -> AppResult<()> {
    let file = std::fs::File::open(file_path)?;
    let mut extractor = ltk_fantome::FantomeExtractor::new(file)
        .map_err(|e| AppError::Other(format!("Fantome extract init failed: {}", e)))?;
    extractor
        .extract_to(mod_dir)
        .map_err(|e| AppError::Other(format!("Fantome extract failed: {}", e)))?;
    Ok(())
}

fn install_modpkg_to_dir(file_path: &Path, mod_dir: &Path) -> AppResult<()> {
    let file = std::fs::File::open(file_path)?;
    let mut modpkg = Modpkg::mount_from_reader(file).map_err(|e| AppError::Modpkg(e.to_string()))?;

    // Extract content into content/<layer>/...
    let content_dir = mod_dir.join("content");
    fs::create_dir_all(&content_dir)?;
    let mut extractor = ModpkgExtractor::new(&mut modpkg);
    extractor
        .extract_all(&content_dir)
        .map_err(|e| AppError::Modpkg(e.to_string()))?;

    // Build a mod project config from metadata/header layers.
    let metadata = modpkg.load_metadata().map_err(|e| AppError::Modpkg(e.to_string()))?;

    // Use header layers as source of truth (they always exist for modpkg content).
    let mut layers: Vec<ModProjectLayer> = modpkg
        .layers
        .values()
        .map(|l| ModProjectLayer {
            name: l.name.clone(),
            priority: l.priority,
            description: metadata
                .layers
                .iter()
                .find(|ml| ml.name == l.name)
                .and_then(|ml| ml.description.clone()),
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

    let config_path = mod_dir.join("mod.config.json");
    fs::write(config_path, serde_json::to_string_pretty(&project)?)?;

    // Optional meta: README + thumbnail.webp
    if let Ok(readme_bytes) = modpkg.load_readme() {
        let _ = fs::write(mod_dir.join("README.md"), readme_bytes);
    }
    if let Ok(thumbnail_bytes) = modpkg.load_thumbnail() {
        let _ = fs::write(mod_dir.join("thumbnail.webp"), thumbnail_bytes);
    }

    Ok(())
}


