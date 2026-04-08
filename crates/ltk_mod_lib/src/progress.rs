pub use ltk_overlay::{OverlayProgress, OverlayStage};

/// Progress reporting trait for overlay and install operations.
///
/// Implement this trait to receive progress updates during long-running operations.
/// The Tauri app implements this via event emission; the CLI implements it via terminal output.
pub trait ProgressReporter: Send + Sync {
    /// Called during overlay build progress.
    fn on_overlay_progress(&self, progress: OverlayProgress);

    /// Called during mod installation progress (batch installs).
    fn on_install_progress(&self, progress: InstallProgress);
}

/// Install progress information for batch operations.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub current: usize,
    pub total: usize,
    pub current_file: String,
}

/// No-op progress reporter that discards all events.
pub struct NoOpReporter;

impl ProgressReporter for NoOpReporter {
    fn on_overlay_progress(&self, _progress: OverlayProgress) {}
    fn on_install_progress(&self, _progress: InstallProgress) {}
}
