//! Main overlay builder implementation.
//!
//! The [`OverlayBuilder`] orchestrates the full overlay build pipeline:
//! game indexing, override collection, cross-WAD distribution, and WAD patching.
//!
//! # Build Algorithm
//!
//! 1. Validate that `game_dir/DATA/FINAL` exists.
//! 2. Build a [`GameIndex`] from all `.wad.client` files in the game directory.
//! 3. Check the saved [`OverlayState`] — if the enabled mod list and game fingerprint
//!    match, and the overlay WAD files on disk are still valid, skip the build entirely.
//! 4. Wipe the overlay directory and iterate each [`EnabledMod`] in order:
//!    - Read its [`ModProject`](ltk_mod_project::ModProject) to get layer definitions.
//!    - For each layer (sorted by priority), list WAD targets and read override files.
//!    - Resolve each override file to a `u64` path hash and collect into a global map.
//!      Later mods overwrite earlier ones on hash collision (last-writer-wins).
//! 5. Distribute the collected overrides to all game WADs containing each hash
//!    (cross-WAD matching via [`GameIndex::find_wads_with_hash`]).
//! 6. For each affected WAD, call [`build_patched_wad`](crate::wad_builder::build_patched_wad)
//!    to produce a patched copy in the overlay directory.
//! 7. Persist the new [`OverlayState`] and emit a completion progress event.

use crate::content::ModContentProvider;
use crate::error::Result;
use crate::game_index::GameIndex;
use crate::state::OverlayState;
use crate::utils::resolve_chunk_hash;
use crate::wad_builder::build_patched_wad;
use camino::Utf8PathBuf;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

/// A mod to be included in the overlay build.
///
/// Each enabled mod contributes override files through its [`ModContentProvider`].
/// Mods are processed in the order they appear in the `enabled_mods` list passed to
/// [`OverlayBuilder::set_enabled_mods`]. When two mods override the same path hash,
/// the mod that appears *later* in the list wins (last-writer-wins).
pub struct EnabledMod {
    /// Unique identifier for the mod (used in state tracking and logging).
    pub id: String,
    /// Content provider for accessing mod metadata and override files.
    ///
    /// This can be backed by a filesystem directory, a `.modpkg` archive, a
    /// `.fantome` ZIP, or any other source that implements [`ModContentProvider`].
    pub content: Box<dyn ModContentProvider>,
    /// Global priority for conflict resolution (higher wins).
    ///
    /// Currently unused — ordering in the enabled list determines winner.
    pub priority: i32,
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
    /// WAD files reused from a previous build (not yet implemented).
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
    /// * `overlay_root` — Directory where patched WAD files will be written. This
    ///   directory is wiped and recreated on each full rebuild.
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
    /// Order matters: when two mods override the same chunk, the mod that appears
    /// later in this list wins.
    pub fn set_enabled_mods(&mut self, mods: Vec<EnabledMod>) {
        self.enabled_mods = mods;
    }

    /// Build the overlay, skipping the rebuild if the overlay state is still valid.
    ///
    /// Checks the saved `overlay.json` state file — if the enabled mod IDs and game
    /// fingerprint match, and the existing overlay WAD files can be mounted, the
    /// build is skipped entirely. Otherwise, performs a full rebuild.
    pub fn build(&mut self) -> Result<OverlayBuildResult> {
        let start_time = std::time::Instant::now();

        // TODO: Implement incremental build logic
        // For now, this is a placeholder that will call the full rebuild
        self.rebuild_all_internal()?;

        let build_time = start_time.elapsed();

        Ok(OverlayBuildResult {
            overlay_root: self.overlay_root.clone(),
            wads_built: Vec::new(),  // TODO
            wads_reused: Vec::new(), // TODO
            conflicts: Vec::new(),   // TODO
            build_time,
        })
    }

    /// Force a full rebuild, ignoring the saved overlay state.
    ///
    /// Use this when the user explicitly requests a rebuild or when you know
    /// the overlay is out of date for reasons the state file cannot track.
    pub fn rebuild_all(&mut self) -> Result<OverlayBuildResult> {
        let start_time = std::time::Instant::now();
        self.rebuild_all_internal()?;
        let build_time = start_time.elapsed();

        Ok(OverlayBuildResult {
            overlay_root: self.overlay_root.clone(),
            wads_built: Vec::new(),  // TODO
            wads_reused: Vec::new(), // TODO
            conflicts: Vec::new(),   // TODO
            build_time,
        })
    }

