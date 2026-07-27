//! WAD hashtable for resolving path hashes to human-readable paths.

use camino::{Utf8Path, Utf8PathBuf};
use ltk_wad::PathResolver;
use std::{
    borrow::Cow,
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
};
use walkdir::WalkDir;

use crate::error::WadHashtableError;

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
    pub fn from_directory(dir: impl AsRef<Utf8Path>) -> Result<Self, WadHashtableError> {
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
    ///
    /// A directory that does not exist loads nothing and is not an error.
    pub fn add_from_dir(&mut self, dir: impl AsRef<Utf8Path>) -> Result<(), WadHashtableError> {
        let dir_path = dir.as_ref();
        if !dir_path.exists() {
            return Ok(());
        }

        for entry in WalkDir::new(dir_path.as_std_path())
            .into_iter()
            .filter_map(|x| x.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            // A hashtable file whose own name is not UTF-8 still loads; only
            // the path in the error message is approximate.
            let path = Utf8PathBuf::from_path_buf(entry.path().to_path_buf())
                .unwrap_or_else(|path| Utf8PathBuf::from(path.to_string_lossy().into_owned()));

            self.add_from_file(&path)?;
        }

        Ok(())
    }

    /// Loads hashtable entries from a single file.
    ///
    /// File format: Each line contains a hex hash followed by a space and the path.
    /// Example: `0123456789abcdef assets/characters/aatrox/skin0.bin`
    pub fn add_from_file(&mut self, path: impl AsRef<Utf8Path>) -> Result<(), WadHashtableError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|source| WadHashtableError::read(path, source))?;

        self.add_from_reader(BufReader::new(file))
            .map_err(|source| WadHashtableError::read(path, source))
    }

    /// Loads hashtable entries from anything readable.
    ///
    /// Errors carry no path; prefer [`add_from_file`](Self::add_from_file)
    /// when there is a file to name.
    pub fn add_from_reader(&mut self, reader: impl BufRead) -> Result<(), std::io::Error> {
        for line in reader.lines() {
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
        self.resolve_path(path_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn resolves_known_hashes_and_falls_back_to_hex() {
        let mut hashtable = WadHashtable::new();
        hashtable
            .add_from_reader(&b"0123456789abcdef assets/characters/aatrox/skin0.bin\n"[..])
            .unwrap();

        assert_eq!(
            hashtable.resolve_path(0x0123456789abcdef),
            "assets/characters/aatrox/skin0.bin"
        );
        assert_eq!(hashtable.resolve_path(0x1), "0000000000000001");
    }

    /// Paths with spaces survive: the line is split on the first space only.
    #[test]
    fn keeps_spaces_in_paths() {
        let mut hashtable = WadHashtable::new();
        hashtable
            .add_from_reader(&b"00000000000000ff some path/with spaces.bin"[..])
            .unwrap();

        assert_eq!(hashtable.resolve_path(0xff), "some path/with spaces.bin");
    }

    #[test]
    fn skips_malformed_lines() {
        let mut hashtable = WadHashtable::new();
        hashtable
            .add_from_reader(&b"\nnot-a-hash some/path\n00000000000000ff kept.bin\nff\n"[..])
            .unwrap();

        assert_eq!(hashtable.len(), 1);
        assert_eq!(hashtable.resolve_path(0xff), "kept.bin");
    }

    #[test]
    fn missing_file_names_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(tmp.path().join("absent.txt")).unwrap();

        match WadHashtable::new().add_from_file(&path) {
            Err(WadHashtableError::Read { path: failed, .. }) => assert_eq!(failed, path),
            other => panic!("expected Read, got {other:?}"),
        }
    }

    /// A hashtable directory the user has not created is not a failure.
    #[test]
    fn missing_directory_loads_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let hashtable = WadHashtable::from_directory(root.join("does-not-exist")).unwrap();

        assert!(hashtable.is_empty());
    }

    #[test]
    fn loads_every_file_in_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        std::fs::create_dir(root.join("nested")).unwrap();
        let mut first = File::create(root.join("a.txt")).unwrap();
        first.write_all(b"0000000000000001 one.bin\n").unwrap();
        let mut second = File::create(root.join("nested").join("b.txt")).unwrap();
        second.write_all(b"0000000000000002 two.bin\n").unwrap();

        let hashtable = WadHashtable::from_directory(&root).unwrap();

        assert_eq!(hashtable.len(), 2);
        assert_eq!(hashtable.resolve_path(1), "one.bin");
        assert_eq!(hashtable.resolve_path(2), "two.bin");
    }
}
