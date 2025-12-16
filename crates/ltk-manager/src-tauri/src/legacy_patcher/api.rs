use std::num::NonZeroU32;
use std::path::Path;
use std::time::Duration;

use libloading::Library;

use crate::utils::native::{cstr_to_str, str_to_cstr_utf16};

/// Bundled legacy patcher DLL (C implementation).
///
/// This must match the relative path inside `src-tauri/resources/`.
pub const PATCHER_DLL_NAME: &str = "legacy-patcher/cslol-dll.dll";

#[repr(u64)]
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum CSLogLevel {
    Error = 0x0,
    Warn = 0x8,
    Info = 0x10,
    Debug = 0x20,
    Trace = 0x1000,
}

#[derive(thiserror::Error, Debug)]
pub enum PatcherError {
    #[error("Failed to load patcher DLL: {0}")]
    LoadFailed(#[from] libloading::Error),
    #[error("Patcher DLL is missing required export '{symbol}': {source}")]
    MissingSymbol {
        symbol: &'static str,
        #[source]
        source: libloading::Error,
    },
    #[error("Failed to initialize cslol: {0}")]
    InitFailed(String),
    #[error("Failed to set patcher config: {0}")]
    SetConfigFailed(String),
    #[error("Failed to set patcher log level: {0}")]
    SetLogLevelFailed(String),
    #[error("Failed to set patcher log file: {0}")]
    SetLogFileFailed(String),
    #[error("Failed to hook: {0}")]
    HookFailed(String),
}

pub struct PatcherApi {
    /// Keeps the DLL loaded in memory while the function pointers are in use.
    #[allow(dead_code)]
    library: Library,
    cslol_init: unsafe extern "C" fn() -> *const u8,
    cslol_set_config: unsafe extern "C" fn(*const u16) -> *const u8,
    cslol_set_flags: unsafe extern "C" fn(u64) -> *const u8,
    cslol_set_log_level: unsafe extern "C" fn(CSLogLevel) -> *const u8,
    cslol_set_log_file: Option<unsafe extern "C" fn(*const u16) -> *const u8>,
    cslol_find: unsafe extern "C" fn() -> u32,
    cslol_sleep: Option<unsafe extern "C" fn(u32)>,
    cslol_hook: unsafe extern "C" fn(u32, u32, u32) -> *const u8,
    cslol_log_pull: Option<unsafe extern "C" fn() -> *const u8>,
}

impl PatcherApi {
    /// Load the patcher DLL from the given path.
    pub fn load(dll_path: &Path) -> Result<Self, PatcherError> {
        let lib = unsafe { Library::new(dll_path)? };

        unsafe {
            let cslol_set_log_file = lib
                .get::<unsafe extern "C" fn(*const u16) -> *const u8>(b"cslol_set_log_file")
                .ok()
                .map(|s| *s);
            let cslol_log_pull = lib
                .get::<unsafe extern "C" fn() -> *const u8>(b"cslol_log_pull")
                .ok()
                .map(|s| *s);
            let cslol_sleep = lib
                .get::<unsafe extern "C" fn(u32)>(b"cslol_sleep")
                .ok()
                .map(|s| *s);

            Ok(Self {
                cslol_init: *lib
                    .get(b"cslol_init")
                    .map_err(|e| PatcherError::MissingSymbol {
                        symbol: "cslol_init",
                        source: e,
                    })?,
                cslol_set_config: *lib
                    .get(b"cslol_set_config")
                    .map_err(|e| PatcherError::MissingSymbol {
                        symbol: "cslol_set_config",
                        source: e,
                    })?,
                cslol_set_flags: *lib
                    .get(b"cslol_set_flags")
                    .map_err(|e| PatcherError::MissingSymbol {
                        symbol: "cslol_set_flags",
                        source: e,
                    })?,
                cslol_set_log_level: *lib
                    .get(b"cslol_set_log_level")
                    .map_err(|e| PatcherError::MissingSymbol {
                        symbol: "cslol_set_log_level",
                        source: e,
                    })?,
                cslol_set_log_file,
                cslol_find: *lib
                    .get(b"cslol_find")
                    .map_err(|e| PatcherError::MissingSymbol {
                        symbol: "cslol_find",
                        source: e,
                    })?,
                cslol_sleep,
                cslol_hook: *lib
                    .get(b"cslol_hook")
                    .map_err(|e| PatcherError::MissingSymbol {
                        symbol: "cslol_hook",
                        source: e,
                    })?,
                cslol_log_pull,
                library: lib,
            })
        }
    }

    pub fn init(&self) -> Result<(), PatcherError> {
        unsafe {
            match cstr_to_str((self.cslol_init)()) {
                Some(err) => Err(PatcherError::InitFailed(err)),
                None => Ok(()),
            }
        }
    }

    pub fn set_config(&self, prefix: &str) -> Result<(), PatcherError> {
        unsafe {
            match cstr_to_str((self.cslol_set_config)(str_to_cstr_utf16(prefix).as_ptr())) {
                Some(err) => Err(PatcherError::SetConfigFailed(err)),
                None => Ok(()),
            }
        }
    }

    pub fn set_flags(&self, flags: u64) -> Result<(), PatcherError> {
        unsafe {
            match cstr_to_str((self.cslol_set_flags)(flags)) {
                Some(err) => Err(PatcherError::SetConfigFailed(err)),
                None => Ok(()),
            }
        }
    }

    pub fn set_log_level(&self, log_level: CSLogLevel) -> Result<(), PatcherError> {
        unsafe {
            match cstr_to_str((self.cslol_set_log_level)(log_level)) {
                Some(err) => Err(PatcherError::SetLogLevelFailed(err)),
                None => Ok(()),
            }
        }
    }

    pub fn set_log_file(&self, log_path: &str) -> Result<(), PatcherError> {
        let Some(set_log_file) = self.cslol_set_log_file else {
            return Ok(());
        };
        unsafe {
            match cstr_to_str(set_log_file(str_to_cstr_utf16(log_path).as_ptr())) {
                Some(err) => Err(PatcherError::SetLogFileFailed(err)),
                None => Ok(()),
            }
        }
    }

    pub fn find(&self) -> Option<NonZeroU32> {
        unsafe { NonZeroU32::new((self.cslol_find)()) }
    }

    pub fn sleep(&self, milliseconds: u32) {
        if let Some(sleep) = self.cslol_sleep {
            unsafe { sleep(milliseconds) }
        } else {
            std::thread::sleep(Duration::from_millis(milliseconds as u64));
        }
    }

    pub fn hook(&self, tid: u32, timeout_ms: u32, step_ms: u32) -> Result<(), PatcherError> {
        unsafe {
            match cstr_to_str((self.cslol_hook)(tid, timeout_ms, step_ms)) {
                Some(err) => Err(PatcherError::HookFailed(err)),
                None => Ok(()),
            }
        }
    }

    pub fn log_pull(&self) -> Option<String> {
        let Some(pull) = self.cslol_log_pull else {
            return None;
        };
        unsafe { cstr_to_str(pull()) }
    }
}


