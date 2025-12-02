//! WAD hashtable for resolving path hashes to human-readable paths.

use camino::Utf8Path;
use ltk_wad::PathResolver;
use std::{
    borrow::Cow,
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
};
use walkdir::WalkDir;

use crate::error::FantomeExtractError;

/// Formats a chunk path hash as a hexadecimal string.
pub fn format_chunk_path_hash(path_hash: u64) -> String {
    format!("{:016x}", path_hash)
}

/// A hashtable that maps WAD path hashes to their original paths.
///
/// WAD files store file paths as 64-bit hashes. This hashtable allows
/// resolving those hashes back to human-readable paths during extraction.
#[derive(Debug, Clone, Default)]
pub struct WadHashtable {
    items: HashMap<u64, String>,
}

impl WadHashtable {
    /// Creates a new empty hashtable.
    pub fn new() -> Self {
        WadHashtable {
            items: HashMap::default(),
        }
    }

    /// Creates a hashtable by loading all files from a directory recursively.
    pub fn from_directory(dir: impl AsRef<Utf8Path>) -> Result<Self, FantomeExtractError> {
        let mut hashtable = Self::new();
        hashtable.add_from_dir(dir)?;
        Ok(hashtable)
    }

    /// Resolves a path hash to its original path, or returns a hex string if not found.
    pub fn resolve_path(&self, path_hash: u64) -> Cow<'_, str> {
        self.items
            .get(&path_hash)
            .map(|s| Cow::Borrowed(s.as_str()))
            .unwrap_or_else(|| Cow::Owned(format_chunk_path_hash(path_hash)))
    }

    /// Loads hashtable entries from all files in a directory recursively.
    pub fn add_from_dir(&mut self, dir: impl AsRef<Utf8Path>) -> Result<(), FantomeExtractError> {
        let dir_path = dir.as_ref();
        if !dir_path.exists() {
            return Ok(()); // Silently skip if directory doesn't exist
        }

        for entry in WalkDir::new(dir_path.as_std_path())
            .into_iter()
            .filter_map(|x| x.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            self.add_from_file(&File::open(entry.path())?)?;
        }

        Ok(())
    }

    /// Loads hashtable entries from a single file.
    ///
    /// File format: Each line contains a hex hash followed by a space and the path.
    /// Example: `0123456789abcdef assets/characters/aatrox/skin0.bin`
    pub fn add_from_file(&mut self, file: &File) -> Result<(), FantomeExtractError> {
        let reader = BufReader::new(file);
        let lines = reader.lines();

        for line in lines {
            let line = line?;
            let mut components = line.split(' ');

            let Some(hash_str) = components.next() else {
                continue; // Skip empty lines
            };

            let Ok(hash) = u64::from_str_radix(hash_str, 16) else {
                continue; // Skip invalid hashes
            };

            let path: String = itertools::join(components, " ");
            if !path.is_empty() {
                self.items.insert(hash, path);
            }
        }

        Ok(())
    }

    /// Returns a reference to the internal hashmap.
    pub fn items(&self) -> &HashMap<u64, String> {
        &self.items
    }

    /// Returns the number of entries in the hashtable.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the hashtable is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl PathResolver for WadHashtable {
    fn resolve(&self, path_hash: u64) -> Cow<'_, str> {
        self.items
            .get(&path_hash)
            .map(|s| Cow::Borrowed(s.as_str()))
            .unwrap_or_else(|| Cow::Owned(format_chunk_path_hash(path_hash)))
    }
}
