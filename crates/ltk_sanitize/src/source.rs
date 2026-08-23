//! Chunk access abstraction the checks run over.
//!
//! The verification logic only ever asks two questions about a WAD-like
//! container: "is a chunk with this path hash present?" and "give me its
//! decompressed bytes". Notably it never reads TOC checksums — they are
//! declared by an untrusted WAD and prove nothing about the content (see
//! [`RefStatus`](crate::check::RefStatus)). Abstracting the two questions
//! behind [`ChunkSource`] lets the same checks run over a mounted WAD file
//! (built overlays, original game WADs, the in-game verifier's
//! memory-mapped pairs) and over a mod archive virtually merged onto the
//! original WAD — so untrusted archives can be checked without extracting
//! them to disk.

use std::io::{Read, Seek};

use ltk_wad::Wad;

/// Read access to one WAD-like set of chunks, keyed by xxh64 path hash.
pub trait ChunkSource {
    /// Whether a chunk with this name hash is present.
    fn contains(&mut self, name_hash: u64) -> bool;

    /// Decompressed chunk bytes. `Err` carries a human-readable reason for a
    /// chunk that is present but cannot be read (corruption); callers decide
    /// whether that is fatal.
    fn load(&mut self, name_hash: u64) -> Result<Vec<u8>, String>;
}

/// [`ChunkSource`] over a mounted [`Wad`].
pub struct WadChunkSource<'a, TSource: Read + Seek>(pub &'a mut Wad<TSource>);

impl<TSource: Read + Seek> ChunkSource for WadChunkSource<'_, TSource> {
    fn contains(&mut self, name_hash: u64) -> bool {
        self.0.chunks().get(name_hash).is_some()
    }

    fn load(&mut self, name_hash: u64) -> Result<Vec<u8>, String> {
        let chunk = self
            .0
            .chunks()
            .get(name_hash)
            .copied()
            .ok_or_else(|| "chunk not present in WAD".to_string())?;
        self.0
            .load_chunk_decompressed(&chunk)
            .map(|data| data.into_vec())
            .map_err(|err| err.to_string())
    }
}

/// Two sources layered: `overlay` wins, `base` fills the rest — the merged
/// view of a mod's chunks on top of the original game WAD, without building
/// (or extracting) anything.
pub struct VirtualMerge<'a> {
    pub overlay: &'a mut dyn ChunkSource,
    pub base: &'a mut dyn ChunkSource,
}

impl ChunkSource for VirtualMerge<'_> {
    fn contains(&mut self, name_hash: u64) -> bool {
        self.overlay.contains(name_hash) || self.base.contains(name_hash)
    }

    fn load(&mut self, name_hash: u64) -> Result<Vec<u8>, String> {
        if self.overlay.contains(name_hash) {
            self.overlay.load(name_hash)
        } else {
            self.base.load(name_hash)
        }
    }
}
