//! Error types for overlay operations.
//!
//! All fallible functions in this crate return [`Result<T>`], which uses [`Error`]
//! as the error type. External error types (`std::io::Error`, `serde_json::Error`,
//! WAD errors) are automatically converted via `From` impls.

use camino::Utf8PathBuf;
use thiserror::Error;

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during overlay building.
#[derive(Error, Debug)]
pub enum Error {
    /// Filesystem I/O failed (reading WADs, writing overlay, etc.).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse or serialize JSON (overlay state, mod config).
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Error from the `ltk_wad` crate when mounting or reading a WAD file.
    #[error("WAD error: {0}")]
    WadError(#[from] ltk_wad::WadError),

    /// Error from the `ltk_wad` WAD builder when writing a patched WAD.
    #[error("WAD builder error: {0}")]
    WadBuilderError(#[from] ltk_wad::WadBuilderError),

    /// The game directory does not contain the expected `DATA/FINAL` structure.
    #[error("Invalid game directory: {0}")]
    InvalidGameDir(String),

    /// A mod references a WAD file that doesn't exist in the game directory.
    #[error("WAD file not found: {0}")]
    WadNotFound(Utf8PathBuf),

    /// A WAD filename matches multiple files in the game directory.
    #[error("Ambiguous WAD '{name}': found {count} candidates")]
    AmbiguousWad { name: String, count: usize },

    /// A mod directory is missing or inaccessible (used by [`FsModContent`](crate::FsModContent)).
    #[error("Invalid mod directory: {0}")]
    InvalidModDir(Utf8PathBuf),

    /// A mod's `mod.config.json` is missing or malformed.
    #[error("Invalid mod config: {0}")]
    InvalidModConfig(String),

    /// The overlay directory exists but its WAD files are corrupted.
    #[error("Overlay validation failed: {0}")]
    ValidationFailed(String),

    /// Zstd compression or decompression failed.
    #[error("Compression error: {0}")]
    Compression(String),

    /// Catch-all for errors from content providers and other sources.
    #[error("{0}")]
    Other(String),
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Other(s)
    }
}
