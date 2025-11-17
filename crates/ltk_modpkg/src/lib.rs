use binrw::binrw;
use chunk::{ModpkgChunk, NO_LAYER_HASH};
use error::ModpkgError;
use std::{
    collections::HashMap,
    fmt::Display,
    io::{Read, Seek},
    path::Path,
};

pub mod builder;
mod chunk;
mod decoder;
mod error;
mod extractor;
mod license;
mod metadata;
mod read;
mod readme;
mod thumbnail;
pub mod utils;

pub use decoder::ModpkgDecoder;
pub use extractor::ModpkgExtractor;
pub use license::*;
pub use metadata::*;
pub use readme::*;
pub use thumbnail::*;
pub use utils::*;

/// The name of the base layer.
pub const BASE_LAYER_NAME: &str = "base";

/// The name of the metadata folder inside the mod package.
pub const METADATA_FOLDER_NAME: &str = "_meta_";

#[derive(Debug, PartialEq)]
pub struct Modpkg<TSource: Read + Seek> {
    signature: Vec<u8>,

    pub layer_indices: Vec<u64>,
    pub layers: HashMap<u64, ModpkgLayer>,

    pub chunk_path_indices: Vec<u64>,
    pub chunk_paths: HashMap<u64, String>,

    pub wads_indices: Vec<u64>,
    pub wads: HashMap<u64, String>,

    /// The chunks in the mod package.
    ///
    /// The key is a tuple of the path hash and the layer hash respectively.
    pub chunks: HashMap<(u64, u64), ModpkgChunk>,

    /// The original byte source.
    source: TSource,
}

/// Describes a layer in the mod package.
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

/// The compression type of a chunk.
#[binrw]
#[brw(little, repr = u8)]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Default)]
pub enum ModpkgCompression {
    #[default]
    None = 0,
    Zstd = 1,
}

impl<TSource: Read + Seek> Modpkg<TSource> {
    /// Create a decoder for this modpkg
    pub fn decoder(&'_ mut self) -> ModpkgDecoder<'_, TSource> {
        ModpkgDecoder {
            source: &mut self.source,
        }
    }

    fn candidate_path_hashes(path: &str) -> (u64, Option<u64>) {
        let literal_hash = hash_chunk_name(path);
        let filename_lower = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_lowercase)
            .unwrap_or_else(|| path.to_lowercase());

        if utils::is_hex_chunk_name(&filename_lower) {
            if let Some(base) = filename_lower.split('.').next() {
                if let Ok(parsed) = u64::from_str_radix(base, 16) {
                    return (literal_hash, Some(parsed));
                }
            }
        }

        (literal_hash, None)
    }

    /// Load the raw data of a chunk using the path hash and layer hash
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

    /// Load and decompress the data of a chunk using the path hash and layer hash
    pub fn load_chunk_decompressed_by_hash(
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
        layer: Option<&str>,
    ) -> Result<Box<[u8]>, ModpkgError> {
        let (literal_hash, hex_hash) = Self::candidate_path_hashes(path);
        let layer_hash = match layer {
            Some(layer_name) => hash_layer_name(layer_name),
            None => NO_LAYER_HASH,
        };

        if let Ok(data) = self.load_chunk_raw(literal_hash, layer_hash) {
            return Ok(data);
        }
        if let Some(hh) = hex_hash {
            return self.load_chunk_raw(hh, layer_hash);
        }
        self.load_chunk_raw(literal_hash, layer_hash)
    }

    /// Load and decompress the data of a chunk by path and layer name
    pub fn load_chunk_decompressed_by_path(
        &mut self,
        path: &str,
        layer: Option<&str>,
    ) -> Result<Box<[u8]>, ModpkgError> {
        let (literal_hash, hex_hash) = Self::candidate_path_hashes(path);
        let layer_hash = match layer {
            Some(layer_name) => hash_layer_name(layer_name),
            None => NO_LAYER_HASH,
        };

        if let Ok(data) = self.load_chunk_decompressed_by_hash(literal_hash, layer_hash) {
            return Ok(data);
        }
        if let Some(hh) = hex_hash {
            return self.load_chunk_decompressed_by_hash(hh, layer_hash);
        }
        self.load_chunk_decompressed_by_hash(literal_hash, layer_hash)
    }

