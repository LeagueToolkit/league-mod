//! Path normalization and hash resolution utilities.
//!
//! These functions bridge the gap between how mod files are stored on disk (or in
//! archives) and the `u64` path hashes used inside WAD files.

use crate::error::Result;
use camino::Utf8Path;

/// Normalize a relative path for hash computation.
///
/// Strips `.ltk` suffixes that the LeagueToolkit extractor adds to avoid filename
/// collisions with the WAD format:
///
/// - `file.ltk.bin` -> `file.bin` (`.ltk` removed, original extension preserved)
/// - `file.ltk` -> `file` (bare `.ltk` suffix removed)
/// - `file.bin` -> `file.bin` (no change)
///
/// Path separators are normalized to forward slashes (`/`) for consistent hashing
/// across platforms.
pub fn normalize_rel_path_for_hash(rel_path: &Utf8Path, _bytes: &[u8]) -> String {
    let mut parts = rel_path
        .components()
        .map(|c| c.as_str().to_string())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return String::new();
    }

    // Special case: strip `.ltk` suffix patterns from the filename
    let last = parts.pop().unwrap();
    let stripped = if let Some(idx) = last.to_ascii_lowercase().find(".ltk.") {
        // Remove .ltk - e.g., "file.ltk.bin" -> "file.bin"
        // idx points to the '.', we want to keep it and append from after '.ltk'
        format!("{}{}", &last[..idx], &last[idx + 4..])
    } else if last.to_ascii_lowercase().ends_with(".ltk") {
        // Remove .ltk suffix - e.g., "file.ltk" -> "file"
        last[..last.len().saturating_sub(4)].to_string()
    } else {
        last
    };
    parts.push(stripped);

    // Join using '/'
    let joined = parts.join("/");

    // If we stripped to empty (rare), fall back to original filename
    if joined.is_empty() {
        return rel_path.as_str().replace('\\', "/");
    }

    joined.replace('\\', "/")
}

/// Resolve the WAD chunk path hash for a mod override file.
///
/// Two resolution strategies:
///
/// 1. **Hex-hash filename**: If the file stem is exactly 16 hex digits
///    (e.g., `0123456789abcdef.bin`), it is parsed directly as a `u64` hash.
///    This is used by packed WAD content providers that don't have the original
///    path names.
///
/// 2. **Named path**: Otherwise, the path is normalized via
///    [`normalize_rel_path_for_hash`] and hashed with
///    [`ltk_modpkg::utils::hash_chunk_name`] (xxHash3).
pub fn resolve_chunk_hash(rel_path: &Utf8Path, bytes: &[u8]) -> Result<u64> {
    let file_name = rel_path.file_name().unwrap_or("");
    let file_stem = Utf8Path::new(file_name).file_stem().unwrap_or("");

    // If this is a hex-hash filename (as emitted by HexPathResolver), use it directly
    if file_stem.len() == 16 && file_stem.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(v) = u64::from_str_radix(file_stem, 16) {
            return Ok(v);
        }
    }

    // Otherwise, compute from normalized path
    let normalized_rel = normalize_rel_path_for_hash(rel_path, bytes);
    Ok(ltk_modpkg::utils::hash_chunk_name(&normalized_rel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    #[test]
    fn test_normalize_ltk_suffix() {
        let path = Utf8PathBuf::from("data/characters/aatrox/aatrox.ltk.bin");
        let normalized = normalize_rel_path_for_hash(&path, b"");
        assert_eq!(normalized, "data/characters/aatrox/aatrox.bin");
    }

    #[test]
    fn test_normalize_ltk_extension() {
        let path = Utf8PathBuf::from("data/characters/aatrox/aatrox.ltk");
        let normalized = normalize_rel_path_for_hash(&path, b"");
        assert_eq!(normalized, "data/characters/aatrox/aatrox");
    }

    #[test]
    fn test_normalize_regular_path() {
        let path = Utf8PathBuf::from("data/characters/aatrox/aatrox.bin");
        let normalized = normalize_rel_path_for_hash(&path, b"");
        assert_eq!(normalized, "data/characters/aatrox/aatrox.bin");
    }

    #[test]
    fn test_resolve_hex_hash() {
        let path = Utf8PathBuf::from("0123456789abcdef.bin");
        let hash = resolve_chunk_hash(&path, b"").unwrap();
        assert_eq!(hash, 0x0123456789abcdef);
    }
}
