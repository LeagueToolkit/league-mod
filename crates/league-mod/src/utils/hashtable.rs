//! Loading WAD hashtable files, the source of real chunk names for `extract`.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_wad::{PathResolver, WadHash};
use thiserror::Error;
use walkdir::WalkDir;

/// Failure to load a WAD hashtable.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WadHashtableError {
    /// A hashtable file could not be read.
    #[error("Failed to read {path}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl WadHashtableError {
    fn read(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }
}

/// The paths a WAD's chunk hashes were hashed from, read from hashtable files.
///
/// A hashtable file holds one entry per line: a hex hash, a space, then the
/// path. A hash with no entry resolves to nothing, and whoever asked names the
/// chunk by its hash instead, so a partial table still names what it covers.
#[derive(Debug, Default)]
pub struct WadHashtable {
    paths: HashMap<WadHash, String>,
}

impl WadHashtable {
    /// Load every file in `dir`, recursively.
    ///
    /// A directory that does not exist loads nothing and is not an error: the
    /// configured hashtable directory is optional, and unpacking falls back to
    /// hex names without it.
    ///
    /// # Errors
    ///
    /// [`WadHashtableError::Read`] if a file below `dir` cannot be read.
    pub fn from_directory(dir: impl AsRef<Utf8Path>) -> Result<Self, WadHashtableError> {
        let dir = dir.as_ref();
        let mut hashtable = Self::default();
        if !dir.exists() {
            return Ok(hashtable);
        }

        for entry in WalkDir::new(dir.as_std_path())
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
        {
            // A hashtable file whose own name is not UTF-8 still loads. Only
            // the path in the error message is approximate.
            let path = Utf8PathBuf::from_path_buf(entry.path().to_path_buf())
                .unwrap_or_else(|path| Utf8PathBuf::from(path.to_string_lossy().into_owned()));

            hashtable.add_from_file(&path)?;
        }

        Ok(hashtable)
    }

    fn add_from_file(&mut self, path: impl AsRef<Utf8Path>) -> Result<(), WadHashtableError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|source| WadHashtableError::read(path, source))?;

        self.add_from_reader(BufReader::new(file))
            .map_err(|source| WadHashtableError::read(path, source))
    }

    /// A line that does not start with a hash, or that names no path, is
    /// skipped: one bad line must not cost the rest of the file.
    fn add_from_reader(&mut self, reader: impl BufRead) -> Result<(), std::io::Error> {
        for line in reader.lines() {
            let line = line?;
            let Some((hash, path)) = line.split_once(' ') else {
                continue;
            };
            let Ok(hash) = u64::from_str_radix(hash, 16) else {
                continue;
            };
            if !path.is_empty() {
                self.paths.insert(WadHash(hash), path.to_owned());
            }
        }

        Ok(())
    }
}

impl PathResolver for WadHashtable {
    fn resolve(&self, path_hash: WadHash) -> Option<String> {
        self.paths.resolve(path_hash)
    }

    fn is_known(&self, path_hash: WadHash) -> bool {
        self.paths.is_known(path_hash)
    }
}

#[cfg(test)]
mod tests;
