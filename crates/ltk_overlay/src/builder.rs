//! Main overlay builder implementation.

use crate::error::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A mod to be included in the overlay.
#[derive(Debug, Clone)]
pub struct EnabledMod {
    /// Unique identifier for the mod.
    pub id: String,
    /// Directory containing the mod project (with mod.config.json and content/).
    pub mod_dir: PathBuf,
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
        // TODO: Port logic from ltk-manager/src-tauri/src/overlay/mod.rs
        // This is a placeholder
        tracing::info!("Building overlay...");
        tracing::info!("Game dir: {}", self.game_dir.display());
        tracing::info!("Overlay root: {}", self.overlay_root.display());
        tracing::info!("Enabled mods: {}", self.enabled_mods.len());

        Ok(())
    }

    /// Emit progress update if a callback is set.
    #[allow(dead_code)] // Used in full implementation
    fn emit_progress(&self, progress: OverlayProgress) {
        if let Some(callback) = &self.progress_callback {
            callback(progress);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            mod_dir: PathBuf::from("/mods/mod1"),
            priority: 0,
        }]);

        assert_eq!(builder.enabled_mods.len(), 1);
    }
}
