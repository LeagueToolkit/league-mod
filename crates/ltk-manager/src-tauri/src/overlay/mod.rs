pub mod fantome_content;
pub mod modpkg_content;

use crate::error::{AppError, AppResult};
use crate::mods::{get_enabled_mods_for_overlay, resolve_storage_dir};
use crate::state::Settings;
use camino::Utf8PathBuf;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

/// Progress event emitted during overlay building.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayProgress {
    pub stage: String,
    pub current_file: Option<String>,
    pub current: u32,
    pub total: u32,
}

/// Ensure the overlay exists and is up-to-date for the current enabled mod set.
///
/// Returns the overlay root directory (the prefix passed to the legacy patcher).
pub fn ensure_overlay(app_handle: &AppHandle, settings: &Settings) -> AppResult<PathBuf> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;

    let game_dir = resolve_game_dir(settings)?;

    // Get active profile slug and enabled mods
    let (profile_slug, enabled_mods) = get_enabled_mods_for_overlay(app_handle, settings)?;

    // Use profile-specific overlay directory
    let overlay_root = storage_dir
        .join("profiles")
        .join(profile_slug.as_str())
        .join("overlay");

    tracing::info!("Overlay: storage_dir={}", storage_dir.display());
    tracing::info!("Overlay: profile_slug={}", profile_slug);
    tracing::info!("Overlay: overlay_root={}", overlay_root.display());
    tracing::info!("Overlay: game_dir={}", game_dir.display());

    let enabled_ids = enabled_mods
        .iter()
        .map(|m| m.id.clone())
        .collect::<Vec<_>>();
    tracing::info!(
        "Overlay: enabled_mods={} ids=[{}]",
        enabled_ids.len(),
        enabled_ids.join(", ")
    );

    // Convert to Utf8PathBuf for ltk_overlay API
    let utf8_game_dir = Utf8PathBuf::from_path_buf(game_dir)
        .map_err(|p| AppError::Other(format!("Non-UTF-8 game directory path: {}", p.display())))?;
    let utf8_overlay_root = Utf8PathBuf::from_path_buf(overlay_root.clone())
        .map_err(|p| AppError::Other(format!("Non-UTF-8 overlay root path: {}", p.display())))?;

    // Build WAD blocklist from settings
    let mut blocked_wads = Vec::new();
    if !settings.patch_tft {
        blocked_wads.push("map22.wad.client".to_string());
    }

    // Build overlay using ltk_overlay crate
    let app_handle_clone = app_handle.clone();
    let mut builder = ltk_overlay::OverlayBuilder::new(utf8_game_dir, utf8_overlay_root)
        .with_blocked_wads(blocked_wads)
        .with_progress(move |progress| {
            // Convert ltk_overlay progress to our format
            let stage = match progress.stage {
                ltk_overlay::OverlayStage::Indexing => "indexing",
                ltk_overlay::OverlayStage::CollectingOverrides => "collecting",
                ltk_overlay::OverlayStage::PatchingWad => "patching",
                ltk_overlay::OverlayStage::ApplyingStringOverrides => "strings",
                ltk_overlay::OverlayStage::Complete => "complete",
            };

            let _ = app_handle_clone.emit(
                "overlay-progress",
                OverlayProgress {
                    stage: stage.to_string(),
                    current_file: progress.current_file,
                    current: progress.current,
                    total: progress.total,
                },
            );
        });

    builder.set_enabled_mods(enabled_mods);

    builder
        .build()
        .map_err(|e| AppError::Other(format!("Overlay build failed: {}", e)))?;

    Ok(overlay_root)
}

/// Delete the active profile's `overlay.json` to force the next build to rebuild.
///
/// This is called after mod-state-changing operations (toggle, reorder, install,
/// uninstall) as defense-in-depth: even though the builder's `matches()` check
/// detects mismatches at build time, proactive invalidation makes the intent
/// explicit and avoids relying solely on the comparison being correct.
pub fn invalidate_overlay(app_handle: &AppHandle, settings: &Settings) -> AppResult<()> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let index = crate::mods::load_library_index(&storage_dir)?;
    let active_profile = crate::mods::get_active_profile(&index)?;
    let overlay_json = storage_dir
        .join("profiles")
        .join(active_profile.slug.as_str())
        .join("overlay")
        .join("overlay.json");
    if overlay_json.exists() {
        std::fs::remove_file(&overlay_json)?;
        tracing::info!("Invalidated overlay for profile {}", active_profile.slug);
    }
    Ok(())
}

fn resolve_game_dir(settings: &Settings) -> AppResult<PathBuf> {
    let league_root = settings
        .league_path
        .clone()
        .ok_or_else(|| AppError::ValidationFailed("League path is not configured".to_string()))?;

    // Users might configure either the install root (…/League of Legends) or the Game dir (…/League of Legends/Game).
    // Accept both.
    let game_dir = league_root.join("Game");
    if game_dir.exists() {
        return Ok(game_dir);
    }
    if league_root.join("DATA").exists() {
        return Ok(league_root);
    }

    Err(AppError::ValidationFailed(format!(
        "League path does not look like an install root or a Game directory: {}",
        league_root.display()
    )))
}
