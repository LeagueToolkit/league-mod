//! Main overlay builder implementation (two-pass architecture).
//!
//! The [`OverlayBuilder`] orchestrates the full overlay build pipeline:
//! game indexing, override metadata collection, cross-WAD distribution, and WAD patching.
//!
//! # Two-Pass Build Algorithm
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
//! 4. **Pass 1**: Collect lightweight override metadata (hashes, sizes, source
//!    locations) from all mods. Uses a persistent metadata cache to skip
//!    unchanged mods entirely. Bytes are hashed and then dropped.
//! 5. Distribute override hashes to WADs, partition into rebuild/reuse sets.
//! 6. **Pass 2**: Re-read override bytes only for WADs that need rebuilding.
//!    Call [`build_patched_wad`](crate::wad_builder::build_patched_wad).
//! 7. Persist the new [`OverlayState`] with per-WAD fingerprints.

mod metadata;
mod resolve;

use crate::content::ModContentProvider;
use crate::error::{Error, Result};
use crate::game_index::GameIndex;
use crate::state::OverlayState;
use camino::{Utf8Path, Utf8PathBuf};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// Shared byte buffer for override data distributed across multiple WADs.
///
/// A single mod override may be distributed to many WADs (cross-WAD matching),
/// so we share the bytes via `Arc` instead of cloning.
pub(crate) type SharedBytes = Arc<[u8]>;

/// Where an override can be re-read from in pass 2.
#[derive(Clone, Debug)]
pub(crate) enum OverrideSource {
    /// Override from a WAD directory inside a layer.
    LayerWad {
        mod_id: String,
        layer: String,
        wad_name: String,
        rel_path: Utf8PathBuf,
    },
    /// Override from the raw content directory.
    Raw {
        mod_id: String,
        rel_path: Utf8PathBuf,
    },
}

/// Lightweight metadata collected in pass 1 (no byte data).
#[derive(Clone, Debug)]
pub struct OverrideMeta {
    pub content_hash: u64,
    pub uncompressed_size: usize,
    pub(crate) source: OverrideSource,
    /// WAD path to route this override to when the game index has no match
    /// (i.e. the override adds a new chunk not present in any game WAD).
    /// Derived from the mod's directory structure during collection.
    pub(crate) fallback_wad: Option<Utf8PathBuf>,
}

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
    /// Optional set of layer names to include. When `Some`, only layers whose
    /// names are in this set will be processed during overlay building. When
    /// `None`, all layers are included (backward-compatible default).
    pub enabled_layers: Option<HashSet<String>>,
}

/// The name of the base layer that is always included regardless of
/// `enabled_layers` configuration. Every mod has a base layer at priority 0.
pub const BASE_LAYER_NAME: &str = "base";

impl EnabledMod {
    /// Compute a cache fingerprint that accounts for both content changes and
    /// the current `enabled_layers` selection.
    ///
    /// Returns `None` if the underlying content provider cannot compute a
    /// fingerprint (the metadata cache will be skipped for this mod).
    pub fn cache_fingerprint(&self) -> Option<u64> {
        use xxhash_rust::xxh3::xxh3_64;

        let base_fp = self.content.content_fingerprint().unwrap_or_else(|e| {
            tracing::warn!(
                "Failed to compute content fingerprint for mod '{}': {}",
                self.id,
                e
            );
            None
        })?;

        Some(match &self.enabled_layers {
            Some(layers) => {
                // Exclude BASE_LAYER_NAME before hashing — it's always implicitly
                // active via is_layer_active, so {"extras"} and {"base","extras"}
                // should produce the same fingerprint.
                let mut sorted: Vec<&str> = layers
                    .iter()
                    .map(|s| s.as_str())
                    .filter(|&s| s != BASE_LAYER_NAME)
                    .collect();
                sorted.sort_unstable();

                // Encode as "fp\0layer1\0layer2\0..." and hash the whole buffer.
                let mut buf = format!("{base_fp}");
                for layer in &sorted {
                    buf.push('\0');
                    buf.push_str(layer);
                }

                xxh3_64(buf.as_bytes())
            }
            None => base_fp,
        })
    }

