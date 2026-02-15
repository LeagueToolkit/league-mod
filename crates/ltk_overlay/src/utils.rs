//! Path normalization and hash resolution utilities.
//!
//! These functions bridge the gap between how mod files are stored on disk (or in
//! archives) and the `u64` path hashes used inside WAD files.

use crate::error::Result;
use camino::Utf8Path;
use std::collections::HashMap;
use xxhash_rust::xxh3::xxh3_64;

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

/// Compute a deterministic fingerprint for a WAD's override set.
///
/// The fingerprint is based on sorted `(path_hash, content_hash)` pairs so that
/// two identical override sets always produce the same fingerprint regardless of
/// iteration order. Returns `0` for an empty override set.
///
/// This is used by the incremental builder to detect which WADs actually changed
/// between builds and skip re-patching the ones that didn't.
pub fn compute_wad_overrides_fingerprint<B: AsRef<[u8]>>(overrides: &HashMap<u64, B>) -> u64 {
    if overrides.is_empty() {
        return 0;
    }

    // Sort by path_hash for determinism
    let mut entries: Vec<(u64, u64)> = overrides
        .iter()
        .map(|(&path_hash, bytes)| (path_hash, xxh3_64(bytes.as_ref())))
        .collect();
    entries.sort_unstable_by_key(|(path_hash, _)| *path_hash);

    // Hash the sorted (path_hash, content_hash) pairs
    let mut buf = Vec::with_capacity(entries.len() * 16);
    for (path_hash, content_hash) in &entries {
        buf.extend_from_slice(&path_hash.to_le_bytes());
        buf.extend_from_slice(&content_hash.to_le_bytes());
    }

    xxh3_64(&buf)
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

    #[test]
    fn test_wad_fingerprint_deterministic() {
        let mut overrides1 = HashMap::new();
        overrides1.insert(1u64, vec![1, 2, 3]);
        overrides1.insert(2u64, vec![4, 5, 6]);

        let mut overrides2 = HashMap::new();
        overrides2.insert(2u64, vec![4, 5, 6]); // different insertion order
        overrides2.insert(1u64, vec![1, 2, 3]);

        assert_eq!(
            compute_wad_overrides_fingerprint(&overrides1),
            compute_wad_overrides_fingerprint(&overrides2)
        );
    }

    #[test]
    fn test_wad_fingerprint_different_content() {
        let mut overrides1 = HashMap::new();
        overrides1.insert(1u64, vec![1, 2, 3]);

        let mut overrides2 = HashMap::new();
        overrides2.insert(1u64, vec![4, 5, 6]);

        assert_ne!(
            compute_wad_overrides_fingerprint(&overrides1),
            compute_wad_overrides_fingerprint(&overrides2)
        );
    }

    #[test]
    fn test_wad_fingerprint_empty() {
        let overrides: HashMap<u64, Vec<u8>> = HashMap::new();
        assert_eq!(compute_wad_overrides_fingerprint(&overrides), 0);
    }

    #[test]
    fn test_wad_fingerprint_nonempty() {
        let mut overrides = HashMap::new();
        overrides.insert(42u64, vec![1, 2, 3]);
        assert_ne!(compute_wad_overrides_fingerprint(&overrides), 0);
    }
}
