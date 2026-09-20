//! Path normalization and hash resolution utilities.
//!
//! These functions bridge the gap between how mod files are stored on disk (or in
//! archives) and the [`WadHash`] path hashes used inside WAD files.

use crate::builder::OverrideMeta;
use crate::error::Result;
use camino::Utf8Path;
use ltk_wad::{WadHash, is_hex_chunk_path};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use xxhash_rust::xxh3::xxh3_64;

/// xxh3_64 over an override's uncompressed bytes: *what* a chunk holds.
///
/// The counterpart to [`WadHash`], which says *where* a chunk goes. The two are
/// both 64-bit hashes and are routinely held in the same map, so keeping them
/// distinct in the type system is what stops one being read as the other: a
/// content hash is shared by every override carrying identical bytes, which is
/// exactly what lets the build compress that content once and hand the result
/// to every WAD holding it.
///
/// Serializes as the bare integer, so it costs `overlay.json` and the metadata
/// cache nothing.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ContentHash(pub u64);

impl ContentHash {
    /// The content hash of `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        Self(xxh3_64(bytes))
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

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
        format!("{}{}", &last[..idx], &last[idx + 4..])
    } else if last.to_ascii_lowercase().ends_with(".ltk") {
        last[..last.len().saturating_sub(4)].to_string()
    } else {
        last
    };
    parts.push(stripped);

    let joined = parts.join("/");
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
///    [`normalize_rel_path_for_hash`] and hashed as a
///    [`ltk_modpkg::ChunkPath`] (xxHash64).
pub fn resolve_chunk_hash(rel_path: &Utf8Path, bytes: &[u8]) -> Result<WadHash> {
    if is_hex_chunk_path(rel_path)
        && let Ok(v) = u64::from_str_radix(rel_path.file_stem().unwrap_or(""), 16)
    {
        return Ok(WadHash(v));
    }

    Ok(WadHash(
        ltk_modpkg::ChunkPath::new(normalize_rel_path_for_hash(rel_path, bytes))
            .hash()
            .value(),
    ))
}

/// Compute a deterministic fingerprint for a WAD's override set.
///
/// The fingerprint is based on sorted `(path_hash, content_hash)` pairs so that
/// two identical override sets always produce the same fingerprint regardless of
/// iteration order. Returns `0` for an empty override set.
///
/// This is used by the incremental builder to detect which WADs actually changed
/// between builds and skip re-patching the ones that didn't.
pub fn compute_wad_overrides_fingerprint<B: AsRef<[u8]>>(overrides: &HashMap<WadHash, B>) -> u64 {
    if overrides.is_empty() {
        return 0;
    }

    // Sort by path_hash for determinism
    let mut entries: Vec<(WadHash, ContentHash)> = overrides
        .iter()
        .map(|(&path_hash, bytes)| (path_hash, ContentHash::of(bytes.as_ref())))
        .collect();
    entries.sort_unstable_by_key(|(path_hash, _)| *path_hash);

    fingerprint_from_sorted_pairs(&entries)
}

/// Compute a deterministic fingerprint from pre-computed `(path_hash, content_hash)` pairs.
///
/// This is the metadata-based equivalent of [`compute_wad_overrides_fingerprint`] -
/// it produces an identical `u64` for the same set of overrides, but uses pre-computed
/// content hashes from the metadata cache instead of hashing raw bytes.
///
/// The `wad_hashes` set selects which entries from `all_meta` belong to this WAD.
pub fn compute_wad_fingerprint_from_meta(
    wad_hashes: &HashSet<WadHash>,
    all_meta: &HashMap<WadHash, OverrideMeta>,
) -> u64 {
    if wad_hashes.is_empty() {
        return 0;
    }

    let mut entries: Vec<(WadHash, ContentHash)> = wad_hashes
        .iter()
        .filter_map(|&path_hash| {
            let meta = all_meta.get(&path_hash)?;
            Some((path_hash, meta.content_hash))
        })
        .collect();
    entries.sort_unstable_by_key(|(path_hash, _)| *path_hash);

    fingerprint_from_sorted_pairs(&entries)
}