    /// Returns whether the given layer name should be processed for this mod.
    ///
    /// A layer is active when:
    /// - `enabled_layers` is `None` (all layers enabled), OR
    /// - The layer is the base layer ([`BASE_LAYER_NAME`]), OR
    /// - The layer is explicitly listed in `enabled_layers`.
    pub fn is_layer_active(&self, layer_name: &str) -> bool {
        match &self.enabled_layers {
            None => true,
            Some(allowed) => layer_name == BASE_LAYER_NAME || allowed.contains(layer_name),
        }
    }
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

impl OverlayProgress {
    /// Create a stage-only progress event with no file or counter information.
    fn stage(stage: OverlayStage) -> Self {
        Self {
            stage,
            current_file: None,
            current: 0,
            total: 0,
        }
    }
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

/// Per-mod summary of which game WAD files a mod's overrides affect.
///
/// Computed independently for each mod (i.e. before cross-mod merging), so
/// the result is **load-order independent**: it represents the mod's full
/// potential WAD footprint regardless of which other mods are enabled
/// alongside it.
///
/// Reports are produced in two ways:
///
/// 1. As a side effect of [`OverlayBuilder::build`], which captures one report
///    per enabled mod and exposes them via
///    [`take_mod_wad_reports`](OverlayBuilder::take_mod_wad_reports).
/// 2. On demand via [`OverlayBuilder::analyze_single_mod`], which runs the
///    same per-mod analysis without writing any overlay files.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModWadReport {
    /// Mod identifier (matches [`EnabledMod::id`]).
    pub mod_id: String,
    /// Game-relative WAD paths the mod's overrides land in, sorted and deduplicated.
    pub affected_wads: Vec<Utf8PathBuf>,
    /// Total number of override entries the mod contributes across all layers.
    pub override_count: u32,
    /// Content fingerprint of the mod at the time the report was computed,
    /// from [`ModContentProvider::content_fingerprint`].
    pub content_fingerprint: Option<u64>,
    /// Game index fingerprint at the time the report was computed.
    pub game_index_fingerprint: u64,
}

impl ModWadReport {
    /// Build a report from one mod's collected override metadata.
    ///
    /// For each override hash, the matching set of game WADs is looked up via
    /// [`GameIndex::find_wads_with_hash`]. Hashes that don't appear in any
    /// game WAD fall back to the per-override `fallback_wad` recorded during
    /// metadata collection (i.e. the WAD the mod's directory structure
    /// pointed at).
    pub(crate) fn from_meta(
        mod_id: String,
        mod_meta: &HashMap<u64, OverrideMeta>,
        content_fingerprint: Option<u64>,
        game_index: &GameIndex,
    ) -> Self {
        let mut wads: std::collections::BTreeSet<Utf8PathBuf> = std::collections::BTreeSet::new();
        for (path_hash, meta) in mod_meta {
            if let Some(wad_paths) = game_index.find_wads_with_hash(*path_hash) {
                for wp in wad_paths {
                    wads.insert(wp.clone());
                }
            } else if let Some(fallback) = &meta.fallback_wad {
                wads.insert(fallback.clone());
            }
        }

        Self {
            mod_id,
            affected_wads: wads.into_iter().collect(),
            override_count: mod_meta.len() as u32,
            content_fingerprint,
            game_index_fingerprint: game_index.game_fingerprint(),
        }
    }
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

pub(crate) type ProgressCallback = Arc<dyn Fn(OverlayProgress) + Send + Sync>;

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
    /// Directory for `overlay.json` and `game_index.bin`
    /// (typically the parent profile directory, e.g. `profiles/default/`).
    state_dir: Utf8PathBuf,
    enabled_mods: Vec<EnabledMod>,
    blocked_wads: HashSet<String>,
    progress_callback: Option<ProgressCallback>,
    /// Per-mod WAD reports captured during the most recent successful
    /// [`build`](Self::build), drained via [`take_mod_wad_reports`](Self::take_mod_wad_reports).
    last_mod_wad_reports: Vec<ModWadReport>,
}

impl OverlayBuilder {
    /// Create a new overlay builder.
    ///
    /// # Arguments
    ///
    /// * `game_dir` — Path to the League of Legends `Game/` directory. Must contain
    ///   a `DATA/FINAL` subdirectory with `.wad.client` files.
    /// * `overlay_root` — Directory where patched WAD files will be written
    ///   (e.g. `profiles/default/overlay`).
    /// * `state_dir` — Directory for `overlay.json` and `game_index.bin`
    ///   (e.g. the profile folder `profiles/default/`).
    pub fn new(game_dir: Utf8PathBuf, overlay_root: Utf8PathBuf, state_dir: Utf8PathBuf) -> Self {
        Self {
            game_dir,
            overlay_root,
            state_dir,
            enabled_mods: Vec::new(),
            blocked_wads: HashSet::new(),
            progress_callback: None,
            last_mod_wad_reports: Vec::new(),
        }
    }

