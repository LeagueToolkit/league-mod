use crate::{LayerIndex, ModpkgCompression, PathHash, WadIndex};
use binrw::binrw;

/// A chunk's table-of-contents record, as stored on disk.
///
/// This is a plain data record: mutating a copy of it changes nothing in the
/// archive it came from.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct ModpkgChunk {
    pub path_hash: PathHash,

    pub data_offset: u64,
    pub compression: ModpkgCompression,
    pub compressed_size: u64,
    pub uncompressed_size: u64,

    pub compressed_checksum: u64,
    pub uncompressed_checksum: u64,

    pub path_index: u32,
    pub layer_index: LayerIndex,
    pub wad_index: WadIndex,
}

impl ModpkgChunk {
    /// The size in bytes of one record in the table of contents.
    pub const RECORD_SIZE: usize =
        (std::mem::size_of::<u64>() * 6) + (std::mem::size_of::<u32>() * 3) + 1;

    /// The chunk's layer table position, or `None` for meta chunks.
    pub fn layer(&self) -> Option<LayerIndex> {
        if self.layer_index == LayerIndex::NONE {
            None
        } else {
            Some(self.layer_index)
        }
    }

    /// The chunk's WAD table position, or `None` for meta chunks.
    pub fn wad(&self) -> Option<WadIndex> {
        if self.wad_index == WadIndex::NONE {
            None
        } else {
            Some(self.wad_index)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use binrw::BinWrite;

    use super::*;

    #[test]
    fn record_size_matches_the_written_layout() {
        let chunk = ModpkgChunk::default();

        let mut writer = Cursor::new(Vec::new());
        chunk.write(&mut writer).unwrap();

        assert_eq!(writer.position() as usize, ModpkgChunk::RECORD_SIZE);
    }
}
