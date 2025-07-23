use binrw::{binrw, BinRead};
use byteorder::{ReadBytesExt, LE};
use chunk::ModpkgChunk;
use error::ModpkgError;
use io_ext::ReaderExt;
use std::{
    collections::HashMap,
    fmt::Display,
    io::{BufReader, Read, Seek, SeekFrom},
};

pub mod builder;
mod chunk;
mod decoder;
mod error;
mod extractor;
mod license;
mod metadata;
pub mod utils;

pub use decoder::ModpkgDecoder;
pub use extractor::ModpkgExtractor;
pub use license::*;
pub use metadata::*;
pub use utils::*;

#[derive(Debug, PartialEq)]
pub struct Modpkg<TSource: Read + Seek> {
    signature: Vec<u8>,

    pub layers: HashMap<u64, ModpkgLayer>,
    pub chunk_paths: HashMap<u64, String>,

    pub metadata: ModpkgMetadata,

    /// The chunks in the mod package.
    ///
    /// The key is a tuple of the path hash and the layer hash.
    pub chunks: HashMap<(u64, u64), ModpkgChunk>,

    /// The original byte source.
    source: TSource,
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ModpkgLayer {
    #[br(temp)]
    #[bw(calc = name.len() as u32)]
    name_len: u32,
    #[br(count = name_len, try_map = String::from_utf8)]
    #[bw(map = |s| s.as_bytes().to_vec())]
    pub name: String,

    pub priority: i32,
}

#[binrw]
#[brw(little, repr = u8)]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Default)]
pub enum ModpkgCompression {
    #[default]
    None = 0,
    Zstd = 1,
}

impl<TSource: Read + Seek> Modpkg<TSource> {
    const MAGIC: [u8; 8] = *b"_modpkg_";

    pub fn mount_from_reader(mut source: TSource) -> Result<Self, ModpkgError> {
        let mut reader = BufReader::new(&mut source);

        let magic = reader.read_u64::<LE>()?;
        if magic != u64::from_le_bytes(Self::MAGIC) {
            return Err(ModpkgError::InvalidMagic(magic));
        }

        let version = reader.read_u32::<LE>()?;
        if version != 1 {
            return Err(ModpkgError::InvalidVersion(version));
        }

        let signature_size = reader.read_u32::<LE>()?;
        let chunk_count = reader.read_u32::<LE>()?;

        let mut signature = vec![0; signature_size as usize];
        reader.read_exact(&mut signature)?;

        let layer_count = reader.read_u32::<LE>()?;
        let mut layers = HashMap::new();
        for _ in 0..layer_count {
            let layer = ModpkgLayer::read(&mut reader)?;
            layers.insert(hash_layer_name(&layer.name), layer);
        }

        let chunk_paths_count = reader.read_u32::<LE>()?;
        let mut chunk_paths = HashMap::new();
        for _ in 0..chunk_paths_count {
            let chunk_path = reader.read_str_until_nul()?;
            let chunk_path_hash = hash_chunk_name(&chunk_path);
            chunk_paths.insert(chunk_path_hash, chunk_path);
        }

        let metadata = ModpkgMetadata::read(&mut reader)?;

        // Skip alignment
        let position = reader.stream_position()?;
        reader.seek(SeekFrom::Current(((8 - (position % 8)) % 8) as i64))?;

        let mut chunks = HashMap::new();
        for _ in 0..chunk_count {
            let chunk = ModpkgChunk::read(&mut reader)?;
            chunks.insert((chunk.path_hash, chunk.layer_hash), chunk);
        }

        Ok(Self {
            signature,
            layers,
            chunk_paths,
            metadata,
            chunks,
            source,
        })
    }

    /// Create a decoder for this modpkg
    pub fn decoder(&mut self) -> ModpkgDecoder<TSource> {
        ModpkgDecoder {
            source: &mut self.source,
        }
    }

    /// Load the raw data of a chunk
    pub fn load_chunk_raw(
        &mut self,
        path_hash: u64,
        layer_hash: u64,
    ) -> Result<Box<[u8]>, ModpkgError> {
        let chunk = match self.chunks.get(&(path_hash, layer_hash)) {
            Some(chunk) => *chunk,
            None => return Err(ModpkgError::MissingChunk(path_hash)),
        };
        self.decoder().load_chunk_raw(&chunk)
    }

    /// Load and decompress the data of a chunk
    pub fn load_chunk_decompressed(
        &mut self,
        path_hash: u64,
        layer_hash: u64,
    ) -> Result<Box<[u8]>, ModpkgError> {
        let chunk = match self.chunks.get(&(path_hash, layer_hash)) {
            Some(chunk) => *chunk,
            None => return Err(ModpkgError::MissingChunk(path_hash)),
        };
        self.decoder().load_chunk_decompressed(&chunk)
    }

    /// Load the raw data of a chunk by path and layer name
    pub fn load_chunk_raw_by_path(
        &mut self,
        path: &str,
        layer: &str,
    ) -> Result<Box<[u8]>, ModpkgError> {
        let path_hash = hash_chunk_name(path);
        let layer_hash = hash_layer_name(layer);

        self.load_chunk_raw(path_hash, layer_hash)
    }

