use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use ltk_mod_project::{ModMap, ModProject, ModProjectAuthor, ModProjectLayer, ModTag};
use ltk_modpkg::Modpkg;
use uuid::Uuid;
use zip::ZipArchive;

use crate::error::{LibraryError, LibraryResult};
use crate::index::{LibraryIndex, LibraryModEntry, ModArchiveFormat};
use crate::progress::{InstallProgress, ProgressReporter};
use crate::query::{InstalledMod, ModLayer};

/// Result of a bulk mod install operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkInstallResult {
    pub installed: Vec<InstalledMod>,
    pub failed: Vec<BulkInstallError>,
}

/// Error info for a single file that failed during bulk install.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkInstallError {
    pub file_path: String,
    pub file_name: String,
    pub message: String,
}

impl LibraryIndex {
    /// Install a single mod from a package file.
    pub fn install_mod(
        &mut self,
        storage_dir: &Utf8Path,
        file_path: &Utf8Path,
    ) -> LibraryResult<InstalledMod> {
        let (_entry, installed_mod) = self.install_single_mod(storage_dir, file_path)?;
        Ok(installed_mod)
    }

    /// Install multiple mods in a batch operation.
    pub fn install_mods_batch(
        &mut self,
        storage_dir: &Utf8Path,
        file_paths: &[Utf8PathBuf],
        reporter: &dyn ProgressReporter,
    ) -> LibraryResult<BulkInstallResult> {
        if file_paths.is_empty() {
            return Ok(BulkInstallResult {
                installed: Vec::new(),
                failed: Vec::new(),
            });
        }

        let total = file_paths.len();
        let mut installed = Vec::new();
        let mut failed = Vec::new();

        for (i, file_path) in file_paths.iter().enumerate() {
            let file_name = file_path
                .file_name()
                .unwrap_or(file_path.as_str())
                .to_string();

            reporter.on_install_progress(InstallProgress {
                current: i + 1,
                total,
                current_file: file_name.clone(),
            });

            match self.install_single_mod(storage_dir, file_path) {
                Ok((_entry, mod_info)) => installed.push(mod_info),
                Err(e) => {
                    tracing::warn!("Failed to install {}: {}", file_path, e);
                    failed.push(BulkInstallError {
                        file_path: file_path.to_string(),
                        file_name,
                        message: e.to_string(),
                    });
                }
            }
        }

        Ok(BulkInstallResult { installed, failed })
    }

    fn install_single_mod(
        &mut self,
        storage_dir: &Utf8Path,
        file_path: &Utf8Path,
    ) -> LibraryResult<(LibraryModEntry, InstalledMod)> {
        if !file_path.exists() {
            return Err(LibraryError::InvalidPath(file_path.to_string()));
        }

        let archives_dir = storage_dir.join("archives");
        let metadata_dir = storage_dir.join("mods");
        fs::create_dir_all(archives_dir.as_std_path())?;
        fs::create_dir_all(metadata_dir.as_std_path())?;

        let id = Uuid::new_v4().to_string();
        let format = file_path
            .extension()
            .and_then(ModArchiveFormat::from_extension)
            .unwrap_or(ModArchiveFormat::Modpkg);

        let installed_at = Utc::now();

        let archive_filename = format!("{}.{}", id, format.extension());
        let archive_path = archives_dir.join(&archive_filename);
        fs::copy(file_path.as_std_path(), archive_path.as_std_path())?;

        let mod_metadata_dir = metadata_dir.join(&id);
        fs::create_dir_all(mod_metadata_dir.as_std_path())?;

        match format {
            ModArchiveFormat::Fantome => {
                extract_fantome_metadata(&archive_path, &mod_metadata_dir)?;
            }
            ModArchiveFormat::Modpkg => {
                extract_modpkg_metadata(&archive_path, &mod_metadata_dir)?;
            }
        }

        let entry = LibraryModEntry {
            id: id.clone(),
            installed_at,
            format,
        };
        self.mods.push(entry.clone());

        if let Ok(profile) = self.active_profile_mut() {
            profile.enabled_mods.insert(0, id.clone());
            profile.mod_order.insert(0, id.clone());
        }

        if let Ok(project) = load_mod_project(&mod_metadata_dir) {
            let new_layer_names: HashSet<&str> =
                project.layers.iter().map(|l| l.name.as_str()).collect();
            for profile in &mut self.profiles {
                if let Some(states) = profile.layer_states.get_mut(&id) {
                    states.retain(|name, _| new_layer_names.contains(name.as_str()));
                }
            }
        }

        let installed_mod = read_installed_mod(&entry, true, storage_dir, None)?;
        Ok((entry, installed_mod))
    }
}

