use crate::error::{EncodingError, ReadTextError, StripPrefixError};
use camino::{Utf8Path, Utf8PathBuf};
use std::path::PathBuf;
use xxhash_rust::xxh3;

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
///
/// Prefer [`ChunkPath`](crate::ChunkPath), which applies this in its
/// constructor and so cannot be hashed before it is normalized. This is
/// exposed for display normalization, where no hash is involved.
pub fn normalize_chunk_path(path: &str) -> String {
    path.to_lowercase().replace('\\', "/")
}

/// Hash a layer name using xxhash3.
pub fn hash_layer_name(name: &str) -> u64 {
    xxh3::xxh3_64(name.to_lowercase().as_bytes())
}

/// Hash a wad name using xxhash3.
pub fn hash_wad_name(name: &str) -> u64 {
    xxh3::xxh3_64(name.to_lowercase().as_bytes())
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
        let bytes = std::fs::read(self).map_err(|source| ReadTextError::new(self, source))?;

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn strip_prefix_normalized(&self, base: &Utf8Path) -> Result<String, StripPrefixError> {
        let rel = self
            .strip_prefix(base)
            .map_err(|_| StripPrefixError::new(self, base))?;

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

    const IN_WAD: &str = "ASSETS/Characters/Aatrox/Skins/Base/Aatrox.dds";
    const CANONICAL: &str = "assets/characters/aatrox/skins/base/aatrox.dds";

    #[test]
    fn normalize_chunk_path_lowercases() {
        assert_eq!(normalize_chunk_path(IN_WAD), CANONICAL);
    }

    #[test]
    fn normalize_chunk_path_converts_backslashes() {
        assert_eq!(
            normalize_chunk_path("assets\\characters\\aatrox\\skins\\base\\aatrox.dds"),
            CANONICAL
        );
    }

    #[test]
    fn normalize_chunk_path_handles_mixed_separators() {
        assert_eq!(
            normalize_chunk_path("ASSETS\\Characters/Aatrox\\Skins/Base/Aatrox.dds"),
            CANONICAL
        );
    }

    #[test]
    fn normalize_chunk_path_noop_on_normalized() {
        assert_eq!(normalize_chunk_path(CANONICAL), CANONICAL);
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

        assert_eq!(err.path(), path);
        assert!(err.to_string().contains("missing"));
        assert_eq!(err.io_error().kind(), std::io::ErrorKind::NotFound);
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
