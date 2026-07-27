//! Error types for overlay operations.
//!
//! All fallible functions in this crate return [`Result<T>`], which uses [`Error`]
//! as the error type. External error types (`std::io::Error`, `serde_json::Error`,
//! WAD errors) are automatically converted via `From` impls.

use camino::Utf8PathBuf;
use thiserror::Error;

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

// TODO: `Other(String)` is this crate's de facto error type, with 39 call
// sites, and nothing can be matched on it. It needs replacing with real
// variants. The enum is `#[non_exhaustive]` so that can land without another
// breaking release.
/// Errors that can occur during overlay building.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Filesystem I/O failed (reading WADs, writing overlay, etc.).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Failed to parse or serialize JSON (overlay state, mod config).
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// Error from the `ltk_wad` crate when mounting or reading a WAD file.
    #[error(transparent)]
    WadError(#[from] ltk_wad::WadError),

    /// Error from the `ltk_wad` WAD builder when writing a patched WAD.
    #[error(transparent)]
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

    /// Catch-all for errors from content providers and other sources.
    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Consumers log these with `{e}` alone rather than walking the chain, so a
    /// pass-through variant must display its cause rather than a category name.
    #[test]
    fn pass_through_display_carries_the_cause() {
        let error = Error::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "overlay is locked",
        ));

        assert!(error.to_string().contains("overlay is locked"), "{error}");
    }
}
