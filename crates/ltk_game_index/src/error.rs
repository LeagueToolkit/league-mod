//! Errors of building an index.

use camino::Utf8PathBuf;

/// A chunk index build that did not run.
///
/// Per-archive read failures never fail a build. They are recorded as
/// [`SkippedArchive`](crate::SkippedArchive)s.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// The game directory has no `DATA/FINAL`.
    #[error("{path} is not a League installation: it has no DATA/FINAL directory")]
    MissingDataFinal {
        /// The game directory.
        path: Utf8PathBuf,
    },
    /// The walk over `DATA/FINAL` failed.
    #[error("cannot enumerate archives under {path}")]
    Enumerate {
        /// The directory walked.
        path: Utf8PathBuf,
        /// The failure.
        #[source]
        source: std::io::Error,
    },
    /// An explicit archive does not lie under the root it is named against.
    #[error("archive {archive} is not under {root}")]
    ArchiveOutsideRoot {
        /// The root archives are named relative to.
        root: Utf8PathBuf,
        /// The archive outside it.
        archive: Utf8PathBuf,
    },
    /// An archive's size or modification time did not read.
    #[error("cannot read metadata of {path}")]
    Metadata {
        /// The archive.
        path: Utf8PathBuf,
        /// The failure.
        #[source]
        source: std::io::Error,
    },
}
