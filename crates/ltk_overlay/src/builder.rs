//! Main overlay builder implementation.
//!
//! The [`OverlayBuilder`] orchestrates the full overlay build pipeline:
//! game indexing, override collection, cross-WAD distribution, and WAD patching.
//!
//! # Build Algorithm
//!
//! 1. Validate that `game_dir/DATA/FINAL` exists.
//! 2. Build (or load from cache) a [`GameIndex`] from all `.wad.client` files.
//! 3. Load the saved [`OverlayState`] and choose a build strategy:
//!    - **Skip**: mod list, game fingerprint, and per-WAD fingerprints all match,
//!      and every overlay WAD file still exists on disk.
//!    - **Incremental**: game fingerprint and state version match but mod list
//!      differs. Compute per-WAD override fingerprints and only rebuild WADs
//!      whose fingerprint changed. Remove stale WADs no longer needed.
//!    - **Full rebuild**: state version or game fingerprint mismatch. Wipe all
//!      overlay WAD files and rebuild everything from scratch.
//! 4. For each WAD that needs rebuilding, call
//!    [`build_patched_wad`](crate::wad_builder::build_patched_wad).
//! 5. Persist the new [`OverlayState`] with per-WAD fingerprints.

use crate::content::ModContentProvider;
use crate::error::{Error, Result};
use crate::game_index::GameIndex;
use crate::state::OverlayState;
use crate::utils::{compute_wad_overrides_fingerprint, resolve_chunk_hash};
use crate::wad_builder::build_patched_wad;
use camino::{Utf8Path, Utf8PathBuf};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use xxhash_rust::xxh3::xxh3_64;

/// Shared byte buffer for override data distributed across multiple WADs.
///
/// A single mod override may be distributed to many WADs (cross-WAD matching),
/// so we share the bytes via `Arc` instead of cloning.
type SharedBytes = Arc<[u8]>;

/// A mod to be included in the overlay build.
///
/// Each enabled mod contributes override files through its [`ModContentProvider`].
/// Mods are processed in the order they appear in the `enabled_mods` list passed to
/// [`OverlayBuilder::set_enabled_mods`]. Position 0 (first in the list) has the
/// **highest** priority — when two mods override the same path hash, the mod
/// closer to the front of the list wins.
pub struct EnabledMod {
    /// Unique identifier for the mod (used in state tracking and logging).
    pub id: String,
    /// Content provider for accessing mod metadata and override files.
    ///
    /// This can be backed by a filesystem directory, a `.modpkg` archive, a
    /// `.fantome` ZIP, or any other source that implements [`ModContentProvider`].
    pub content: Box<dyn ModContentProvider>,
}

/// Progress information emitted during overlay building.
///
/// Serialized as JSON and sent to the frontend via Tauri events so the UI can
/// display a progress bar and stage label. The `current`/`total` fields are only
/// meaningful during the [`PatchingWad`](OverlayStage::PatchingWad) stage.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayProgress {
    /// Current stage of the build process.
    pub stage: OverlayStage,
    /// Filename of the WAD currently being patched (set during `PatchingWad`).
    pub current_file: Option<String>,
    /// 1-based index of the WAD currently being patched.
    pub current: u32,
    /// Total number of WADs that need patching.
    pub total: u32,
}

/// Stages of the overlay build pipeline.
///
/// Emitted in order: `Indexing` -> `CollectingOverrides` -> `PatchingWad` (repeated) -> `Complete`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayStage {
    /// Scanning the game directory and building the [`GameIndex`].
    Indexing,
    /// Reading override files from all enabled mods.
    CollectingOverrides,
    /// Building a patched WAD file in the overlay directory.
    PatchingWad,
    /// Applying string table overrides (reserved for future use).
    ApplyingStringOverrides,
    /// Build finished successfully.
    Complete,
}

/// Summary returned after an overlay build completes.
#[derive(Debug)]
pub struct OverlayBuildResult {
    /// Root directory of the overlay (mirrors the game's `DATA/FINAL` structure).
    pub overlay_root: Utf8PathBuf,
    /// WAD files that were freshly built during this run.
    pub wads_built: Vec<Utf8PathBuf>,
    /// WAD files reused from a previous build (unchanged fingerprint).
    pub wads_reused: Vec<Utf8PathBuf>,
    /// Detected conflicts between mods (not yet implemented).
    pub conflicts: Vec<Conflict>,
    /// Wall-clock time for the entire build.
    pub build_time: Duration,
}

