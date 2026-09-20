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

    /// The manifest declares a hashtable file the archive does not hold.
    #[error("The manifest declares {path}, but the archive holds no such entry")]
    MissingHashtable {
        /// The declared path, relative to the archive root.
        path: String,
    },

    /// A declared hashtable file does not fit the table grammar.
    #[error("Failed to read the hashtable at {path}")]
    Hashtable {
        /// The declared path, relative to the archive root.
        path: String,
        #[source]
        source: ltk_hashtable::HashtableReadError,
    },

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

/// A failed file operation and the file it was working on.
///
/// [`std::io::Error`] carries no path, so an error a user reads has to be told
/// which file gave out. This is what [`IoResultExt::at`] produces and what each
/// of the crate's error types converts from, which is what lets a call site
/// attach the path and leave the `?` to name the error:
///
/// ```text
/// let file = File::open(path.as_std_path()).at(path)?;
/// ```
///
/// A carrier rather than a generic constructor, because `?` chooses its
/// conversion from the function's return type alone: an `at` generic over the
/// error it builds leaves that type for inference to guess, and an error enum
/// with more than one `#[from]` gives it nothing to guess from.
#[derive(Debug)]
pub(crate) struct PathIo {
    /// The file that failed.
    pub(crate) path: Utf8PathBuf,
    /// How it failed.
    pub(crate) source: std::io::Error,
}

/// Name the file a failed operation was working on.
///
/// Without this it is the same closure at every call site -
/// `.map_err(|e| Error::io(path, e))?` - which buries the one word that differs
/// in six that never do, and gets copied wrong.
pub(crate) trait IoResultExt<T> {
    /// The value, or the failure with `path` attached.
    fn at(self, path: impl Into<Utf8PathBuf>) -> Result<T, PathIo>;
}

impl<T> IoResultExt<T> for Result<T, std::io::Error> {
    fn at(self, path: impl Into<Utf8PathBuf>) -> Result<T, PathIo> {
        self.map_err(|source| PathIo {
            path: path.into(),
            source,
        })
    }
}

/// A failed persist hands back the temporary file as well as the failure. Only
/// the failure is kept: the path worth reporting is the one the caller asked to
/// persist *to*, and dropping the returned handle deletes the temporary exactly
/// as dropping it anywhere else would.
impl<T> IoResultExt<T> for Result<T, tempfile::PersistError> {
    fn at(self, path: impl Into<Utf8PathBuf>) -> Result<T, PathIo> {
        self.map_err(|failed| PathIo {
            path: path.into(),
            source: failed.error,
        })
    }
}
