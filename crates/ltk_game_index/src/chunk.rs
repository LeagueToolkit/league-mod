//! Chunk rows of the chunk index.

use ltk_hash::Hash as _;
use ltk_wad::WadHash;
use serde::{Deserialize, Serialize};

use crate::ArchiveId;

/// One copy of a chunk in one holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChunkCopy {
    /// The holder.
    pub archive: ArchiveId,
    /// The holder's table-of-contents checksum for this chunk.
    pub checksum: u64,
}

/// What the index stores for one chunk hash.
///
/// A row has at least one copy. Copies are in archive id order, one per holder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRow {
    size: u64,
    copies: Vec<ChunkCopy>,
}

impl ChunkRow {
    pub(crate) fn new(size: u64, copies: Vec<ChunkCopy>) -> Self {
        debug_assert!(!copies.is_empty(), "a chunk row holds at least one copy");
        Self { size, copies }
    }

    /// Uncompressed size in bytes, as the first holder's table of contents states it.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Every copy, in archive id order.
    #[must_use]
    pub fn copies(&self) -> &[ChunkCopy] {
        &self.copies
    }

    /// Every holder, in archive id order.
    pub fn holders(&self) -> impl Iterator<Item = ArchiveId> + '_ {
        self.copies.iter().map(|copy| copy.archive)
    }

    /// The first holder in archive id order.
    #[must_use]
    pub fn first_holder(&self) -> ArchiveId {
        self.copies[0].archive
    }

    /// Whether every copy carries the same checksum.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        let first = self.copies[0].checksum;
        self.copies.iter().all(|copy| copy.checksum == first)
    }
}

/// The chunk hash of a path: XXH64, seed zero, over the ASCII-lowercased path.
///
/// `Target::chunk_hash()` in `ltk_game_data` computes the same value.
#[must_use]
pub fn chunk_hash(path: &str) -> WadHash {
    WadHash::hash_str(path)
}
