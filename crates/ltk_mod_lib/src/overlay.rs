use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use ltk_modpkg::Modpkg;
use ltk_overlay::{FantomeContent, ModpkgContent};

use crate::error::{LibraryError, LibraryResult};
use crate::index::{LibraryIndex, ModArchiveFormat};
use crate::progress::ProgressReporter;

const SCRIPTS_WAD: &str = "scripts.wad.client";
const TFT_WAD: &str = "map22.wad.client";

/// Overlay build configuration.
pub struct OverlayConfig {
    pub league_path: Utf8PathBuf,
    pub patch_tft: bool,
    pub block_scripts_wad: bool,
    pub wad_blocklist: Vec<String>,
}

impl OverlayConfig {
    /// Build the full blocked WADs list from config settings.
    ///
    /// Starts with the user-configured blocklist, then conditionally adds
    /// `scripts.wad.client` and `map22.wad.client` based on flags.
    pub fn build_blocked_wads_list(&self) -> Vec<String> {
        let mut blocked: Vec<String> = self
            .wad_blocklist
            .iter()
            .map(|w| w.to_lowercase())
            .collect();
        if self.block_scripts_wad && !blocked.contains(&SCRIPTS_WAD.to_string()) {
            blocked.push(SCRIPTS_WAD.to_string());
        }
        if !self.patch_tft && !blocked.contains(&TFT_WAD.to_string()) {
            blocked.push(TFT_WAD.to_string());
        }
        blocked
    }
}

/// Build the overlay for the active profile.
///
/// Returns the overlay root directory path on success.
pub(crate) fn build_overlay(
    storage_dir: &Utf8Path,
    index: &LibraryIndex,
    config: &OverlayConfig,
    reporter: Arc<dyn ProgressReporter>,
) -> LibraryResult<Utf8PathBuf> {
    let game_dir = resolve_game_dir(&config.league_path)?;
    let active_profile = index.active_profile()?;

    let profile_dir = storage_dir
        .join("profiles")
        .join(active_profile.slug.as_str());
    let overlay_root = profile_dir.join("overlay");

    let mut builder = ltk_overlay::OverlayBuilder::new(game_dir, overlay_root.clone(), profile_dir)
        .with_blocked_wads(config.build_blocked_wads_list())
        .with_enabled_mods(collect_enabled_mods(storage_dir, index)?)
        .with_progress(move |progress| {
            reporter.on_overlay_progress(progress);
        });

    builder
        .build()
        .map_err(|e| LibraryError::OverlayFailed(e.to_string()))?;

    Ok(overlay_root)
}

/// Collect enabled mods as `ltk_overlay::EnabledMod` for the active profile.
pub fn collect_enabled_mods(
    storage_dir: &Utf8Path,
    index: &LibraryIndex,
) -> LibraryResult<Vec<ltk_overlay::EnabledMod>> {
    let active_profile = index.active_profile()?;
    let mut enabled_mods = Vec::new();

    for mod_id in &active_profile.enabled_mods {
        let Some(entry) = index.mods.iter().find(|m| &m.id == mod_id) else {
            tracing::warn!("Mod {} in profile but not found in library", mod_id);
            continue;
        };

        let archive_path = entry.archive_path(storage_dir);
        if !archive_path.exists() {
            tracing::warn!("Archive not found for mod {}: {}", entry.id, archive_path);
            continue;
        }

        let content: Box<dyn ltk_overlay::ModContentProvider> = match entry.format {
            ModArchiveFormat::Fantome => Box::new(
                FantomeContent::new(File::open(archive_path.as_std_path())?)
                    .map_err(|e| {
                        LibraryError::Fantome(format!("Failed to open fantome archive: {}", e))
                    })?
                    .with_archive_path(archive_path.clone()),
            ),
            ModArchiveFormat::Modpkg => Box::new(
                ModpkgContent::new(Modpkg::mount_from_reader(File::open(
                    archive_path.as_std_path(),
                )?)?)
                .with_archive_path(archive_path.clone()),
            ),
        };

        let enabled_layers =
            active_profile
                .layer_states
                .get(&entry.id)
                .map(|states: &HashMap<String, bool>| {
                    states
                        .iter()
                        .filter(|(_, &enabled)| enabled)
                        .map(|(name, _)| name.clone())
                        .collect::<HashSet<String>>()
                });

        enabled_mods.push(ltk_overlay::EnabledMod {
            id: entry.id.clone(),
            content,
            enabled_layers,
        });
    }

    Ok(enabled_mods)
}

fn resolve_game_dir(league_path: &Utf8Path) -> LibraryResult<Utf8PathBuf> {
    let game_dir = league_path.join("Game");
    if game_dir.exists() {
        return Ok(game_dir);
    }
    if league_path.join("DATA").exists() {
        return Ok(league_path.to_path_buf());
    }

    Err(LibraryError::ValidationFailed(format!(
        "League path does not look like an install root or a Game directory: {}",
        league_path
    )))
}