pub(crate) fn load_mod_project(mod_dir: &Utf8Path) -> LibraryResult<ModProject> {
    let config_path = mod_dir.join("mod.config.json");
    let contents = fs::read_to_string(config_path.as_std_path()).map_err(|e| {
        LibraryError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read {}: {}", config_path, e),
        ))
    })?;
    serde_json::from_str(&contents).map_err(LibraryError::from)
}

/// Read a single installed mod's metadata from disk and merge with profile state.
pub(crate) fn read_installed_mod(
    entry: &LibraryModEntry,
    enabled: bool,
    storage_dir: &Utf8Path,
    layer_states: Option<&HashMap<String, bool>>,
) -> LibraryResult<InstalledMod> {
    let mod_dir = entry.metadata_dir(storage_dir);
    let project = load_mod_project(&mod_dir)?;

    let authors = project
        .authors
        .iter()
        .map(|a| match a {
            ModProjectAuthor::Name(name) => name.clone(),
            ModProjectAuthor::Role { name, role: _ } => name.clone(),
        })
        .collect();

    let layers = project
        .layers
        .iter()
        .map(|l| ModLayer {
            name: l.name.clone(),
            priority: l.priority,
            enabled: layer_states
                .and_then(|states| states.get(&l.name))
                .copied()
                .unwrap_or(true),
        })
        .collect();

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
        tags: project.tags.iter().map(|t| t.to_string()).collect(),
        champions: project.champions.clone(),
        maps: project.maps.iter().map(|m| m.to_string()).collect(),
        mod_dir: mod_dir.to_string(),
    })
}

fn extract_modpkg_metadata(archive_path: &Utf8Path, metadata_dir: &Utf8Path) -> LibraryResult<()> {
    let file = File::open(archive_path.as_std_path())?;
    let mut modpkg = Modpkg::mount_from_reader(file)?;

    let metadata = modpkg.load_metadata()?;

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
            .map(|a| ModProjectAuthor::Name(a.name))
            .collect(),
        license: None,
        tags: metadata.tags.into_iter().map(ModTag::from).collect(),
        champions: metadata.champions,
        maps: metadata.maps.into_iter().map(ModMap::from).collect(),
        transformers: Vec::new(),
        layers,
        thumbnail: None,
    };

    let config_json = serde_json::to_string_pretty(&project)?;
    fs::write(metadata_dir.join("mod.config.json"), config_json)?;

    if let Ok(thumbnail_bytes) = modpkg.load_thumbnail() {
        let _ = fs::write(metadata_dir.join("thumbnail.webp"), thumbnail_bytes);
    }

    Ok(())
}

fn extract_fantome_metadata(file_path: &Utf8Path, metadata_dir: &Utf8Path) -> LibraryResult<()> {
    let file = File::open(file_path.as_std_path())?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| LibraryError::Fantome(format!("Failed to open fantome archive: {}", e)))?;

    let mut info_content = String::new();
    let mut found_metadata = false;

    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| LibraryError::Fantome(format!("Failed to read archive entry: {}", e)))?;
        let name = file.name().to_lowercase();

        if name == "meta/info.json" {
            drop(file);
            let mut info_file = archive
                .by_index(i)
                .map_err(|e| LibraryError::Fantome(format!("Failed to read info.json: {}", e)))?;
            info_file.read_to_string(&mut info_content).map_err(|e| {
                LibraryError::Fantome(format!("Failed to read info.json content: {}", e))
            })?;
            found_metadata = true;
            break;
        }
    }

    if !found_metadata {
        return Err(LibraryError::Fantome(
            "No META/info.json found in fantome archive".to_string(),
        ));
    }

    let info: serde_json::Value = serde_json::from_str(&info_content)
        .map_err(|e| LibraryError::Fantome(format!("Invalid info.json: {}", e)))?;

    let name = info["Name"].as_str().unwrap_or("unknown").to_string();
    let display_name = name.clone();
    let author = info["Author"].as_str().unwrap_or("Unknown").to_string();
    let version = info["Version"].as_str().unwrap_or("1.0.0").to_string();
    let description = info["Description"].as_str().unwrap_or("").to_string();

    let project = ModProject {
        name: slug::slugify(&display_name),
        display_name,
        version,
        description,
        authors: vec![ModProjectAuthor::Name(author)],
        license: None,
        tags: Vec::new(),
        champions: Vec::new(),
        maps: Vec::new(),
        transformers: Vec::new(),
        layers: vec![ModProjectLayer {
            name: "base".to_string(),
            priority: 0,
            description: None,
            string_overrides: Default::default(),
        }],
        thumbnail: None,
    };

    let config_json = serde_json::to_string_pretty(&project)?;
    fs::write(metadata_dir.join("mod.config.json"), config_json)?;

    Ok(())
}
