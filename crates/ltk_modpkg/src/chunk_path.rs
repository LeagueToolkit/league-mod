use std::fmt::Display;

use xxhash_rust::xxh64;

use crate::PathHash;

/// A chunk's stored path: the authored casing, forward slashes.
///
/// This is the path the file has *inside its target WAD*, matching what the
/// game ships. The WAD it belongs to and the layer it came from are stored
/// separately on the chunk, so neither the `.wad.client` directory nor the
/// layer name appears here.
///
/// Two forms of one path (ADR 0003): the **stored path** is the authored form
/// this type holds and [`as_str`](Self::as_str) returns; the **canonical
/// name** is its ASCII-lowercased form, which is what gets hashed and what
/// identity means. The canonical string itself is never exposed -
/// [`hash`](Self::hash) is the identity, and equality, ordering and
/// [`std::hash::Hash`] all judge the canonical name, so two spellings of one
/// path are one `ChunkPath`. ASCII-lowercasing matches every other hash space
/// in the workspace (`ltk_hashtable`'s keys, `ltk_hash`).
///
/// Chunks are keyed by the hash of their path, so a hash taken over a
/// denormalized path silently matches nothing. Separator normalization happens
/// once, in the constructor, and the lowercasing lives inside [`hash`](Self::hash),
/// which makes those states unrepresentable rather than merely documented.
///
/// ```
/// use ltk_modpkg::ChunkPath;
///
/// let path = ChunkPath::new("ASSETS\\Characters\\Aatrox\\Skins\\Base\\Aatrox.dds");
///
/// // The stored path keeps the authored casing...
/// assert_eq!(path.as_str(), "ASSETS/Characters/Aatrox/Skins/Base/Aatrox.dds");
/// // ...and identity is the canonical name.
/// assert_eq!(
///     path.hash(),
///     ChunkPath::new("assets/characters/aatrox/skins/base/aatrox.dds").hash()
/// );
/// ```
#[derive(Debug, Clone, Default)]
pub struct ChunkPath(String);

impl ChunkPath {
    /// Normalize `path`'s separators into a chunk path, keeping its casing.
    pub fn new(path: impl AsRef<str>) -> Self {
        Self(path.as_ref().replace('\\', "/"))
    }

    /// The xxhash64 of the canonical name, as stored in the chunk table.
    pub fn hash(&self) -> PathHash {
        PathHash::new(xxh64::xxh64(self.0.to_ascii_lowercase().as_bytes(), 0))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the path, yielding the stored string.
    pub fn into_string(self) -> String {
        self.0
    }

    /// The canonical name, one byte at a time, without allocating.
    ///
    /// The same ASCII-lowercasing [`hash`](Self::hash) applies, so everything
    /// that judges identity judges the same bytes.
    fn canonical_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.bytes().map(|byte| byte.to_ascii_lowercase())
    }
}

// Identity is the canonical name (ADR 0003): implemented by hand rather than
// derived so two spellings of one path stay equal now that the stored bytes
// keep their casing. `Hash` must agree with `Eq`, so it feeds the same
// lowercased bytes.

impl PartialEq for ChunkPath {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for ChunkPath {}

impl std::hash::Hash for ChunkPath {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for byte in self.canonical_bytes() {
            state.write_u8(byte);
        }
    }
}

impl PartialOrd for ChunkPath {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ChunkPath {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.canonical_bytes().cmp(other.canonical_bytes())
    }
}

impl Display for ChunkPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ChunkPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ChunkPath {
    fn from(path: &str) -> Self {
        Self::new(path)
    }
}

impl From<String> for ChunkPath {
    fn from(path: String) -> Self {
        Self::new(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN_WAD: &str = "ASSETS/Characters/Aatrox/Skins/Base/Aatrox.dds";
    const CANONICAL: &str = "assets/characters/aatrox/skins/base/aatrox.dds";

    /// ADR 0003: the stored path is the authored form. Lowercasing it flattened
    /// every modpkg extraction.
    #[test]
    fn new_keeps_the_authored_casing() {
        assert_eq!(ChunkPath::new(IN_WAD).as_str(), IN_WAD);
    }

    #[test]
    fn new_converts_backslashes() {
        assert_eq!(
            ChunkPath::new("ASSETS\\Characters\\Aatrox\\Skins\\Base\\Aatrox.dds").as_str(),
            IN_WAD
        );
    }

    #[test]
    fn new_is_idempotent() {
        let once = ChunkPath::new("ASSETS\\Characters/Aatrox\\Skins/Base/Aatrox.dds");
        let twice = ChunkPath::new(once.as_str());

        assert_eq!(once.as_str(), twice.as_str());
    }

    /// Two spellings of one path were the same `ChunkPath` before ADR 0003 and
    /// must still be: identity is the canonical name, not the stored path.
    #[test]
    fn spellings_of_one_path_are_equal_and_hash_alike() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let authored = ChunkPath::new(IN_WAD);
        let flattened = ChunkPath::new(CANONICAL);

        assert_eq!(authored, flattened);
        assert_eq!(authored.cmp(&flattened), std::cmp::Ordering::Equal);

        // Qualified because the inherent `hash()` (the xxh64) shadows the
        // trait method.
        let mut first = DefaultHasher::new();
        Hash::hash(&authored, &mut first);
        let mut second = DefaultHasher::new();
        Hash::hash(&flattened, &mut second);
        assert_eq!(first.finish(), second.finish());
    }

    /// Canonicalization is ASCII-lowercasing, matching `ltk_hashtable`'s keys
    /// and `ltk_hash`: a non-ASCII character never folds, so `É` and `é` are
    /// two different chunks.
    #[test]
    fn canonicalization_is_ascii_only() {
        assert_ne!(
            ChunkPath::new("data/É.bin").hash(),
            ChunkPath::new("data/é.bin").hash()
        );
        assert_ne!(ChunkPath::new("data/É.bin"), ChunkPath::new("data/é.bin"));
    }

    /// Case decides nothing, so ordering falls to what differs after it.
    #[test]
    fn ordering_is_case_insensitive() {
        // A derived (byte-wise) order would put `B` (0x42) before `a` (0x61).
        assert!(ChunkPath::new("B/x.bin") > ChunkPath::new("a/y.bin"));
        assert!(ChunkPath::new("a/x.bin") < ChunkPath::new("B/y.bin"));
    }

    /// The whole point of the type: paths that differ only in case or
    /// separator are the same chunk, so they must hash alike.
    #[test]
    fn hash_is_independent_of_case_and_separator() {
        let forward = ChunkPath::new(CANONICAL);
        let back = ChunkPath::new("assets\\characters\\aatrox\\skins\\base\\aatrox.dds");
        let mixed = ChunkPath::new("ASSETS\\Characters/Aatrox\\Skins/Base/Aatrox.dds");

        assert_eq!(forward.hash(), back.hash());
        assert_eq!(forward.hash(), mixed.hash());
    }

    /// The stored bytes must not move: the hash of an already-normalized path
    /// is what every previously written package recorded.
    #[test]
    fn hash_matches_xxhash64_of_the_canonical_string() {
        assert_eq!(
            ChunkPath::new(IN_WAD).hash(),
            PathHash::new(xxh64::xxh64(CANONICAL.as_bytes(), 0))
        );
    }
}