    /// Get a chunk by path and layer name
    pub fn get_chunk(&self, path: &str, layer: Option<&str>) -> Result<&ModpkgChunk, ModpkgError> {
        let (literal_hash, hex_hash) = Self::candidate_path_hashes(path);
        let layer_hash = match layer {
            Some(layer_name) => hash_layer_name(layer_name),
            None => NO_LAYER_HASH,
        };

        if let Some(chunk) = self.chunks.get(&(literal_hash, layer_hash)) {
            return Ok(chunk);
        }
        if let Some(hh) = hex_hash {
            if let Some(chunk) = self.chunks.get(&(hh, layer_hash)) {
                return Ok(chunk);
            }
        }
        Err(ModpkgError::MissingChunk(literal_hash))
    }

    /// Load a chunk into memory
    pub fn load_chunk_decompressed(
        &mut self,
        chunk: &ModpkgChunk,
    ) -> Result<Box<[u8]>, ModpkgError> {
        self.decoder().load_chunk_decompressed(chunk)
    }

    /// Check if a chunk exists by path and layer name
    pub fn has_chunk(&self, path: &str, layer: Option<&str>) -> Result<bool, ModpkgError> {
        let (literal_hash, hex_hash) = Self::candidate_path_hashes(path);
        let layer_hash = match layer {
            Some(layer_name) => hash_layer_name(layer_name),
            None => NO_LAYER_HASH,
        };

        if self.chunks.contains_key(&(literal_hash, layer_hash)) {
            return Ok(true);
        }
        if let Some(hh) = hex_hash {
            return Ok(self.chunks.contains_key(&(hh, layer_hash)));
        }
        Ok(false)
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
        let raw_data_by_path = modpkg
            .load_chunk_raw_by_path(path, Some(layer_name))
            .unwrap();
        assert_eq!(raw_data_by_path.len(), chunk.compressed_size as usize);

        // Test decompressed loading by path
        let decompressed_data_by_path = modpkg
            .load_chunk_decompressed_by_path(path, Some(layer_name))
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
        let test_chunk_path = "abcdef1234567890.dds";
        let layer_name = "base";

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_hashed_chunk_name(test_chunk_path)
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

        println!("{:?}", modpkg.layers);
        println!("{:?}", modpkg.chunks);

        // Test loading by hex path (uses hex base of file name)
        let data_by_hex_path = modpkg
            .load_chunk_decompressed_by_path(test_chunk_path, Some(layer_name))
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
        let hex_path = "abcdef1234567890";
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
                    .with_hashed_chunk_name(hex_path)
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
        assert!(modpkg.has_chunk(path, Some(layer_name)).unwrap());
        assert!(modpkg.has_chunk(hex_path, Some(layer_name)).unwrap());
        assert!(!modpkg.has_chunk("nonexistent", Some(layer_name)).unwrap());

        // Test get_chunk
        let chunk = modpkg.get_chunk(path, Some(layer_name)).unwrap();
        assert_eq!(chunk.uncompressed_size, 100);
        assert_eq!(chunk.compression, ModpkgCompression::None);
        assert!(chunk.layer().is_some()); // Layer should be present

        let hex_chunk = modpkg.get_chunk(hex_path, Some(layer_name)).unwrap();
        assert_eq!(hex_chunk.uncompressed_size, 100);
        assert_eq!(hex_chunk.compression, ModpkgCompression::None);
        assert!(hex_chunk.layer().is_some()); // Layer should be present

        assert!(modpkg.get_chunk("nonexistent", Some(layer_name)).is_err());
    }
}