    /// Drain the per-mod WAD reports captured during the most recent successful
    /// [`build`](Self::build).
    ///
    /// Each report describes one mod's full potential WAD footprint, computed
    /// independently of load order. Returns an empty vector if no build has
    /// run yet, or if the previous build short-circuited (e.g. exact-match skip).
    pub fn take_mod_wad_reports(&mut self) -> Vec<ModWadReport> {
        std::mem::take(&mut self.last_mod_wad_reports)
    }

    /// Analyze a single mod's WAD footprint without building or modifying any
    /// overlay artifacts.
    ///
    /// Loads (or builds) the [`GameIndex`] from `game_dir` using the cache at
    /// `state_dir/game_index.bin`, then runs the same per-mod metadata
    /// collection used during a full build and resolves it into a
    /// [`ModWadReport`]. Safe to call concurrently with [`build`](Self::build)
    /// because it neither writes overlay state nor takes any locks held by
    /// the build pipeline.
    pub fn analyze_single_mod(
        game_dir: &Utf8Path,
        state_dir: &Utf8Path,
        enabled_mod: &mut EnabledMod,
    ) -> Result<ModWadReport> {
        let data_final_dir = game_dir.join("DATA").join("FINAL");
        if !data_final_dir.as_std_path().exists() {
            return Err(format!(
                "League path does not contain Game/DATA/FINAL. Game dir: '{}'",
                game_dir
            )
            .into());
        }

        std::fs::create_dir_all(state_dir.as_std_path())?;
        let cache_path = state_dir.join("game_index.bin");
        let game_index = GameIndex::load_or_build(game_dir, &cache_path)?;

        let fingerprint = enabled_mod.cache_fingerprint();
        let mod_meta = metadata::collect_single_mod_metadata(enabled_mod, &game_index, game_dir)?;

        Ok(ModWadReport::from_meta(
            enabled_mod.id.clone(),
            &mod_meta,
            fingerprint,
            &game_index,
        ))
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

    /// Set WAD filenames to block from patching.
    ///
    /// Filenames are automatically lowercased for case-insensitive matching.
    pub fn with_blocked_wads(mut self, wads: Vec<String>) -> Self {
        self.blocked_wads = wads.into_iter().map(|w| w.to_ascii_lowercase()).collect();
        self
    }

    /// Set the ordered list of mods to include in the overlay.
    ///
    /// Order matters: the first mod in the list (index 0) has the highest priority.
    /// When two mods override the same chunk, the mod closer to the front wins.
    pub fn set_enabled_mods(&mut self, mods: Vec<EnabledMod>) {
        self.enabled_mods = mods;
    }

    /// Build the overlay with incremental rebuild support (two-pass).
    ///
    /// 1. If the overlay state matches exactly and all WAD files exist → skip.
    /// 2. If the game fingerprint and state version match → incremental rebuild
    ///    (only re-patch WADs whose override fingerprint changed).
    /// 3. Otherwise → full rebuild (wipe and rebuild everything).
    pub fn build(&mut self) -> Result<OverlayBuildResult> {
        let start_time = std::time::Instant::now();

        let effective_blocked = self.effective_blocked_wads();

        tracing::info!("Building overlay...");
        tracing::debug!("Game dir: {}", self.game_dir);
        tracing::debug!("Overlay root: {}", self.overlay_root);
        tracing::debug!("Enabled mods: {}", self.enabled_mods.len());
        tracing::debug!("Blocked WADs: {:?}", effective_blocked);

        self.emit_progress(OverlayProgress::stage(OverlayStage::Indexing));

        let data_final_dir = self.game_dir.join("DATA").join("FINAL");
        if !data_final_dir.as_std_path().exists() {
            return Err(format!(
                "League path does not contain Game/DATA/FINAL. Game dir: '{}'",
                self.game_dir
            )
            .into());
        }

        std::fs::create_dir_all(self.overlay_root.as_std_path())?;
        std::fs::create_dir_all(self.state_dir.as_std_path())?;

        let cache_path = self.state_dir.join("game_index.bin");
        let game_index = GameIndex::load_or_build(&self.game_dir, &cache_path)?;

        // Load previous state
        let state_path = self.state_dir.join("overlay.json");
        let enabled_ids: Vec<String> = self.enabled_mods.iter().map(|m| m.id.clone()).collect();
        let prev_state = OverlayState::load(&state_path)?;

        // --- Handle empty mod list ---
        if self.enabled_mods.is_empty() {
            tracing::info!("Overlay: no enabled mods, cleaning overlay");
            self.clean_overlay_wads()?;
            let state = OverlayState::new(
                Vec::new(),
                game_index.game_fingerprint(),
                effective_blocked.clone(),
                BTreeMap::new(),
            );
            state.save(&state_path)?;
            self.emit_progress(OverlayProgress::stage(OverlayStage::Complete));
            return Ok(OverlayBuildResult {
                overlay_root: self.overlay_root.clone(),
                wads_built: Vec::new(),
                wads_reused: Vec::new(),
                conflicts: Vec::new(),
                build_time: start_time.elapsed(),
            });
        }

        if let Some(ref state) = prev_state {
            if state.matches(
                &enabled_ids,
                game_index.game_fingerprint(),
                &effective_blocked,
            ) {
                if self.validate_wads_exist(state) {
                    tracing::info!("Overlay: exact match, skipping build");
                    self.emit_progress(OverlayProgress::stage(OverlayStage::Complete));
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
                } else {
                    tracing::info!(
                        "Overlay: state matched but some WADs missing, doing incremental repair"
                    );
                }
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

        self.emit_progress(OverlayProgress::stage(OverlayStage::CollectingOverrides));

        let (all_meta, mod_wad_reports) = self.collect_all_override_metadata(&game_index)?;
        self.last_mod_wad_reports = mod_wad_reports;

        let mut wad_hash_sets = self.distribute_override_hashes(&all_meta, &game_index);

        wad_hash_sets.retain(|path, _| {
            let blocked = self.is_wad_blocked(path);
            if blocked {
                tracing::info!("Blocked WAD from patching: {}", path);
            }
            !blocked
        });

        let (wads_to_build, wads_to_reuse, new_wad_fingerprints) =
            self.partition_wads_from_meta(&wad_hash_sets, &all_meta, &prev_state, can_incremental);

        if can_incremental {
            if let Some(ref state) = prev_state {
                self.clean_stale_wads(state, &new_wad_fingerprints)?;
            }
        }

        let wad_overrides =
            self.resolve_overrides_for_wads(&wads_to_build, &wad_hash_sets, &all_meta)?;

        let built_paths = self.patch_wads_parallel(wads_to_build, wad_overrides)?;

        let reused_paths: Vec<Utf8PathBuf> = wads_to_reuse
            .iter()
            .map(|p| self.overlay_root.join(p))
            .collect();

        let state = OverlayState::new(
            enabled_ids,
            game_index.game_fingerprint(),
            effective_blocked,
            new_wad_fingerprints,
        );
        state.save(&state_path)?;

        let total_wads = built_paths.len() as u32;
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
            conflicts: Vec::new(),
            build_time: start_time.elapsed(),
        })
    }

    /// Force a full rebuild, ignoring the saved overlay state.
    ///
    /// Use this when the user explicitly requests a rebuild or when you know
    /// the overlay is out of date for reasons the state file cannot track.
    pub fn rebuild_all(&mut self) -> Result<OverlayBuildResult> {
        // Remove previous state so build() sees no match
        let state_path = self.state_dir.join("overlay.json");
        if state_path.as_std_path().exists() {
            std::fs::remove_file(state_path.as_std_path())?;
        }
        self.clean_overlay_wads()?;
        self.build()
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

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

    /// Remove overlay WADs that were in the previous state but are no longer needed.
    ///
    /// Also cleans up empty parent directories left behind.
    fn clean_stale_wads(
        &self,
        prev_state: &OverlayState,
        new_wad_fingerprints: &BTreeMap<String, u64>,
    ) -> Result<()> {
        for old_wad_path in prev_state.wad_fingerprints.keys() {
            if !new_wad_fingerprints.contains_key(old_wad_path) {
                let stale_path = self.overlay_root.join(old_wad_path);
                if stale_path.as_std_path().exists() {
                    tracing::info!("Removing stale WAD: {}", stale_path);
                    std::fs::remove_file(stale_path.as_std_path())?;
                }
                self.cleanup_empty_parents(&stale_path);
            }
        }
        Ok(())
    }

    /// Remove all WAD files from the overlay directory.
    fn clean_overlay_wads(&self) -> Result<()> {
        let data_dir = self.overlay_root.join("DATA");
        if data_dir.as_std_path().exists() {
            std::fs::remove_dir_all(data_dir.as_std_path())?;
        }
        Ok(())
    }

    /// Clean up empty parent directories after removing a stale WAD.
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
                            break;
                        }
                        let _ = std::fs::remove_dir(dir.as_std_path());
                    }
                    Err(_) => break,
                }
            }
            current = dir.parent();
        }
    }

    /// Compute the effective blocklist from user-configured blocked WADs.
    fn effective_blocked_wads(&self) -> Vec<String> {
        let mut all: Vec<String> = self.blocked_wads.iter().cloned().collect();
        all.sort();
        all.dedup();
        all
    }

    /// Check if a WAD path is blocked from patching.
    fn is_wad_blocked(&self, wad_path: &Utf8Path) -> bool {
        let filename = wad_path.file_name().unwrap_or("").to_ascii_lowercase();
        self.blocked_wads.contains(&filename)
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
        let builder = OverlayBuilder::new(
            Utf8PathBuf::from("/game"),
            Utf8PathBuf::from("/profile/overlay"),
            Utf8PathBuf::from("/profile"),
        );

        assert_eq!(builder.game_dir, Utf8PathBuf::from("/game"));
        assert_eq!(builder.overlay_root, Utf8PathBuf::from("/profile/overlay"));
        assert_eq!(builder.state_dir, Utf8PathBuf::from("/profile"));
        assert_eq!(builder.enabled_mods.len(), 0);
    }

    #[test]
    fn test_set_enabled_mods() {
        let mut builder = OverlayBuilder::new(
            Utf8PathBuf::from("/game"),
            Utf8PathBuf::from("/profile/overlay"),
            Utf8PathBuf::from("/profile"),
        );

        builder.set_enabled_mods(vec![EnabledMod {
            id: "mod1".to_string(),
            content: Box::new(FsModContent::new(Utf8PathBuf::from("/mods/mod1"))),
            enabled_layers: None,
        }]);

        assert_eq!(builder.enabled_mods.len(), 1);
    }

    #[test]
    fn test_override_meta_types() {
        let meta = OverrideMeta {
            content_hash: 0x1234,
            uncompressed_size: 100,
            source: OverrideSource::LayerWad {
                mod_id: "test-mod".to_string(),
                layer: "base".to_string(),
                wad_name: "Test.wad.client".to_string(),
                rel_path: Utf8PathBuf::from("data/file.bin"),
            },
            fallback_wad: None,
        };
        assert_eq!(meta.content_hash, 0x1234);
        assert_eq!(meta.uncompressed_size, 100);
    }
}
