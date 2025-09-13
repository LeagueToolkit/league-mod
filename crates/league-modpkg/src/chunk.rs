use crate::ModpkgCompression;
use binrw::binrw;

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
    pub layer_index: u32,
    pub wad_index: u32,
}

impl ModpkgChunk {
    pub fn size_of() -> usize {
        (std::mem::size_of::<u64>() * 6) + (std::mem::size_of::<u32>() * 3) + 1
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
