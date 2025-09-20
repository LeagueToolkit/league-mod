use crate::ModpkgCompression;
use binrw::binrw;

/// Layer index value indicating that a chunk does not belong to any layer
pub const NO_LAYER_INDEX: i32 = -1;

/// Wad index value indicating that a chunk does not belong to any `wad` file
pub const NO_WAD_INDEX: i32 = -1;

/// Layer hash value used for chunks that do not belong to any layer
pub const NO_LAYER_HASH: u64 = u64::MAX;

/// Wad hash value used for chunks that do not belong to any `wad` file
#[allow(dead_code)] // May be used in future features
pub const NO_WAD_HASH: u64 = u64::MAX;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct ModpkgChunk {
    pub path_hash: u64,

    pub data_offset: u64,
    pub compression: ModpkgCompression,
    pub compressed_size: u64,
    pub uncompressed_size: u64,

    pub compressed_checksum: u64,
    pub uncompressed_checksum: u64,

    pub path_index: u32,
    pub layer_index: i32,
    pub wad_index: i32,
}

impl ModpkgChunk {
    pub fn size_of() -> usize {
        (std::mem::size_of::<u64>() * 6)
            + std::mem::size_of::<u32>()
            + (std::mem::size_of::<i32>() * 2)
            + 1
    }

    /// Get the layer index as an Option, where NO_LAYER_INDEX represents None (no layer)
    pub fn layer(&self) -> Option<i32> {
        if self.layer_index == NO_LAYER_INDEX {
            None
        } else {
            Some(self.layer_index)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use binrw::BinWrite;

    use super::*;

    #[test]
    fn test_size_of() {
        let chunk = ModpkgChunk::default();

        let mut writer = Cursor::new(Vec::new());
        chunk.write(&mut writer).unwrap();

        assert_eq!(writer.position() as usize, ModpkgChunk::size_of());
    }
}
