//! Errors returned when reading and writing Fantome data.
//!
//! Reading and writing have separate types: neither can produce the other's
//! failures, so a merged enum would force matches on unreachable cases.

use camino::Utf8PathBuf;
use thiserror::Error;

pub use crate::writer::FantomeWriteError;

/// Failure to extract from a Fantome archive.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FantomeExtractError {
    /// A file could not be written to the output directory.
    #[error("Failed to write {path}")]
    Write {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Reading from the archive failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The archive could not be read.
    #[error("Failed to read the archive")]
    Zip(#[from] zip::result::ZipError),

    /// `META/info.json` is not valid JSON, or does not describe a mod.
    #[error("Failed to parse META/info.json")]
    Json(#[from] serde_json::Error),

    /// A packed WAD inside the archive could not be extracted.
    #[error("Failed to extract a WAD")]
    Wad(#[from] ltk_wad::WadError),

    /// The archive has no `META/info.json`.
    #[error("Missing info.json metadata file")]
    MissingMetadata,

    /// An entry names a path that leaves the directory it would be extracted
    /// to, so extracting the archive would write outside it.
    ///
    /// The archive is refused whole, by [`FantomeReader::new`], before any of
    /// it is read: an archive carrying such an entry is not a mod that happens
    /// to have one bad file in it, and there is nothing in it worth unpacking
    /// the rest of.
    ///
    /// [`FantomeReader::new`]: crate::FantomeReader::new
    #[error("Archive entry escapes the output directory: {name}")]
    EscapingEntry {
        /// The entry name, as the archive spells it.
        name: String,
    },

    /// The caller's cancellation asked for the extraction to stop. Whatever
    /// had been written to the output directory is still there.
    #[error("The extraction was cancelled")]
    Cancelled,
}

impl FantomeExtractError {
    pub(crate) fn write(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Write {
            path: path.into(),
            source,
        }
    }
}
