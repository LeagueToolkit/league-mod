use std::fmt::Display;

use xxhash_rust::xxh64;

/// A chunk path in canonical form: lowercase, forward slashes.
///
/// This is the path the file has *inside its target WAD*, matching what the
/// game ships. The WAD it belongs to and the layer it came from are stored
/// separately on the chunk, so neither the `.wad.client` directory nor the
/// layer name appears here.
///
/// Chunks are keyed by the hash of their path, so a hash taken over a
/// denormalized path silently matches nothing. Normalization happens once, in
/// the constructor, which makes that state unrepresentable rather than merely
/// documented.
///
/// ```
/// use ltk_modpkg::ChunkPath;
///
/// let path = ChunkPath::new("ASSETS\\Characters\\Aatrox\\Skins\\Base\\Aatrox.dds");
///
/// assert_eq!(path.as_str(), "assets/characters/aatrox/skins/base/aatrox.dds");
/// assert_eq!(
///     path.hash(),
///     ChunkPath::new("assets/characters/aatrox/skins/base/aatrox.dds").hash()
/// );
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkPath(String);

impl ChunkPath {
    /// Normalize `path` into a chunk path.
    pub fn new(path: impl AsRef<str>) -> Self {
        Self(crate::utils::normalize_chunk_path(path.as_ref()))
    }

    /// The xxhash64 of the canonical path, as stored in the chunk table.
    pub fn hash(&self) -> u64 {
        xxh64::xxh64(self.0.as_bytes(), 0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the path, yielding the canonical string.
    pub fn into_string(self) -> String {
        self.0
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

    #[test]
    fn new_lowercases() {
        assert_eq!(ChunkPath::new(IN_WAD).as_str(), CANONICAL);
    }

    #[test]
    fn new_converts_backslashes() {
        assert_eq!(
            ChunkPath::new("assets\\characters\\aatrox\\skins\\base\\aatrox.dds").as_str(),
            CANONICAL
        );
    }

    #[test]
    fn new_is_idempotent() {
        let once = ChunkPath::new("ASSETS\\Characters/Aatrox\\Skins/Base/Aatrox.dds");
        let twice = ChunkPath::new(once.as_str());

        assert_eq!(once, twice);
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
            xxh64::xxh64(CANONICAL.as_bytes(), 0)
        );
    }
}
