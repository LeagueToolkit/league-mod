//! Error types for overlay operations.
//!
//! All fallible functions in this crate return [`Result<T>`], which uses [`Error`]
//! as the error type. External error types (`std::io::Error`, `serde_json::Error`,
//! WAD errors) are automatically converted via `From` impls.

use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

// TODO: `Other(String)` still carries this crate's own domain failures, 22 call
// sites spread over `fantome_content`, `wad_builder`, `resolve`, `strings` and
// `builder`, and nothing can be matched on it. Each needs a named variant. The
// enum is `#[non_exhaustive]`, so those can land without a breaking release;
// only removing `Other` itself breaks.
/// Errors that can occur during overlay building.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// A file or directory could not be read.
    #[error("Failed to read {path}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A file or directory could not be written.
    #[error("Failed to write {path}")]
    Write {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// An IO failure with no single path to blame.
    ///
    /// Prefer [`Read`](Self::Read) or [`Write`](Self::Write) wherever there is
    /// a path: `io::Error` never carries one.
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

    /// A ZIP archive (`.fantome` mod content) could not be opened or read.
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),

    /// One entry inside a ZIP archive could not be read.
    ///
    /// Separate from [`Zip`](Self::Zip) because `ZipError` never names the
    /// entry, and "file not found in archive" is useless without it.
    #[error("Failed to read `{entry}` from the archive")]
    ArchiveEntry {
        entry: String,
        #[source]
        source: zip::result::ZipError,
    },

    /// Error from the `ltk_modpkg` crate when reading `.modpkg` mod content.
    #[error(transparent)]
    Modpkg(#[from] ltk_modpkg::ModpkgError),

    /// A cache file could not be written.
    #[error("Failed to write the cache at {path}")]
    CacheWrite {
        path: Utf8PathBuf,
        #[source]
        source: CacheError,
    },

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

    /// A mod project's `.modignore` could not be loaded (used by
    /// [`FsModContent`](crate::FsModContent)). Failing beats filtering
    /// differently than packing would: what the overlay injects for testing
    /// must be what the package ships.
    #[error(transparent)]
    Ignore(#[from] ltk_mod_project::ModIgnoreError),

    /// Catch-all for errors from content providers and other sources.
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// A failure reading the file or directory at `path`.
    pub(crate) fn read(path: impl AsRef<Utf8Path>, source: std::io::Error) -> Self {
        Self::Read {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// A failure writing the file or directory at `path`.
    pub(crate) fn write(path: impl AsRef<Utf8Path>, source: std::io::Error) -> Self {
        Self::Write {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// A failure reading `entry` from a ZIP archive.
    ///
    /// Takes `impl Into<ZipError>` so an `io::Error` from decompressing an
    /// entry lands here too.
    pub(crate) fn archive_entry(
        entry: impl Into<String>,
        source: impl Into<zip::result::ZipError>,
    ) -> Self {
        Self::ArchiveEntry {
            entry: entry.into(),
            source: source.into(),
        }
    }

    /// A failure writing the cache file at `path`, whatever the step.
    pub(crate) fn cache_write(path: impl Into<Utf8PathBuf>, source: impl Into<CacheError>) -> Self {
        Self::CacheWrite {
            path: path.into(),
            source: source.into(),
        }
    }
}

/// Failure to access one of this crate's on-disk caches.
///
/// Separates a disk problem the user can act on (no space, no permission)
/// from an encoding failure, which is a bug to report.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CacheError {
    /// A filesystem failure on the cache file or its parent directory.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The cache could not be encoded.
    ///
    /// The source is boxed rather than named, so the encoding the caches
    /// happen to use is not part of this crate's public API.
    #[error("Failed to encode the cache")]
    Encode(#[source] Box<dyn std::error::Error + Send + Sync>),
}

// Kept as a `From` impl so `?` and `Error::cache_write` work across both cache
// writers. The conversion names `rmp_serde`, but the variant does not, so
// matching on an encode failure never forces a caller to depend on it.
impl From<rmp_serde::encode::Error> for CacheError {
    fn from(error: rmp_serde::encode::Error) -> Self {
        Self::Encode(Box::new(error))
    }
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

    /// `io::Error` never carries a path, so "the system cannot find the file
    /// specified" is unactionable on its own.
    #[test]
    fn file_errors_name_the_file() {
        let read = Error::read(
            "content/base/Aatrox.wad.client/skin0.bin",
            std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        let write = Error::write(
            "overlay/DATA/FINAL",
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );

        assert!(read.to_string().contains("skin0.bin"), "{read}");
        assert!(write.to_string().contains("overlay/DATA/FINAL"), "{write}");
    }

    /// The path goes in the message, so the source must not repeat it or a
    /// chain walker prints the file twice.
    #[test]
    fn file_errors_do_not_embed_their_source() {
        let error = Error::read("state/overlay.json", std::io::Error::other("inner detail"));
        let source = std::error::Error::source(&error).unwrap().to_string();

        assert!(!error.to_string().contains(&source), "{error}");
    }

    /// `ZipError::FileNotFound` renders without naming a file, so the variant
    /// that wraps it has to supply the entry itself.
    #[test]
    fn archive_entry_names_the_entry() {
        let error = Error::archive_entry("META/info.json", zip::result::ZipError::FileNotFound);

        assert!(error.to_string().contains("META/info.json"), "{error}");
    }

    /// The two cache writers share one variant, so the path is the only thing
    /// that says which cache failed.
    #[test]
    fn cache_write_names_the_file() {
        let error = Error::cache_write(
            Utf8PathBuf::from("cache/game_index.bin"),
            std::io::Error::other("disk full"),
        );

        assert!(
            error.to_string().contains("cache/game_index.bin"),
            "{error}"
        );
    }

    /// A full disk is worth surfacing to the user; a failed encode is a bug to
    /// report. The caller can only tell them apart if the variants differ.
    #[test]
    fn cache_write_separates_disk_from_encoding() {
        let error = Error::cache_write("cache/meta.bin", std::io::Error::other("disk full"));

        assert!(
            matches!(
                error,
                Error::CacheWrite {
                    source: CacheError::Io(_),
                    ..
                }
            ),
            "{error}"
        );
    }

    /// A caller can now tell a corrupt archive from a missing game file, which
    /// a stringly `Other` never allowed.
    #[test]
    fn wrapped_sources_stay_matchable() {
        let error = Error::from(zip::result::ZipError::FileNotFound);

        assert!(matches!(error, Error::Zip(_)), "{error}");
    }
}
