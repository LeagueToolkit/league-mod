/// Library-specific error types with no Tauri dependency.
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Modpkg error: {0}")]
    Modpkg(#[from] ltk_modpkg::error::ModpkgError),

    #[error("Fantome error: {0}")]
    Fantome(String),

    #[error("Mod not found: {0}")]
    ModNotFound(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    #[error("Storage locked by another process")]
    StorageLocked,

    #[error("Library index is corrupt: {0}")]
    IndexCorrupt(String),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Overlay build failed: {0}")]
    OverlayFailed(String),

    #[error("{0}")]
    Other(String),
}

pub type LibraryResult<T> = Result<T, LibraryError>;
