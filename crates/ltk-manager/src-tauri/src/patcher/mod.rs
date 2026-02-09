pub mod api;
pub mod runner;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde::Serialize;

/// Current phase of the patcher lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PatcherPhase {
    Idle,
    Building,
    Patching,
}

pub struct PatcherState(pub Arc<Mutex<PatcherStateInner>>);

impl PatcherState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(PatcherStateInner::new())))
    }
}

impl Default for PatcherState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PatcherStateInner {
    /// Flag to signal the patcher thread to stop.
    pub stop_flag: Arc<AtomicBool>,
    /// Handle to the patcher thread.
    pub thread_handle: Option<JoinHandle<()>>,
    /// The config path used when starting.
    pub config_path: Option<String>,
    /// Current phase of the patcher lifecycle.
    pub phase: PatcherPhase,
}

impl PatcherStateInner {
    pub fn new() -> Self {
        Self {
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
            config_path: None,
            phase: PatcherPhase::Idle,
        }
    }

    pub fn is_running(&self) -> bool {
        self.thread_handle
            .as_ref()
            .map(|h| !h.is_finished())
            .unwrap_or(false)
    }
}

impl Default for PatcherStateInner {
    fn default() -> Self {
        Self::new()
    }
}
