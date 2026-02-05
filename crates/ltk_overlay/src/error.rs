//! Error types for overlay building.

use std::path::PathBuf;
use thiserror::Error;

/// Result type for overlay operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during overlay building.
#[derive(Error, Debug)]
pub enum Error {
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// WAD error.
    #[error("WAD error: {0}")]
    WadError(#[from] ltk_wad::WadError),

    /// WAD builder error.
    #[error("WAD builder error: {0}")]
    WadBuilderError(#[from] ltk_wad::WadBuilderError),

    /// Game directory not found or invalid.
    #[error("Invalid game directory: {0}")]
    InvalidGameDir(String),

    /// WAD file not found.
    #[error("WAD file not found: {0}")]
    WadNotFound(PathBuf),

    /// Multiple WAD candidates found (ambiguous).
    #[error("Ambiguous WAD '{name}': found {count} candidates")]
    AmbiguousWad { name: String, count: usize },

    /// Mod directory not found or invalid.
    #[error("Invalid mod directory: {0}")]
    InvalidModDir(PathBuf),

    /// Mod config.json not found or invalid.
    #[error("Invalid mod config: {0}")]
    InvalidModConfig(String),

    /// Overlay output validation failed.
    #[error("Overlay validation failed: {0}")]
    ValidationFailed(String),

    /// Compression error.
    #[error("Compression error: {0}")]
    Compression(String),

    /// Other errors.
    #[error("{0}")]
    Other(String),
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Other(s)
    }
}
