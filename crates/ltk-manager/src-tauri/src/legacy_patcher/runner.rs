use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::api::{CSLogLevel, PatcherApi, PatcherError};

/// Default timeout for hook initialization (5 minutes in milliseconds).
pub const DEFAULT_HOOK_TIMEOUT_MS: u32 = 300_000;
/// Step interval for the hook loop (milliseconds).
pub const HOOK_STEP_MS: u32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum LegacyPatcherLoopError {
    #[error(transparent)]
    Patcher(#[from] PatcherError),
    #[error("Patcher stopped by request")]
    Stopped,
}

/// Run the legacy patcher loop.
///
/// This uses the simplified `cslol_hook` API that handles the hook lifecycle internally.
/// The legacy patcher continuously waits for game instances and patches them as they appear.
pub fn run_legacy_patcher_loop(
    dll_path: &Path,
    overlay_root: &str,
    log_file: Option<&str>,
    timeout_ms: u32,
    flags: u64,
    stop_flag: &AtomicBool,
) -> Result<(), LegacyPatcherLoopError> {
    let api = PatcherApi::load(dll_path)?;

    tracing::info!("Legacy patcher: set_flags({})", flags);
    api.set_flags(flags)?;
    tracing::info!("Legacy patcher: init()");
    api.init()?;
    tracing::info!("Legacy patcher: set_config(prefix='{}')", overlay_root);
    api.set_config(overlay_root)?;
    api.set_log_level(CSLogLevel::Trace)?;

    if let Some(log_path) = log_file {
        tracing::info!("Legacy patcher: set_log_file('{}')", log_path);
        api.set_log_file(log_path)?;
    }

    tracing::info!("Legacy patcher initialized, waiting for game...");

    loop {
        if stop_flag.load(Ordering::SeqCst) {
            return Err(LegacyPatcherLoopError::Stopped);
        }

        tracing::info!("Waiting for game to start (polling cslol_find)...");
        let mut last_wait_log = Instant::now();
        let tid = loop {
            if stop_flag.load(Ordering::SeqCst) {
                return Err(LegacyPatcherLoopError::Stopped);
            }
            match api.find() {
                Some(tid) => break tid.get(),
                None => {
                    if last_wait_log.elapsed() >= Duration::from_secs(5) {
                        tracing::info!("Still waiting for game process...");
                        last_wait_log = Instant::now();
                    }
                    api.sleep(100);
                }
            }
        };

        tracing::info!("Game found (thread id: {})", tid);

        tracing::info!(
            "Applying hook (timeout_ms={}, step_ms={})...",
            timeout_ms,
            HOOK_STEP_MS
        );
        api.hook(tid, timeout_ms, HOOK_STEP_MS)?;
        tracing::info!("Hook applied, waiting for game to exit...");

        while !stop_flag.load(Ordering::SeqCst) {
            match api.find() {
                Some(current) if current.get() == tid => {
                    while let Some(msg) = api.log_pull() {
                        tracing::info!("[cslol] {}", msg);
                    }
                    api.sleep(1000);
                }
                _ => break,
            }
        }

        tracing::info!("Game exited, returning to wait loop");
    }
}
