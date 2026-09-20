//! The identity of an installation's archive set.

use std::fmt;

use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::Xxh3;

use crate::{Archive, BuildError};

/// Identity of an installation's archive set: sizes and modification times.
///
/// XXH3-64 over the sorted archive names, each followed by its file size and its
/// modification time in nanoseconds since the Unix epoch. Skipped archives contribute their
/// metadata. Two installations with equal fingerprints index identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint(u64);

impl Fingerprint {
    /// The fingerprint as a plain integer, for wire formats that carry a `u64`.
    #[must_use]
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Fingerprint {
    /// Sixteen lowercase hex digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl fmt::LowerHex for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

/// The fingerprint of `archives`, which are sorted by name.
///
/// # Errors
///
/// [`BuildError::Metadata`] when an archive's metadata does not read.
pub(crate) fn of_archives(archives: &[Archive]) -> Result<Fingerprint, BuildError> {
    let mut hasher = Xxh3::new();
    for archive in archives {
        let metadata = std::fs::metadata(archive.path.as_std_path()).map_err(|source| {
            BuildError::Metadata {
                path: archive.path.clone(),
                source,
            }
        })?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_nanos());
        hasher.update(archive.name.as_bytes());
        hasher.update(&metadata.len().to_le_bytes());
        hasher.update(&modified.to_le_bytes());
    }
    Ok(Fingerprint(hasher.digest()))
}

/// The fingerprint of the installation at `game_dir`, without mounting an archive.
pub(crate) fn of_game_dir(game_dir: &Utf8Path) -> Result<Fingerprint, BuildError> {
    let archives = crate::build::enumerate_archives(game_dir)?;
    of_archives(&archives)
}
