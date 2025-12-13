use std::num::NonZeroU32;
use std::path::Path;

use libloading::Library;

use crate::utils::native::{cstr_to_str, str_to_cstr_utf16};

pub const PATCHER_DLL_NAME: &str = "cslol-dll.dll";

#[repr(u64)]
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
    #[error("Failed to initialize cslol: {0}")]
    InitFailed(String),
    #[error("Failed to set patcher config: {0}")]
    SetConfigFailed(String),
    #[error("Failed to set patcher log level: {0}")]
    SetLogLevelFailed(String),
    #[error("Failed to set patcher log file: {0}")]
    SetLogFileFailed(String),
}

pub struct PatcherApi {
    /// Keeps the DLL loaded in memory while the function pointers are in use.
    #[allow(dead_code)]
    library: Library,
    cslol_init: unsafe extern "C" fn() -> *const u8,
    cslol_set_config: unsafe extern "C" fn(*const u16) -> *const u8,
    cslol_set_log_level: unsafe extern "C" fn(CSLogLevel) -> *const u8,
    cslol_set_log_file: unsafe extern "C" fn(*const u16) -> *const u8,
    cslol_find: unsafe extern "C" fn() -> u32,
    cslol_sleep: unsafe extern "C" fn(u32),
    cslol_hook_count: unsafe extern "C" fn() -> usize,
    cslol_hook_begin: unsafe extern "C" fn(u32) -> usize,
    cslol_hook_continue: unsafe extern "C" fn(u32, usize) -> usize,
    cslol_hook_end: unsafe extern "C" fn(u32, usize) -> usize,
}

impl PatcherApi {
    /// Load the patcher DLL from the given path.
    pub fn load(dll_path: &Path) -> Result<Self, PatcherError> {
        let lib = unsafe { Library::new(dll_path)? };

        unsafe {
            Ok(Self {
                cslol_init: *lib.get(b"cslol_init")?,
                cslol_set_config: *lib.get(b"cslol_set_config")?,
                cslol_set_log_level: *lib.get(b"cslol_set_log_level")?,
                cslol_set_log_file: *lib.get(b"cslol_set_log_file")?,
                cslol_find: *lib.get(b"cslol_find")?,
                cslol_sleep: *lib.get(b"cslol_sleep")?,
                cslol_hook_count: *lib.get(b"cslol_hook_count")?,
                cslol_hook_begin: *lib.get(b"cslol_hook_begin")?,
                cslol_hook_continue: *lib.get(b"cslol_hook_continue")?,
                cslol_hook_end: *lib.get(b"cslol_hook_end")?,
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

    pub fn set_log_level(&self, log_level: CSLogLevel) -> Result<(), PatcherError> {
        unsafe {
            match cstr_to_str((self.cslol_set_log_level)(log_level)) {
                Some(err) => Err(PatcherError::SetLogLevelFailed(err)),
                None => Ok(()),
            }
        }
    }

    pub fn set_log_file(&self, log_path: &str) -> Result<(), PatcherError> {
        unsafe {
            match cstr_to_str((self.cslol_set_log_file)(
                str_to_cstr_utf16(log_path).as_ptr(),
            )) {
                Some(err) => Err(PatcherError::SetLogFileFailed(err)),
                None => Ok(()),
            }
        }
    }

    pub fn find(&self) -> Option<NonZeroU32> {
        unsafe { NonZeroU32::new((self.cslol_find)()) }
    }

    pub fn sleep(&self, milliseconds: u32) {
        unsafe { (self.cslol_sleep)(milliseconds) }
    }

    pub fn hook_count(&self) -> usize {
        unsafe { (self.cslol_hook_count)() }
    }

    pub fn hook_begin(&self, tid: u32) -> usize {
        unsafe { (self.cslol_hook_begin)(tid) }
    }

    pub fn hook_continue(&self, tid: u32, hook: usize) -> usize {
        unsafe { (self.cslol_hook_continue)(tid, hook) }
    }

    pub fn hook_end(&self, tid: u32, hook: usize) -> usize {
        unsafe { (self.cslol_hook_end)(tid, hook) }
    }
}
