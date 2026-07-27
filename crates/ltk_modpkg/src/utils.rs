use crate::error::{EncodingError, ReadTextError, StripPrefixError};
use camino::{Utf8Path, Utf8PathBuf};
use std::path::PathBuf;
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

/// Path operations that `ltk_modpkg` needs but `camino` does not provide.
pub trait Utf8PathExt {
    /// Read the file as text, replacing invalid UTF-8 instead of failing.
    ///
    /// Files picked up by convention rather than by explicit request
    /// (`README.md`, `LICENSE`) are frequently authored in Latin-1, and a `©`
    /// in a copy of a GPL or Creative Commons text must not be able to fail an
    /// otherwise valid operation. Genuine IO failures still surface, with the
    /// path attached.
    fn read_text_lossy(&self) -> Result<String, ReadTextError>;

    /// Strip `base` from the path and return the remainder as a normalized
    /// string (forward slashes, for cross-platform consistency).
    fn strip_prefix_normalized(&self, base: &Utf8Path) -> Result<String, StripPrefixError>;
}

impl Utf8PathExt for Utf8Path {
    fn read_text_lossy(&self) -> Result<String, ReadTextError> {
        let bytes = std::fs::read(self).map_err(|source| ReadTextError {
            path: self.to_owned(),
            source,
        })?;

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn strip_prefix_normalized(&self, base: &Utf8Path) -> Result<String, StripPrefixError> {
        let rel = self.strip_prefix(base).map_err(|_| StripPrefixError {
            path: self.to_owned(),
            base: base.to_owned(),
        })?;

        Ok(rel.as_str().replace('\\', "/"))
    }
}

/// Conversion from the `std` path types that OS APIs hand back.
pub trait PathBufExt {
    /// Convert into a [`Utf8PathBuf`], or fail with the lossy rendering.
    fn into_utf8(self) -> Result<Utf8PathBuf, EncodingError>;
}

impl PathBufExt for PathBuf {
    fn into_utf8(self) -> Result<Utf8PathBuf, EncodingError> {
        Utf8PathBuf::from_path_buf(self)
            .map_err(|path| EncodingError::NonUtf8Path(path.display().to_string()))
    }
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
    fn read_text_lossy_replaces_invalid_utf8() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("LICENSE").into_utf8().unwrap();

        // Latin-1 "Copyright © 2026".
        std::fs::write(&path, b"Copyright \xA9 2026").unwrap();

        assert_eq!(path.read_text_lossy().unwrap(), "Copyright \u{FFFD} 2026");
    }

    #[test]
    fn read_text_lossy_names_the_file_it_failed_on() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("missing").into_utf8().unwrap();

        let err = path.read_text_lossy().unwrap_err();

        assert_eq!(err.path, path);
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn strip_prefix_normalized_normalizes_separators() {
        let base = Utf8Path::new("root/content/base");

        assert_eq!(
            base.join("Graves.wad.client")
                .join("f.bin")
                .strip_prefix_normalized(base)
                .unwrap(),
            "Graves.wad.client/f.bin"
        );
    }

    #[test]
    fn strip_prefix_normalized_rejects_unrelated_base() {
        let err = Utf8Path::new("a/b")
            .strip_prefix_normalized(Utf8Path::new("c"))
            .unwrap_err();

        // Both sides make it into the message; the std error says only
        // "prefix not found", which is useless without them.
        assert_eq!(err.to_string(), "a/b is not inside c");
    }

    #[test]
    fn into_utf8_accepts_a_utf8_path() {
        assert_eq!(
            PathBuf::from("root/content/base").into_utf8().unwrap(),
            Utf8Path::new("root/content/base")
        );
    }
}
