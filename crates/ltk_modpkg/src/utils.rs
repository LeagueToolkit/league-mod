use camino::{Utf8Path, Utf8PathBuf};
use std::io;
use std::path::{PathBuf, StripPrefixError};
use xxhash_rust::{xxh3, xxh64};

pub fn is_hex_chunk_name(chunk_name: &str) -> bool {
    // Reject 0x prefix
    if chunk_name.starts_with("0x") {
        return false;
    }

    // Validate the base name (before extension)
    let base = chunk_name.split('.').next().unwrap_or(chunk_name);
    if base.len() != 16 {
        return false;
    }

    base.chars().all(|c| c.is_ascii_hexdigit())
}

/// Normalize a chunk path for storage and hashing.
///
/// Lowercases the path and converts backslashes to forward slashes so that
/// the same logical path is represented identically on all platforms.
/// Call this once before storing or hashing a chunk path.
pub fn normalize_chunk_path(path: &str) -> String {
    path.to_lowercase().replace('\\', "/")
}

/// Hash a layer name using xxhash3.
pub fn hash_layer_name(name: &str) -> u64 {
    xxh3::xxh3_64(name.to_lowercase().as_bytes())
}

/// Hash a chunk name using xxhash64.
///
/// Expects a pre-normalized path (lowercase, forward slashes).
/// Use [`normalize_chunk_path`] before calling this if the input may
/// contain uppercase characters or backslashes.
pub fn hash_chunk_name(name: &str) -> u64 {
    xxh64::xxh64(name.as_bytes(), 0)
}

/// Hash a wad name using xxhash3.
pub fn hash_wad_name(name: &str) -> u64 {
    xxh3::xxh3_64(name.to_lowercase().as_bytes())
}

/// Check if a string is a valid slug (lowercase alphanumeric with hyphens).
pub fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

/// Read a text file, replacing invalid UTF-8 instead of failing.
///
/// Files picked up by convention rather than by explicit request (`README.md`,
/// `LICENSE`) are frequently authored in Latin-1, and a `©` in a copy of a GPL
/// or Creative Commons text must not be able to fail an otherwise valid
/// operation. Genuine IO failures still surface.
pub fn read_text_file_lossy(path: &Utf8Path) -> io::Result<String> {
    let bytes = std::fs::read(path)?;

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Convert a `std::path::PathBuf` to a `Utf8PathBuf`.
///
/// On failure the error carries the offending path rendered lossily, which is
/// the only form left that can go in a message.
pub fn utf8_path_from(path: PathBuf) -> Result<Utf8PathBuf, String> {
    Utf8PathBuf::from_path_buf(path).map_err(|path| path.display().to_string())
}

/// Strip a prefix from a path and return the remainder as a normalized string
/// (forward slashes, for cross-platform consistency).
pub fn strip_path_prefix(path: &Utf8Path, base: &Utf8Path) -> Result<String, StripPrefixError> {
    let rel = path.strip_prefix(base)?;

    Ok(rel.as_str().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_chunk_path_lowercases() {
        assert_eq!(
            normalize_chunk_path("Graves.wad.client/Data/File.bin"),
            "graves.wad.client/data/file.bin"
        );
    }

    #[test]
    fn normalize_chunk_path_converts_backslashes() {
        assert_eq!(
            normalize_chunk_path("graves.wad.client\\data\\characters\\graves"),
            "graves.wad.client/data/characters/graves"
        );
    }

    #[test]
    fn normalize_chunk_path_handles_mixed_separators() {
        assert_eq!(
            normalize_chunk_path("Graves.wad.client\\Data/Characters\\Graves"),
            "graves.wad.client/data/characters/graves"
        );
    }

    #[test]
    fn normalize_chunk_path_noop_on_normalized() {
        let path = "graves.wad.client/data/file.bin";
        assert_eq!(normalize_chunk_path(path), path);
    }

    #[test]
    fn hash_chunk_name_consistent_after_normalization() {
        let forward = normalize_chunk_path("graves.wad.client/data/file.bin");
        let back = normalize_chunk_path("graves.wad.client\\data\\file.bin");
        let mixed = normalize_chunk_path("Graves.wad.client\\Data/File.bin");

        assert_eq!(hash_chunk_name(&forward), hash_chunk_name(&back));
        assert_eq!(hash_chunk_name(&forward), hash_chunk_name(&mixed));
    }

    #[test]
    fn test_is_valid_slug() {
        assert!(is_valid_slug("base"));
        assert!(is_valid_slug("my-layer"));
        assert!(is_valid_slug("layer123"));
        assert!(!is_valid_slug(""));
        assert!(!is_valid_slug("-invalid"));
        assert!(!is_valid_slug("invalid-"));
        assert!(!is_valid_slug("UPPERCASE"));
        assert!(!is_valid_slug("has spaces"));
    }

    #[test]
    fn read_text_file_lossy_replaces_invalid_utf8() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(tmp.path().join("LICENSE")).unwrap();

        // Latin-1 "Copyright © 2026".
        std::fs::write(&path, b"Copyright \xA9 2026").unwrap();

        assert_eq!(
            read_text_file_lossy(&path).unwrap(),
            "Copyright \u{FFFD} 2026"
        );
    }

    #[test]
    fn read_text_file_lossy_surfaces_io_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(tmp.path().join("missing")).unwrap();

        assert!(read_text_file_lossy(&path).is_err());
    }

    #[test]
    fn strip_path_prefix_normalizes_separators() {
        let base = Utf8Path::new("root/content/base");

        assert_eq!(
            strip_path_prefix(&base.join("Graves.wad.client").join("f.bin"), base).unwrap(),
            "Graves.wad.client/f.bin"
        );
    }

    #[test]
    fn strip_path_prefix_rejects_unrelated_base() {
        assert!(strip_path_prefix(Utf8Path::new("a/b"), Utf8Path::new("c")).is_err());
    }
}