    /// Core build implementation. See module-level docs for the full algorithm.
    fn rebuild_all_internal(&mut self) -> Result<()> {
        tracing::info!("Building overlay...");
        tracing::info!("Game dir: {}", self.game_dir);
        tracing::info!("Overlay root: {}", self.overlay_root);
        tracing::info!("Enabled mods: {}", self.enabled_mods.len());

        // Emit start event
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

        // Index game files
        tracing::info!("Indexing game files...");
        let game_index = GameIndex::build(&self.game_dir)?;

        // Check if we can reuse the existing overlay
        let state_path = self.overlay_root.join("overlay.json");
        let enabled_ids: Vec<String> = self.enabled_mods.iter().map(|m| m.id.clone()).collect();

        if let Some(state) = OverlayState::load(&state_path)? {
            if state.matches(&enabled_ids, game_index.game_fingerprint()) {
                // Check if overlay outputs are still valid
                if self.validate_overlay_outputs()? {
                    tracing::info!("Overlay: reusing existing overlay (enabled mods unchanged)");
                    return Ok(());
                } else {
                    tracing::info!(
                        "Overlay: overlay state matched but outputs invalid; forcing rebuild"
                    );
                }
            }
        }

        tracing::info!("Overlay: rebuilding overlay...");

        // Clean overlay and rebuild
        if self.overlay_root.as_std_path().exists() {
            std::fs::remove_dir_all(self.overlay_root.as_std_path())?;
        }
        std::fs::create_dir_all(self.overlay_root.as_std_path())?;

        // Emit collecting stage
        self.emit_progress(OverlayProgress {
            stage: OverlayStage::CollectingOverrides,
            current_file: None,
            current: 0,
            total: 0,
        });

        // Collect ALL mod overrides as a flat map: path_hash -> bytes
        let mut all_overrides: HashMap<u64, Vec<u8>> = HashMap::new();

        for enabled_mod in &mut self.enabled_mods {
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
                        all_overrides.insert(path_hash, bytes);
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

        // Distribute overrides to ALL affected WADs using the game hash index
        let mut wad_overrides: BTreeMap<Utf8PathBuf, HashMap<u64, Vec<u8>>> = BTreeMap::new();
        for (path_hash, override_bytes) in &all_overrides {
            if let Some(wad_paths) = game_index.find_wads_with_hash(*path_hash) {
                for wad_path in wad_paths {
                    wad_overrides
                        .entry(wad_path.clone())
                        .or_default()
                        .insert(*path_hash, override_bytes.clone());
                }
            }
        }

        tracing::info!(
            "Distributed overrides to {} affected WAD files",
            wad_overrides.len()
        );

        // Build patched WADs for all affected game WADs
        let total_wads = wad_overrides.len() as u32;
        for (idx, (relative_game_path, overrides)) in wad_overrides.into_iter().enumerate() {
            let current_wad = (idx + 1) as u32;
            let wad_name = relative_game_path.file_name().unwrap_or("unknown");

            // Emit progress event
            self.emit_progress(OverlayProgress {
                stage: OverlayStage::PatchingWad,
                current_file: Some(wad_name.to_string()),
                current: current_wad,
                total: total_wads,
            });

            let src_wad_path = self.game_dir.join(&relative_game_path);
            let dst_wad_path = self.overlay_root.join(&relative_game_path);

            tracing::info!(
                "Writing patched WAD src={} dst={} overrides={}",
                src_wad_path,
                dst_wad_path,
                overrides.len()
            );

            build_patched_wad(&src_wad_path, &dst_wad_path, &overrides)?;
        }

        // Persist overlay state for reuse
        let state = OverlayState::new(enabled_ids, game_index.game_fingerprint());
        state.save(&state_path)?;

        // Emit completion event
        self.emit_progress(OverlayProgress {
            stage: OverlayStage::Complete,
            current_file: None,
            current: total_wads,
            total: total_wads,
        });

        Ok(())
    }

    /// Emit a progress event if a callback was registered.
    fn emit_progress(&self, progress: OverlayProgress) {
        if let Some(callback) = &self.progress_callback {
            callback(progress);
        }
    }

    /// Check that the overlay directory contains valid, mountable WAD files.
    ///
    /// Returns `false` if the `DATA/` subdirectory doesn't exist, contains no WAD
    /// files, or any WAD file fails to mount. This guards against corrupted overlays
    /// (e.g., from a crashed previous build).
    fn validate_overlay_outputs(&self) -> Result<bool> {
        use ltk_wad::Wad;

        let data_dir = self.overlay_root.join("DATA");
        if !data_dir.as_std_path().exists() {
            return Ok(false);
        }

        // Local traversal uses std::path since we only need it for File::open
        let mut stack = vec![data_dir.into_std_path_buf()];
        let mut wad_files = Vec::new();

        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    stack.push(path);
                    continue;
                }

                let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };

                if name.to_ascii_lowercase().ends_with(".wad.client") {
                    wad_files.push(path);
                }
            }
        }

        if wad_files.is_empty() {
            return Ok(false);
        }

        // Sanity check: overlay WADs should be mountable
        for wad_path in wad_files {
            let file = std::fs::File::open(&wad_path)?;
            Wad::mount(file)?;
        }

        Ok(true)
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
            priority: 0,
        }]);

        assert_eq!(builder.enabled_mods.len(), 1);
    }
}
