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
pub mod error;
mod extractor;
mod license;
mod metadata;
mod read;
mod readme;
mod thumbnail;
pub mod utils;

#[cfg(feature = "project")]
pub mod project;

pub use decoder::ModpkgDecoder;
pub use extractor::ModpkgExtractor;
pub use license::*;
pub use metadata::*;
pub use readme::*;
pub use thumbnail::*;
pub use utils::*;

/// The name of the base layer.
pub const BASE_LAYER_NAME: &str = "base";

/// A batch-loaded chunk entry: `(path_hash, layer_hash, decompressed_data)`.
pub type BatchChunkEntry = (u64, u64, Box<[u8]>);

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

    /// Secondary index: chunks grouped by (wad_index, layer_index).
    ///
    /// Values are chunk keys `(path_hash, layer_hash)` that can be looked up in `chunks`.
    pub chunks_by_wad_layer: HashMap<(u32, u32), Vec<(u64, u64)>>,

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

    /// Resolve the chunk key `(path_hash, layer_hash)` for a given path and layer,
    /// handling both literal and hex-encoded chunk names.
    ///
    /// Returns the first matching key, or `Err` if no chunk matches.
    fn resolve_chunk_key(
        &self,
        path: &str,
        layer: Option<&str>,
    ) -> Result<(u64, u64), ModpkgError> {
        let normalized = utils::normalize_chunk_path(path);
        let literal_hash = hash_chunk_name(&normalized);
        let layer_hash = match layer {
            Some(name) => hash_layer_name(name),
            None => NO_LAYER_HASH,
        };

        if self.chunks.contains_key(&(literal_hash, layer_hash)) {
            return Ok((literal_hash, layer_hash));
        }

        // Try hex-encoded chunk name fallback (e.g., "abcdef1234567890.dds")
        let filename_lower = Path::new(&normalized)
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_lowercase)
            .unwrap_or_else(|| normalized.to_lowercase());

        if utils::is_hex_chunk_name(&filename_lower) {
            if let Some(base) = filename_lower.split('.').next() {
                if let Ok(parsed) = u64::from_str_radix(base, 16) {
                    if self.chunks.contains_key(&(parsed, layer_hash)) {
                        return Ok((parsed, layer_hash));
                    }
                }
            }
        }

        Err(ModpkgError::MissingChunk(literal_hash))
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
        let (ph, lh) = self.resolve_chunk_key(path, layer)?;
        self.load_chunk_raw(ph, lh)
    }

    /// Load and decompress the data of a chunk by path and layer name
    pub fn load_chunk_decompressed_by_path(
        &mut self,
        path: &str,
        layer: Option<&str>,
    ) -> Result<Box<[u8]>, ModpkgError> {
        let (ph, lh) = self.resolve_chunk_key(path, layer)?;
        self.load_chunk_decompressed_by_hash(ph, lh)
    }

    /// Get a chunk by path and layer name
    pub fn get_chunk(&self, path: &str, layer: Option<&str>) -> Result<&ModpkgChunk, ModpkgError> {
        let (ph, lh) = self.resolve_chunk_key(path, layer)?;
        Ok(self.chunks.get(&(ph, lh)).unwrap())
    }

    /// Load a chunk into memory
    pub fn load_chunk_decompressed(
        &mut self,
        chunk: &ModpkgChunk,
    ) -> Result<Box<[u8]>, ModpkgError> {
        self.decoder().load_chunk_decompressed(chunk)
    }

    /// Check if a chunk exists by path and layer name
    pub fn has_chunk(&self, path: &str, layer: Option<&str>) -> bool {
        self.resolve_chunk_key(path, layer).is_ok()
    }

    /// Resolve a layer name to its index in the layer table.
    pub fn layer_index(&self, layer: &str) -> Option<u32> {
        let layer_hash = hash_layer_name(layer);
        self.layer_indices
            .iter()
            .position(|&h| h == layer_hash)
            .map(|idx| idx as u32)
    }

    /// Resolve a WAD name to its index in the WAD table.
    pub fn wad_index(&self, wad_name: &str) -> Option<u32> {
        let wad_hash = hash_wad_name(wad_name);
        self.wads_indices
            .iter()
            .position(|&h| h == wad_hash)
            .map(|idx| idx as u32)
    }

    /// Get the WAD name for a given WAD index, or `None` if the index is invalid.
    pub fn wad_name_for_index(&self, wad_index: u32) -> Option<&str> {
        let wad_hash = self.wads_indices.get(wad_index as usize)?;
        self.wads.get(wad_hash).map(|s| s.as_str())
    }

    /// Get the chunk keys for a given (wad_index, layer_index) pair.
    ///
    /// Returns an empty slice if no chunks match.
    pub fn chunks_for_wad_layer(&self, wad_index: u32, layer_index: u32) -> &[(u64, u64)] {
        self.chunks_by_wad_layer
            .get(&(wad_index, layer_index))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Load and decompress multiple chunks in offset-sorted order for better I/O performance.
    ///
    /// Returns `(path_hash, layer_hash, data)` tuples in arbitrary order.
    pub fn load_chunks_batch(
        &mut self,
        keys: &[(u64, u64)],
    ) -> Result<Vec<BatchChunkEntry>, ModpkgError> {
        // Resolve keys to chunks and sort by data_offset for sequential I/O
        let mut sorted: Vec<_> = keys
            .iter()
            .filter_map(|&(ph, lh)| self.chunks.get(&(ph, lh)).map(|c| (ph, lh, *c)))
            .collect();
        sorted.sort_by_key(|(_, _, c)| c.data_offset);

        let mut results = Vec::with_capacity(sorted.len());
        let mut decoder = ModpkgDecoder {
            source: &mut self.source,
        };
        for (ph, lh, chunk) in &sorted {
            let data = decoder.load_chunk_decompressed(chunk)?;
            results.push((*ph, *lh, data));
        }
        Ok(results)
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
        assert!(modpkg.has_chunk(path, Some(layer_name)));
        assert!(modpkg.has_chunk(hex_path, Some(layer_name)));
        assert!(!modpkg.has_chunk("nonexistent", Some(layer_name)));

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