/// Hash sorted `(path_hash, content_hash)` pairs into a single fingerprint.
///
/// The two `.0`s are the one place these newtypes are unwrapped on purpose:
/// this encodes the hashes as bytes, and the result is persisted in
/// `overlay.json`, so the layout has to stay fixed.
fn fingerprint_from_sorted_pairs(entries: &[(WadHash, ContentHash)]) -> u64 {
    if entries.is_empty() {
        return 0;
    }

    let mut buf = Vec::with_capacity(entries.len() * 16);
    for (path_hash, content_hash) in entries {
        buf.extend_from_slice(&path_hash.0.to_le_bytes());
        buf.extend_from_slice(&content_hash.0.to_le_bytes());
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
        assert_eq!(hash, WadHash(0x0123456789abcdef));
    }

    #[test]
    fn test_wad_fingerprint_deterministic() {
        let mut overrides1 = HashMap::new();
        overrides1.insert(WadHash(1), vec![1, 2, 3]);
        overrides1.insert(WadHash(2), vec![4, 5, 6]);

        let mut overrides2 = HashMap::new();
        overrides2.insert(WadHash(2), vec![4, 5, 6]); // different insertion order
        overrides2.insert(WadHash(1), vec![1, 2, 3]);

        assert_eq!(
            compute_wad_overrides_fingerprint(&overrides1),
            compute_wad_overrides_fingerprint(&overrides2)
        );
    }

    #[test]
    fn test_wad_fingerprint_different_content() {
        let mut overrides1 = HashMap::new();
        overrides1.insert(WadHash(1), vec![1, 2, 3]);

        let mut overrides2 = HashMap::new();
        overrides2.insert(WadHash(1), vec![4, 5, 6]);

        assert_ne!(
            compute_wad_overrides_fingerprint(&overrides1),
            compute_wad_overrides_fingerprint(&overrides2)
        );
    }

    #[test]
    fn test_wad_fingerprint_empty() {
        let overrides: HashMap<WadHash, Vec<u8>> = HashMap::new();
        assert_eq!(compute_wad_overrides_fingerprint(&overrides), 0);
    }

    #[test]
    fn test_wad_fingerprint_nonempty() {
        let mut overrides = HashMap::new();
        overrides.insert(WadHash(42), vec![1, 2, 3]);
        assert_ne!(compute_wad_overrides_fingerprint(&overrides), 0);
    }

    #[test]
    fn test_meta_fingerprint_matches_byte_fingerprint() {
        use crate::builder::{OverrideMeta, OverrideSource};

        // Create byte-based overrides
        let mut byte_overrides: HashMap<WadHash, Vec<u8>> = HashMap::new();
        byte_overrides.insert(WadHash(1), vec![1, 2, 3]);
        byte_overrides.insert(WadHash(2), vec![4, 5, 6]);
        byte_overrides.insert(WadHash(100), vec![7, 8, 9, 10]);

        let byte_fp = compute_wad_overrides_fingerprint(&byte_overrides);

        // Create equivalent metadata
        let mut all_meta: HashMap<WadHash, OverrideMeta> = HashMap::new();
        for (&path_hash, bytes) in &byte_overrides {
            all_meta.insert(
                path_hash,
                OverrideMeta {
                    content_hash: ContentHash::of(bytes),
                    uncompressed_size: bytes.len(),
                    source: OverrideSource::Raw {
                        mod_id: "test-mod".to_string(),
                        rel_path: Utf8PathBuf::from("dummy"),
                    },
                    fallback_wad: None,
                    unlocalized_wad: None,
                    linked_bins: Vec::new(),
                },
            );
        }

        let wad_hashes: HashSet<WadHash> = byte_overrides.keys().copied().collect();
        let meta_fp = compute_wad_fingerprint_from_meta(&wad_hashes, &all_meta);

        assert_eq!(
            byte_fp, meta_fp,
            "Metadata-based fingerprint must match byte-based fingerprint"
        );
    }

    #[test]
    fn test_meta_fingerprint_empty() {
        let wad_hashes: HashSet<WadHash> = HashSet::new();
        let all_meta: HashMap<WadHash, OverrideMeta> = HashMap::new();
        assert_eq!(compute_wad_fingerprint_from_meta(&wad_hashes, &all_meta), 0);
    }
}