/// A conflict where multiple mods override the same chunk (not yet implemented).
#[derive(Debug, Clone)]
pub struct Conflict {
    /// xxHash3 path hash of the conflicting chunk.
    pub path_hash: u64,
    /// Human-readable path (if available from hash tables).
    pub path: String,
    /// All mods that contributed an override for this chunk.
    pub contributing_mods: Vec<ModContribution>,
    /// The mod whose override was used (last-writer-wins).
    pub winner: String,
}

/// Details about one mod's contribution to a conflicting chunk.
#[derive(Debug, Clone)]
pub struct ModContribution {
    /// Unique mod identifier.
    pub mod_id: String,
    /// Human-readable mod name.
    pub mod_name: String,
    /// Layer name the override came from.
    pub layer: String,
    /// Layer priority value.
    pub priority: i32,
    /// Position in the enabled mods list (0-based).
    pub install_order: usize,
}

type ProgressCallback = Arc<dyn Fn(OverlayProgress) + Send + Sync>;

/// Orchestrates the overlay build pipeline.
///
/// Create a builder with [`new`](Self::new), configure it with
/// [`set_enabled_mods`](Self::set_enabled_mods) and optionally
/// [`with_progress`](Self::with_progress), then call [`build`](Self::build).
///
/// The builder owns the enabled mod list and consumes each mod's content provider
/// during the build. After building, the same builder instance can be reconfigured
/// and built again.
pub struct OverlayBuilder {
    game_dir: Utf8PathBuf,
    overlay_root: Utf8PathBuf,
    enabled_mods: Vec<EnabledMod>,
    progress_callback: Option<ProgressCallback>,
}

impl OverlayBuilder {
    /// Create a new overlay builder.
    ///
    /// # Arguments
    ///
    /// * `game_dir` — Path to the League of Legends `Game/` directory. Must contain
    ///   a `DATA/FINAL` subdirectory with `.wad.client` files.
    /// * `overlay_root` — Directory where patched WAD files will be written.
    pub fn new(game_dir: Utf8PathBuf, overlay_root: Utf8PathBuf) -> Self {
        Self {
            game_dir,
            overlay_root,
            enabled_mods: Vec::new(),
            progress_callback: None,
        }
    }

    /// Register a progress callback.
    ///
    /// The callback receives [`OverlayProgress`] updates at each stage of the build.
    /// This is typically used to forward progress to a UI via Tauri events.
    pub fn with_progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(OverlayProgress) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    /// Set the ordered list of mods to include in the overlay.
    ///
    /// Order matters: the first mod in the list (index 0) has the highest priority.
    /// When two mods override the same chunk, the mod closer to the front wins.
    pub fn set_enabled_mods(&mut self, mods: Vec<EnabledMod>) {
        self.enabled_mods = mods;
    }

