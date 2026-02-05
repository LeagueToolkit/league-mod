//! Main overlay builder implementation.

use crate::content::ModContentProvider;
use crate::error::Result;
use crate::game_index::GameIndex;
use crate::state::OverlayState;
use crate::utils::resolve_chunk_hash;
use crate::wad_builder::build_patched_wad;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A mod to be included in the overlay.
pub struct EnabledMod {
    /// Unique identifier for the mod.
    pub id: String,
    /// Content provider for accessing mod files.
    pub content: Box<dyn ModContentProvider>,
    /// Global priority for conflict resolution (higher wins).
    pub priority: i32,
}

/// Progress information emitted during overlay building.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayProgress {
    /// Current stage of the build process.
    pub stage: OverlayStage,
    /// Current file being processed (if applicable).
    pub current_file: Option<String>,
    /// Current progress counter.
    pub current: u32,
    /// Total items to process.
    pub total: u32,
}

/// Stages of overlay building.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayStage {
    /// Indexing game files.
    Indexing,
    /// Collecting mod overrides.
    CollectingOverrides,
    /// Building/patching a WAD file.
    PatchingWad,
    /// Applying string overrides.
    ApplyingStringOverrides,
    /// Build complete.
    Complete,
}

/// Result of an overlay build operation.
#[derive(Debug)]
pub struct OverlayBuildResult {
    /// Root directory of the overlay.
    pub overlay_root: PathBuf,
    /// List of WAD files that were built.
    pub wads_built: Vec<PathBuf>,
    /// List of WAD files that were reused from previous build.
    pub wads_reused: Vec<PathBuf>,
    /// Detected conflicts between mods.
    pub conflicts: Vec<Conflict>,
    /// Total build time.
    pub build_time: Duration,
}

/// A conflict between multiple mods modifying the same file.
#[derive(Debug, Clone)]
pub struct Conflict {
    /// Hash of the conflicting file path.
    pub path_hash: u64,
    /// Human-readable path (if available).
    pub path: String,
    /// All mods that provide this file.
    pub contributing_mods: Vec<ModContribution>,
    /// The mod that won (based on priority/ordering).
    pub winner: String,
}

/// Information about a mod contributing a file.
#[derive(Debug, Clone)]
pub struct ModContribution {
    /// Mod ID.
    pub mod_id: String,
    /// Mod display name.
    pub mod_name: String,
    /// Layer name.
    pub layer: String,
    /// Layer priority.
    pub priority: i32,
    /// Installation order (index in enabled mods list).
    pub install_order: usize,
}

type ProgressCallback = Arc<dyn Fn(OverlayProgress) + Send + Sync>;

/// Main overlay builder.
pub struct OverlayBuilder {
    game_dir: PathBuf,
    overlay_root: PathBuf,
    enabled_mods: Vec<EnabledMod>,
    progress_callback: Option<ProgressCallback>,
}

impl OverlayBuilder {
    /// Create a new overlay builder.
    ///
    /// # Arguments
    ///
    /// * `game_dir` - Path to the League of Legends Game directory
    /// * `overlay_root` - Path where the overlay will be built
    pub fn new(game_dir: PathBuf, overlay_root: PathBuf) -> Self {
        Self {
            game_dir,
            overlay_root,
            enabled_mods: Vec::new(),
            progress_callback: None,
        }
    }

    /// Set a progress callback to receive build progress updates.
    pub fn with_progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(OverlayProgress) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    /// Set the list of enabled mods to include in the overlay.
    pub fn set_enabled_mods(&mut self, mods: Vec<EnabledMod>) {
        self.enabled_mods = mods;
    }

    /// Build the overlay, using incremental rebuild when possible.
    ///
    /// This will:
    /// 1. Index the game files
    /// 2. Collect overrides from enabled mods
    /// 3. Determine which WADs need rebuilding
    /// 4. Build only the changed WADs
    /// 5. Apply string overrides (if any)
    ///
    /// Returns information about what was built.
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

    /// Force a full rebuild of the overlay, ignoring any cached state.
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

    /// Internal implementation for full rebuild.
    fn rebuild_all_internal(&mut self) -> Result<()> {
        tracing::info!("Building overlay...");
        tracing::info!("Game dir: {}", self.game_dir.display());
        tracing::info!("Overlay root: {}", self.overlay_root.display());
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
        if !data_final_dir.exists() {
            return Err(format!(
                "League path does not contain Game/DATA/FINAL. Game dir: '{}'",
                self.game_dir.display()
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
        if self.overlay_root.exists() {
            std::fs::remove_dir_all(&self.overlay_root)?;
        }
        std::fs::create_dir_all(&self.overlay_root)?;

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
                        .map_err(|_| {
                            format!(
                                "WAD path is not under Game/: {}",
                                original_wad_path.display()
                            )
                        })?
                        .to_path_buf();

                    tracing::info!(
                        "WAD='{}' resolved original={} relative={}",
                        wad_name,
                        original_wad_path.display(),
                        relative_game_path.display()
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

        // Distribute overrides to ALL affected WADs using the game hash index
        let mut wad_overrides: BTreeMap<PathBuf, HashMap<u64, Vec<u8>>> = BTreeMap::new();
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
            let wad_name = relative_game_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

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
                src_wad_path.display(),
                dst_wad_path.display(),
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

    /// Emit progress update if a callback is set.
    fn emit_progress(&self, progress: OverlayProgress) {
        if let Some(callback) = &self.progress_callback {
            callback(progress);
        }
    }

    /// Validate that overlay outputs exist and are valid.
    fn validate_overlay_outputs(&self) -> Result<bool> {
        use ltk_wad::Wad;

        let data_dir = self.overlay_root.join("DATA");
        if !data_dir.exists() {
            return Ok(false);
        }

        let mut stack = vec![data_dir];
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
        let builder = OverlayBuilder::new(PathBuf::from("/game"), PathBuf::from("/overlay"));

        assert_eq!(builder.game_dir, PathBuf::from("/game"));
        assert_eq!(builder.overlay_root, PathBuf::from("/overlay"));
        assert_eq!(builder.enabled_mods.len(), 0);
    }

    #[test]
    fn test_set_enabled_mods() {
        let mut builder = OverlayBuilder::new(PathBuf::from("/game"), PathBuf::from("/overlay"));

        builder.set_enabled_mods(vec![EnabledMod {
            id: "mod1".to_string(),
            content: Box::new(FsModContent::new(PathBuf::from("/mods/mod1"))),
            priority: 0,
        }]);

        assert_eq!(builder.enabled_mods.len(), 1);
    }
}
