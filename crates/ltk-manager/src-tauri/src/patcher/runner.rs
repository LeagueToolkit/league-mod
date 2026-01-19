use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::api::{CSLogLevel, PatcherApi, PatcherError};

/// Default timeout for hook initialization (5 minutes in milliseconds).
pub const DEFAULT_HOOK_TIMEOUT_MS: u32 = 300_000;
/// Step interval for the hook loop (milliseconds).
pub const HOOK_STEP_MS: u32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum PatcherLoopError {
    #[error(transparent)]
    Patcher(#[from] PatcherError),
    #[error("Failed to begin hook")]
    HookFailed,
    #[error("Hook initialization timed out")]
    HookTimeout,
    #[error("Patcher stopped by request")]
    Stopped,
}

/// Run the new patcher loop with granular hook control.
///
/// This uses the `hook_begin`, `hook_continue`, `hook_end`, and `hook_count` API
/// for fine-grained control over the hook lifecycle.
pub fn run_patcher_loop(
    dll_path: &Path,
    config_path: &str,
    log_file: Option<&str>,
    timeout_ms: u32,
    stop_flag: &AtomicBool,
) -> Result<(), PatcherLoopError> {
    let api = PatcherApi::load(dll_path)?;

    api.init()?;
    api.set_config(config_path)?;
    api.set_log_level(CSLogLevel::Debug)?;

    if let Some(log_path) = log_file {
        api.set_log_file(log_path)?;
    }

    tracing::info!("Patcher initialized, waiting for League process...");

    let tid = loop {
        if stop_flag.load(Ordering::SeqCst) {
            return Err(PatcherLoopError::Stopped);
        }
        match api.find() {
            Some(tid) => break tid.get(),
            None => api.sleep(100),
        }
    };

    tracing::info!("Found League process, thread id: {}", tid);

    let count_before = api.hook_count();
    let hook = api.hook_begin(tid);
    if hook == 0 {
        return Err(PatcherLoopError::HookFailed);
    }

    let mut time_remaining = timeout_ms as i64;
    loop {
        if stop_flag.load(Ordering::SeqCst) {
            api.hook_end(tid, hook);
            return Err(PatcherLoopError::Stopped);
        }

        if time_remaining <= 0 {
            api.hook_end(tid, hook);
            return Err(PatcherLoopError::HookTimeout);
        }

        api.hook_continue(tid, hook);
        api.sleep(HOOK_STEP_MS);

        if api.hook_count() != count_before {
            tracing::info!("Hooks applied successfully");
            api.hook_end(tid, hook);
            break;
        }

        time_remaining -= HOOK_STEP_MS as i64;
    }

    tracing::info!("Hook session completed");
    Ok(())
}