    /// Build the overlay with incremental rebuild support.
    ///
    /// 1. If the overlay state matches exactly and all WAD files exist → skip.
    /// 2. If the game fingerprint and state version match → incremental rebuild
    ///    (only re-patch WADs whose override fingerprint changed).
    /// 3. Otherwise → full rebuild (wipe and rebuild everything).
    pub fn build(&mut self) -> Result<OverlayBuildResult> {
        let start_time = std::time::Instant::now();

        tracing::info!("Building overlay...");
        tracing::info!("Game dir: {}", self.game_dir);
        tracing::info!("Overlay root: {}", self.overlay_root);
        tracing::info!("Enabled mods: {}", self.enabled_mods.len());

        // --- Stage: Indexing ---
        self.emit_progress(OverlayProgress {
            stage: OverlayStage::Indexing,
            current_file: None,
            current: 0,
            total: 0,
        });

        // Validate game directory
        let data_final_dir = self.game_dir.join("DATA").join("FINAL");
        if !data_final_dir.as_std_path().exists() {
            return Err(format!(
                "League path does not contain Game/DATA/FINAL. Game dir: '{}'",
                self.game_dir
            )
            .into());
        }

        // Ensure overlay root exists (for cache and state files)
        std::fs::create_dir_all(self.overlay_root.as_std_path())?;

        // Build or load cached game index
        let cache_path = self.overlay_root.join("game_index_cache.json");
        let game_index = GameIndex::load_or_build(&self.game_dir, &cache_path)?;

        // Load previous state
        let state_path = self.overlay_root.join("overlay.json");
        let enabled_ids: Vec<String> = self.enabled_mods.iter().map(|m| m.id.clone()).collect();
        let prev_state = OverlayState::load(&state_path)?;

        // --- Handle empty mod list ---
        if self.enabled_mods.is_empty() {
            tracing::info!("Overlay: no enabled mods, cleaning overlay");
            self.clean_overlay_wads()?;
            let state =
                OverlayState::new(Vec::new(), game_index.game_fingerprint(), BTreeMap::new());
            state.save(&state_path)?;
            self.emit_progress(OverlayProgress {
                stage: OverlayStage::Complete,
                current_file: None,
                current: 0,
                total: 0,
            });
            return Ok(OverlayBuildResult {
                overlay_root: self.overlay_root.clone(),
                wads_built: Vec::new(),
                wads_reused: Vec::new(),
                conflicts: Vec::new(),
                build_time: start_time.elapsed(),
            });
        }

        // --- Fast path: exact match → skip entirely ---
        if let Some(ref state) = prev_state {
            if state.matches(&enabled_ids, game_index.game_fingerprint()) {
                if self.validate_wads_exist(state) {
                    tracing::info!("Overlay: exact match, skipping build");
                    self.emit_progress(OverlayProgress {
                        stage: OverlayStage::Complete,
                        current_file: None,
                        current: 0,
                        total: 0,
                    });
                    let reused: Vec<Utf8PathBuf> = state
                        .wad_fingerprints
                        .keys()
                        .map(|k| self.overlay_root.join(k))
                        .collect();
                    return Ok(OverlayBuildResult {
                        overlay_root: self.overlay_root.clone(),
                        wads_built: Vec::new(),
                        wads_reused: reused,
                        conflicts: Vec::new(),
                        build_time: start_time.elapsed(),
                    });
                }
                tracing::info!(
                    "Overlay: state matched but some WADs missing, doing incremental repair"
                );
            }
        }

        // Determine if incremental build is possible
        let can_incremental = prev_state
            .as_ref()
            .is_some_and(|s| s.supports_incremental(game_index.game_fingerprint()));

        if !can_incremental {
            tracing::info!(
                "Overlay: full rebuild required (state version or game fingerprint mismatch)"
            );
            self.clean_overlay_wads()?;
        }

        // --- Stage: Collecting overrides ---
        self.emit_progress(OverlayProgress {
            stage: OverlayStage::CollectingOverrides,
            current_file: None,
            current: 0,
            total: 0,
        });

        let (all_overrides, override_target_wads) = self.collect_all_overrides(&game_index)?;
        let wad_overrides =
            self.distribute_overrides(&all_overrides, &game_index, &override_target_wads);

        // --- Compute per-WAD fingerprints ---
        let new_wad_fingerprints: BTreeMap<String, u64> = wad_overrides
            .iter()
            .map(|(wad_path, overrides)| {
                (
                    wad_path.as_str().to_string(),
                    compute_wad_overrides_fingerprint(overrides),
                )
            })
            .collect();

        // --- Partition WADs into rebuild / reuse ---
        let mut wads_to_build: Vec<Utf8PathBuf> = Vec::new();
        let mut wads_to_reuse: Vec<Utf8PathBuf> = Vec::new();

        for (wad_path_str, &new_fp) in &new_wad_fingerprints {
            let wad_path = Utf8PathBuf::from(wad_path_str);
            let overlay_wad = self.overlay_root.join(&wad_path);

            if can_incremental {
                if let Some(ref state) = prev_state {
                    if let Some(old_fp) = state.wad_fingerprint(wad_path_str) {
                        if old_fp == new_fp && overlay_wad.as_std_path().exists() {
                            tracing::debug!("Reusing WAD: {}", wad_path);
                            wads_to_reuse.push(wad_path);
                            continue;
                        }
                    }
                }
            }

            tracing::debug!("Need to rebuild WAD: {}", wad_path);
            wads_to_build.push(wad_path);
        }

        // --- Clean stale WADs (in old state but not in new) ---
        if can_incremental {
            if let Some(ref state) = prev_state {
                for old_wad_path in state.wad_fingerprints.keys() {
                    if !new_wad_fingerprints.contains_key(old_wad_path) {
                        let stale_path = self.overlay_root.join(old_wad_path);
                        if stale_path.as_std_path().exists() {
                            tracing::info!("Removing stale WAD: {}", stale_path);
                            std::fs::remove_file(stale_path.as_std_path())?;
                        }
                        self.cleanup_empty_parents(&stale_path);
                    }
                }
            }
        }

        // --- Stage: Patching WADs (parallel) ---
        let total_wads = wads_to_build.len() as u32;
        let completed = AtomicU32::new(0);
        let game_dir = &self.game_dir;
        let overlay_root = &self.overlay_root;
        let progress_callback = &self.progress_callback;

        let emit = |progress: OverlayProgress| {
            if let Some(callback) = progress_callback {
                callback(progress);
            }
        };

        emit(OverlayProgress {
            stage: OverlayStage::PatchingWad,
            current_file: None,
            current: 0,
            total: total_wads,
        });

        let built_paths: Result<Vec<Utf8PathBuf>> = wads_to_build
            .par_iter()
            .map(|relative_game_path| {
                let src_wad_path = game_dir.join(relative_game_path);
                let dst_wad_path = overlay_root.join(relative_game_path);

                let mut overrides = wad_overrides
                    .get(relative_game_path)
                    .expect("WAD must be in overrides map")
                    .clone();

                tracing::info!(
                    "Patching WAD src={} dst={} overrides={}",
                    src_wad_path,
                    dst_wad_path,
                    overrides.len()
                );

                let override_hashes: HashSet<u64> = overrides.keys().copied().collect();
                build_patched_wad(&src_wad_path, &dst_wad_path, &override_hashes, |hash| {
                    overrides
                        .remove(&hash)
                        .map(|arc| arc.to_vec())
                        .ok_or_else(|| {
                            Error::Other(format!("Missing override data for hash {:016x}", hash))
                        })
                })?;

                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                emit(OverlayProgress {
                    stage: OverlayStage::PatchingWad,
                    current_file: Some(
                        relative_game_path
                            .file_name()
                            .unwrap_or("unknown")
                            .to_string(),
                    ),
                    current: done,
                    total: total_wads,
                });

                Ok(dst_wad_path)
            })
            .collect();

        let built_paths = built_paths?;

        let reused_paths: Vec<Utf8PathBuf> = wads_to_reuse
            .iter()
            .map(|p| self.overlay_root.join(p))
            .collect();

        // --- Persist overlay state ---
        let state = OverlayState::new(
            enabled_ids,
            game_index.game_fingerprint(),
            new_wad_fingerprints,
        );
        state.save(&state_path)?;

        // --- Stage: Complete ---
        self.emit_progress(OverlayProgress {
            stage: OverlayStage::Complete,
            current_file: None,
            current: total_wads,
            total: total_wads,
        });

        tracing::info!(
            "Overlay build complete: {} built, {} reused in {:?}",
            built_paths.len(),
            reused_paths.len(),
            start_time.elapsed()
        );

        Ok(OverlayBuildResult {
            overlay_root: self.overlay_root.clone(),
            wads_built: built_paths,
            wads_reused: reused_paths,
            conflicts: Vec::new(), // Phase 4
            build_time: start_time.elapsed(),
        })
    }

