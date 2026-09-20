//! Archives of an installation and their identity.

use std::fmt;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::BuildError;

/// The suffix an archive's file name ends in, compared ASCII case-insensitively.
const ARCHIVE_SUFFIX: &str = ".wad.client";

/// The ordinal of an archive in [`GameIndex::archives`](crate::GameIndex::archives).
///
/// Ids are dense from zero in archive name order. An id is stable for one index and its
/// caches. An index built against a changed installation numbers its archives afresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ArchiveId(pub(crate) u32);

impl ArchiveId {
    /// The position of the archive in the index's archive list.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for ArchiveId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// One `.wad.client` of the installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Archive {
    /// `DATA/FINAL`-relative, forward slashes: `Champions/Aatrox.wad.client`.
    pub name: String,
    /// Absolute path of the file.
    pub path: Utf8PathBuf,
}

impl Archive {
    /// The last segment of `name`: `Aatrox.wad.client`.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.name.rsplit('/').next().unwrap_or(&self.name)
    }
}

/// Why an archive did not index.
///
/// The message of the failure is kept. A skipped archive survives the cache and
/// compares equal to itself after a round trip.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ArchiveReadError {
    /// The file did not open.
    #[error("cannot open the archive: {0}")]
    Open(String),
    /// The table of contents did not mount.
    #[error("cannot mount the archive: {0}")]
    Mount(String),
}

impl ArchiveReadError {
    pub(crate) fn open(source: &std::io::Error) -> Self {
        Self::Open(source.to_string())
    }

    pub(crate) fn mount(source: &ltk_wad::WadError) -> Self {
        Self::Mount(source.to_string())
    }
}

/// An archive the build could not open or mount.
///
/// The archive keeps its id and its place in the archive list. It holds no chunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SkippedArchive {
    /// The archive that did not read.
    pub archive: ArchiveId,
    /// Why it did not read.
    pub error: ArchiveReadError,
}

/// A file-name lookup that did not name exactly one archive.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArchiveLookupError {
    /// No archive has the file name.
    #[error("no archive is named {file_name}")]
    Absent {
        /// The name looked up.
        file_name: String,
    },
    /// Several archives have the file name.
    #[error("{file_name} names {} archives", candidates.len())]
    Ambiguous {
        /// The name looked up.
        file_name: String,
        /// Every archive with that file name, in id order.
        candidates: Vec<ArchiveId>,
    },
}

/// Whether `file_name` is an archive's, ASCII case-insensitively.
pub(crate) fn is_archive_file_name(file_name: &str) -> bool {
    file_name.len() >= ARCHIVE_SUFFIX.len()
        && file_name
            .get(file_name.len() - ARCHIVE_SUFFIX.len()..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(ARCHIVE_SUFFIX))
}

/// Every archive file under `root`, unsorted.
///
/// A path that is not UTF-8 is logged and left out.
///
/// # Errors
///
/// [`BuildError::Enumerate`] when the walk fails on any entry.
pub(crate) fn enumerate(root: &Utf8Path) -> Result<Vec<Utf8PathBuf>, BuildError> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(root.as_std_path()) {
        let entry = entry.map_err(|source| BuildError::Enumerate {
            path: root.to_path_buf(),
            source: source.into(),
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = match Utf8PathBuf::from_path_buf(entry.into_path()) {
            Ok(path) => path,
            Err(path) => {
                tracing::warn!("Skipping non-UTF-8 path {}", path.display());
                continue;
            }
        };
        if path.file_name().is_some_and(is_archive_file_name) {
            paths.push(path);
        }
    }
    Ok(paths)
}

/// The archive at `path` named relative to `root`.
///
/// An absolute `path` is stripped of `root`. A relative `path` is taken as already relative
/// to `root`. The name has forward slashes.
///
/// # Errors
///
/// [`BuildError::ArchiveOutsideRoot`] when `path` does not lie under `root`.
pub(crate) fn archive_under(root: &Utf8Path, path: &Utf8Path) -> Result<Archive, BuildError> {
    let outside = || BuildError::ArchiveOutsideRoot {
        root: root.to_path_buf(),
        archive: path.to_path_buf(),
    };
    let relative = if path.is_absolute() {
        path.strip_prefix(root).map_err(|_| outside())?
    } else {
        path
    };
    if relative
        .components()
        .any(|part| !matches!(part, camino::Utf8Component::Normal(_)))
    {
        return Err(outside());
    }
    Ok(Archive {
        name: relative.as_str().replace('\\', "/"),
        path: root.join(relative),
    })
}