    /// Load and decompress the data of a chunk by path and layer name
    pub fn load_chunk_decompressed_by_path(
        &mut self,
        path: &str,
        layer: &str,
    ) -> Result<Box<[u8]>, ModpkgError> {
        let path_hash = hash_chunk_name(path);
        let layer_hash = hash_layer_name(layer);

        self.load_chunk_decompressed(path_hash, layer_hash)
    }

    /// Get a chunk by path and layer name
    pub fn get_chunk(&self, path: &str, layer: &str) -> Result<&ModpkgChunk, ModpkgError> {
        let path_hash = hash_chunk_name(path);
        let layer_hash = hash_layer_name(layer);

        self.chunks
            .get(&(path_hash, layer_hash))
            .ok_or(ModpkgError::MissingChunk(path_hash))
    }

    /// Check if a chunk exists by path and layer name
    pub fn has_chunk(&self, path: &str, layer: &str) -> Result<bool, ModpkgError> {
        let path_hash = hash_chunk_name(path);
        let layer_hash = hash_layer_name(layer);

        Ok(self.chunks.contains_key(&(path_hash, layer_hash)))
    }
}

impl Display for ModpkgCompression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}",
            match self {
                ModpkgCompression::None => "none",
                ModpkgCompression::Zstd => "zstd",
            }
        )
    }
}

impl TryFrom<u8> for ModpkgCompression {
    type Error = ModpkgError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => ModpkgCompression::None,
            1 => ModpkgCompression::Zstd,
            _ => return Err(ModpkgError::InvalidCompressionType(value)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder};
    use std::io::{Cursor, Write};

    #[test]
    fn test_load_chunk() {
        // Create a test modpkg in memory
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let test_data = [0xAA; 100];
        let path = "test.bin";
        let path_hash = hash_chunk_name(path);
        let layer_name = "base";
        let layer_hash = hash_layer_name(layer_name);

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(path)
                    .unwrap()
                    .with_compression(ModpkgCompression::Zstd),
            );

        builder
            .build_to_writer(&mut cursor, |_, cursor| {
                cursor.write_all(&test_data)?;
                Ok(())
            })
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Test raw loading by hash
        let raw_data = modpkg.load_chunk_raw(path_hash, layer_hash).unwrap();
        let chunk = *modpkg.chunks.get(&(path_hash, layer_hash)).unwrap();
        assert_eq!(raw_data.len(), chunk.compressed_size as usize);

        // Test decompressed loading by hash
        let decompressed_data = modpkg.decoder().load_chunk_decompressed(&chunk).unwrap();
        assert_eq!(decompressed_data.len(), chunk.uncompressed_size as usize);
        assert_eq!(&decompressed_data[..], &test_data[..]);

        // Test raw loading by path
        let raw_data_by_path = modpkg.load_chunk_raw_by_path(path, layer_name).unwrap();
        assert_eq!(raw_data_by_path.len(), chunk.compressed_size as usize);

        // Test decompressed loading by path
        let decompressed_data_by_path = modpkg
            .load_chunk_decompressed_by_path(path, layer_name)
            .unwrap();
        assert_eq!(
            decompressed_data_by_path.len(),
            chunk.uncompressed_size as usize
        );
        assert_eq!(&decompressed_data_by_path[..], &test_data[..]);
    }

    #[test]
    fn test_load_hex_chunk() {
        // Create a test modpkg in memory
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let test_data = [0xBB; 100];
        let hex_path = "deadbeef";
        let layer_name = "base";

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(hex_path)
                    .unwrap()
                    .with_compression(ModpkgCompression::None),
            );

        builder
            .build_to_writer(&mut cursor, |_, cursor| {
                cursor.write_all(&test_data)?;
                Ok(())
            })
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Test loading by hex path
        let data_by_hex_path = modpkg
            .load_chunk_decompressed_by_path(hex_path, layer_name)
            .unwrap();
        assert_eq!(&data_by_hex_path[..], &test_data[..]);
    }

    #[test]
    fn test_has_and_get_chunk() {
        // Create a test modpkg in memory
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let test_data = [0xCC; 100];
        let path = "test.bin";
        let hex_path = "abcdef";
        let layer_name = "base";

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(path)
                    .unwrap()
                    .with_compression(ModpkgCompression::None),
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(hex_path)
                    .unwrap()
                    .with_compression(ModpkgCompression::None),
            );

        builder
            .build_to_writer(&mut cursor, |_, cursor| {
                cursor.write_all(&test_data)?;
                Ok(())
            })
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Test has_chunk
        assert!(modpkg.has_chunk(path, layer_name).unwrap());
        assert!(modpkg.has_chunk(hex_path, layer_name).unwrap());
        assert!(!modpkg.has_chunk("nonexistent", layer_name).unwrap());

        // Test get_chunk
        let chunk = modpkg.get_chunk(path, layer_name).unwrap();
        assert_eq!(chunk.uncompressed_size, 100);
        assert_eq!(chunk.compression, ModpkgCompression::None);

        let hex_chunk = modpkg.get_chunk(hex_path, layer_name).unwrap();
        assert_eq!(hex_chunk.uncompressed_size, 100);
        assert_eq!(hex_chunk.compression, ModpkgCompression::None);

        assert!(modpkg.get_chunk("nonexistent", layer_name).is_err());
    }
}