    /// Force a full rebuild, ignoring the saved overlay state.
    ///
    /// Use this when the user explicitly requests a rebuild or when you know
    /// the overlay is out of date for reasons the state file cannot track.
    pub fn rebuild_all(&mut self) -> Result<OverlayBuildResult> {
        // Remove previous state so build() sees no match
        let state_path = self.overlay_root.join("overlay.json");
        if state_path.as_std_path().exists() {
            std::fs::remove_file(state_path.as_std_path())?;
        }
        self.clean_overlay_wads()?;
        self.build()
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Collect all mod overrides as a flat map: `path_hash -> shared bytes`.
    ///
    /// Also returns a map of `path_hash -> relative_wad_path` so new entries
    /// (not in any game WAD) can be routed to the correct WAD.
    ///
    /// Iterates mods in **reverse** order (back-to-front) so that the first mod
    /// in the list wins conflicts (last-writer-wins, with the front mod written last).
    /// Layers are sorted by priority. SubChunkTOC entries are filtered out.
    fn collect_all_overrides(
        &mut self,
        game_index: &GameIndex,
    ) -> Result<(HashMap<u64, SharedBytes>, HashMap<u64, Utf8PathBuf>)> {
        let mut all_overrides: HashMap<u64, SharedBytes> = HashMap::new();
        // Track which WAD (by relative game path) each override belongs to,
        // so new entries (not in any game WAD) can still be routed correctly.
        let mut override_target_wads: HashMap<u64, Utf8PathBuf> = HashMap::new();

        for enabled_mod in self.enabled_mods.iter_mut().rev() {
            tracing::info!("Processing mod id={}", enabled_mod.id);

            let project = enabled_mod.content.mod_project()?;
            let mut layers = project.layers.clone();
            layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

            for layer in &layers {
                let wad_names = enabled_mod.content.list_layer_wads(&layer.name)?;
                if wad_names.is_empty() {
                    tracing::debug!(
                        "Mod={} layer='{}' no WADs found, skipping",
                        enabled_mod.id,
                        layer.name,
                    );
                    continue;
                }

                tracing::info!("Mod={} layer='{}'", enabled_mod.id, layer.name);

                for wad_name in &wad_names {
                    let original_wad_path = game_index.find_wad(wad_name)?;
                    let relative_game_path = original_wad_path
                        .strip_prefix(&self.game_dir)
                        .map_err(|_| format!("WAD path is not under Game/: {}", original_wad_path))?
                        .to_path_buf();

                    tracing::info!(
                        "WAD='{}' resolved original={} relative={}",
                        wad_name,
                        original_wad_path,
                        relative_game_path
                    );

                    let before = all_overrides.len();
                    let override_files = enabled_mod
                        .content
                        .read_wad_overrides(&layer.name, wad_name)?;
                    for (rel_path, bytes) in override_files {
                        let path_hash = resolve_chunk_hash(&rel_path, &bytes)?;
                        all_overrides.insert(path_hash, Arc::from(bytes));
                        override_target_wads.insert(path_hash, relative_game_path.clone());
                    }
                    let after = all_overrides.len();
                    tracing::info!(
                        "WAD='{}' overrides added={} total_all_overrides={}",
                        wad_name,
                        after.saturating_sub(before),
                        after
                    );
                }
            }
        }

        tracing::info!(
            "Collected {} unique override hashes from all mods",
            all_overrides.len()
        );

        // Filter out SubChunkTOC entries — mods must not override these
        let blocked = game_index.subchunktoc_blocked();
        let before_filter = all_overrides.len();
        all_overrides.retain(|path_hash, _| {
            let dominated = blocked.contains(path_hash);
            if dominated {
                tracing::debug!("Filtered SubChunkTOC override: {:016x}", path_hash);
            }
            !dominated
        });
        let filtered_count = before_filter - all_overrides.len();
        if filtered_count > 0 {
            tracing::info!(
                "Filtered {} SubChunkTOC override(s) from mod overrides",
                filtered_count
            );
        }

        // Filter out lazy overrides — mod files identical to game originals.
        // Comparing xxh3_64(override_bytes) against the pre-computed uncompressed
        // content hash avoids recompressing and writing unchanged chunks.
        let before_lazy = all_overrides.len();
        all_overrides.retain(|&path_hash, bytes| {
            if let Some(original_hash) = game_index.content_hash(path_hash) {
                let override_hash = xxh3_64(bytes.as_ref());
                if override_hash == original_hash {
                    tracing::debug!("Filtered lazy override: {:016x}", path_hash);
                    return false;
                }
            }
            true
        });
        override_target_wads.retain(|hash, _| all_overrides.contains_key(hash));
        let lazy_count = before_lazy - all_overrides.len();
        if lazy_count > 0 {
            tracing::info!(
                "Filtered {} lazy override(s) (identical to game originals)",
                lazy_count
            );
        }

        Ok((all_overrides, override_target_wads))
    }

    /// Distribute collected overrides to all affected WADs using the game hash index.
    ///
    /// `SharedBytes` (`Arc<[u8]>`) avoids cloning the actual data — only the Arc
    /// pointer is cloned when the same override appears in multiple WADs.
    ///
    /// Overrides whose path hash doesn't appear in any game WAD are routed to the
    /// WAD indicated by the mod's directory structure (`override_target_wads`).
    ///
    /// Returns a map of `relative_wad_path -> { path_hash -> override_bytes }`.
    fn distribute_overrides(
        &self,
        all_overrides: &HashMap<u64, SharedBytes>,
        game_index: &GameIndex,
        override_target_wads: &HashMap<u64, Utf8PathBuf>,
    ) -> BTreeMap<Utf8PathBuf, HashMap<u64, SharedBytes>> {
        let mut wad_overrides: BTreeMap<Utf8PathBuf, HashMap<u64, SharedBytes>> = BTreeMap::new();
        let mut new_entry_count = 0usize;

        for (path_hash, override_bytes) in all_overrides {
            if let Some(wad_paths) = game_index.find_wads_with_hash(*path_hash) {
                for wad_path in wad_paths {
                    wad_overrides
                        .entry(wad_path.clone())
                        .or_default()
                        .insert(*path_hash, Arc::clone(override_bytes));
                }
            } else if let Some(target_wad) = override_target_wads.get(path_hash) {
                // New entry: hash doesn't exist in any game WAD.
                // Route it to the WAD that the mod's directory structure targets.
                wad_overrides
                    .entry(target_wad.clone())
                    .or_default()
                    .insert(*path_hash, Arc::clone(override_bytes));
                new_entry_count += 1;
            }
        }

        if new_entry_count > 0 {
            tracing::info!(
                "Routed {} new entries (not in any game WAD) via mod directory structure",
                new_entry_count
            );
        }
        tracing::info!(
            "Distributed overrides to {} affected WAD files",
            wad_overrides.len()
        );

        wad_overrides
    }

    /// Check that all WADs listed in the state actually exist on disk.
    fn validate_wads_exist(&self, state: &OverlayState) -> bool {
        for wad_path in state.wad_fingerprints.keys() {
            let full_path = self.overlay_root.join(wad_path);
            if !full_path.as_std_path().exists() {
                tracing::warn!("Expected overlay WAD missing: {}", full_path);
                return false;
            }
        }
        true
    }

    /// Remove all WAD files from the overlay directory.
    ///
    /// Deletes the `DATA/` subdirectory but preserves `overlay.json` and
    /// `game_index_cache.json` at the overlay root.
    fn clean_overlay_wads(&self) -> Result<()> {
        let data_dir = self.overlay_root.join("DATA");
        if data_dir.as_std_path().exists() {
            std::fs::remove_dir_all(data_dir.as_std_path())?;
        }
        Ok(())
    }

    /// Clean up empty parent directories after removing a stale WAD.
    ///
    /// Walks up from the given path toward `overlay_root`, removing empty
    /// directories. Stops at the overlay root or the first non-empty directory.
    fn cleanup_empty_parents(&self, path: &Utf8Path) {
        let mut current = path.parent();
        while let Some(dir) = current {
            if dir == self.overlay_root {
                break;
            }
            if dir.as_std_path().exists() {
                match std::fs::read_dir(dir.as_std_path()) {
                    Ok(mut entries) => {
                        if entries.next().is_some() {
                            break; // Not empty
                        }
                        let _ = std::fs::remove_dir(dir.as_std_path());
                    }
                    Err(_) => break,
                }
            }
            current = dir.parent();
        }
    }

    /// Emit a progress event if a callback was registered.
    fn emit_progress(&self, progress: OverlayProgress) {
        if let Some(callback) = &self.progress_callback {
            callback(progress);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::FsModContent;

    #[test]
    fn test_builder_creation() {
        let builder =
            OverlayBuilder::new(Utf8PathBuf::from("/game"), Utf8PathBuf::from("/overlay"));

        assert_eq!(builder.game_dir, Utf8PathBuf::from("/game"));
        assert_eq!(builder.overlay_root, Utf8PathBuf::from("/overlay"));
        assert_eq!(builder.enabled_mods.len(), 0);
    }

    #[test]
    fn test_set_enabled_mods() {
        let mut builder =
            OverlayBuilder::new(Utf8PathBuf::from("/game"), Utf8PathBuf::from("/overlay"));

        builder.set_enabled_mods(vec![EnabledMod {
            id: "mod1".to_string(),
            content: Box::new(FsModContent::new(Utf8PathBuf::from("/mods/mod1"))),
        }]);

        assert_eq!(builder.enabled_mods.len(), 1);
    }
}
