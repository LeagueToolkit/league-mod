use binrw::binrw;
use std::{
    collections::HashMap,
    fmt::Display,
    io::{Read, Seek},
    path::Path,
};

pub mod builder;
mod chunk;
mod chunk_path;
mod decoder;
pub mod error;
mod extractor;
mod hashes;
mod indices;
mod license;
mod metadata;
mod read;
mod readme;
mod slug;
mod thumbnail;

pub use chunk::ModpkgChunk;
pub use chunk_path::ChunkPath;
pub use decoder::ModpkgDecoder;
pub use error::{InvalidSlugError, ModpkgError};
pub use extractor::ModpkgExtractor;
pub use hashes::{ChunkKey, LayerHash, PathHash, WadHash};
pub use indices::{LayerIndex, WadIndex};
pub use license::*;
pub use metadata::*;
pub use readme::*;
pub use slug::Slug;
pub use thumbnail::*;

/// The name of the base layer.
pub const BASE_LAYER_NAME: &str = "base";

/// A batch-loaded chunk entry: its key and decompressed data.
pub type BatchChunkEntry = (ChunkKey, Box<[u8]>);

/// The name of the metadata folder inside the mod package.
pub const METADATA_FOLDER_NAME: &str = "_meta_";

#[derive(Debug, PartialEq)]
pub struct Modpkg<TSource: Read + Seek> {
    signature: Vec<u8>,

    layer_indices: Vec<LayerHash>,
    layers: HashMap<LayerHash, ModpkgLayer>,

    chunk_path_indices: Vec<PathHash>,
    chunk_paths: HashMap<PathHash, String>,

    wad_indices: Vec<WadHash>,
    wads: HashMap<WadHash, String>,

    chunks: HashMap<ChunkKey, ModpkgChunk>,

    // Secondary index: chunk keys grouped by (wad_index, layer_index). A key
    // appears under every WAD group whose records referenced it.
    chunks_by_wad_layer: HashMap<(WadIndex, LayerIndex), Vec<ChunkKey>>,

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

impl ModpkgCompression {
    /// The compression to request for a content file, chosen from its extension.
    ///
    /// Wwise audio containers (`.bnk`/`.wpk`) are always stored uncompressed,
    /// mirroring how the overlay builder treats them in game WADs. Everything
    /// else requests Zstd; the builder stores a chunk raw when compression
    /// doesn't meaningfully reduce its size, so already-compressed formats
    /// need no special-casing here.
    pub fn for_extension(ext: Option<&str>) -> Self {
        match ext.map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("bnk" | "wpk") => Self::None,
            _ => Self::Zstd,
        }
    }
}

impl<TSource: Read + Seek> Modpkg<TSource> {
    /// Create a decoder for this modpkg
    pub fn decoder(&'_ mut self) -> ModpkgDecoder<'_, TSource> {
        ModpkgDecoder {
            source: &mut self.source,
        }
    }

    /// The chunks in the package.
    ///
    /// A chunk registered under several WADs appears here once; each of its
    /// WAD memberships is listed by
    /// [`chunks_for_wad_layer`](Self::chunks_for_wad_layer).
    pub fn chunks(&self) -> &HashMap<ChunkKey, ModpkgChunk> {
        &self.chunks
    }

    /// The layers in the package, keyed by the hash of their name.
    pub fn layers(&self) -> &HashMap<LayerHash, ModpkgLayer> {
        &self.layers
    }

    /// The WAD names in the package, keyed by their hash.
    pub fn wads(&self) -> &HashMap<WadHash, String> {
        &self.wads
    }

    /// The number of entries in the package's WAD table.
    ///
    /// Positions `0..wad_count()`, wrapped in a [`WadIndex`], are valid inputs
    /// to [`wad_name_for_index`](Self::wad_name_for_index) and
    /// [`chunks_for_wad_layer`](Self::chunks_for_wad_layer).
    pub fn wad_count(&self) -> usize {
        self.wad_indices.len()
    }

    /// The chunk paths in the package, keyed by their hash.
    pub fn chunk_paths(&self) -> &HashMap<PathHash, String> {
        &self.chunk_paths
    }

    /// Resolve the [`ChunkKey`] for a given path and layer, handling both
    /// literal and hex-encoded chunk names.
    ///
    /// Returns the first matching key, or `Err` if no chunk matches.
    fn resolve_chunk_key(&self, path: &str, layer: Option<&str>) -> Result<ChunkKey, ModpkgError> {
        let normalized = ChunkPath::new(path);
        let literal_hash = normalized.hash();
        let layer_hash = match layer {
            Some(name) => LayerHash::from_name(name),
            None => LayerHash::NONE,
        };

        let literal_key = ChunkKey::new(literal_hash, layer_hash);
        if self.chunks.contains_key(&literal_key) {
            return Ok(literal_key);
        }

        // Try hex-encoded chunk name fallback (e.g., "abcdef1234567890.dds").
        let file_name = Path::new(normalized.as_str())
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(normalized.as_str());

        if let Some(hash) = PathHash::from_hex_name(file_name) {
            let hex_key = ChunkKey::new(hash, layer_hash);
            if self.chunks.contains_key(&hex_key) {
                return Ok(hex_key);
            }
        }

        Err(ModpkgError::MissingChunk(literal_hash))
    }

    /// Load the raw data of a chunk using its key
    pub fn load_chunk_raw(&mut self, key: ChunkKey) -> Result<Box<[u8]>, ModpkgError> {
        let chunk = match self.chunks.get(&key) {
            Some(chunk) => *chunk,
            None => return Err(ModpkgError::MissingChunk(key.path)),
        };
        self.decoder().load_chunk_raw(&chunk)
    }

    /// Load and decompress the data of a chunk using its key
    pub fn load_chunk_decompressed(&mut self, key: ChunkKey) -> Result<Box<[u8]>, ModpkgError> {
        let chunk = match self.chunks.get(&key) {
            Some(chunk) => *chunk,
            None => return Err(ModpkgError::MissingChunk(key.path)),
        };
        self.decoder().load_chunk_decompressed(&chunk)
    }

    /// Load the raw data of a chunk by path and layer name
    pub fn load_chunk_raw_by_path(
        &mut self,
        path: &str,
        layer: Option<&str>,
    ) -> Result<Box<[u8]>, ModpkgError> {
        let key = self.resolve_chunk_key(path, layer)?;
        self.load_chunk_raw(key)
    }

    /// Load and decompress the data of a chunk by path and layer name
    pub fn load_chunk_decompressed_by_path(
        &mut self,
        path: &str,
        layer: Option<&str>,
    ) -> Result<Box<[u8]>, ModpkgError> {
        let key = self.resolve_chunk_key(path, layer)?;
        self.load_chunk_decompressed(key)
    }

    /// Look up a chunk's record by path and layer name.
    ///
    /// # Errors
    ///
    /// Returns [`ModpkgError::MissingChunk`] when no chunk matches.
    pub fn chunk(&self, path: &str, layer: Option<&str>) -> Result<&ModpkgChunk, ModpkgError> {
        let key = self.resolve_chunk_key(path, layer)?;
        Ok(self.chunks.get(&key).unwrap())
    }

    /// Check if a chunk exists by path and layer name
    pub fn has_chunk(&self, path: &str, layer: Option<&str>) -> bool {
        self.resolve_chunk_key(path, layer).is_ok()
    }

    /// Resolve a layer name to its position in the layer table.
    pub fn layer_index(&self, layer: &str) -> Option<LayerIndex> {
        let layer_hash = LayerHash::from_name(layer);
        self.layer_indices
            .iter()
            .position(|&h| h == layer_hash)
            .map(|idx| LayerIndex::new(idx as u32))
    }

    /// Resolve a WAD name to its position in the WAD table.
    pub fn wad_index(&self, wad_name: &str) -> Option<WadIndex> {
        let wad_hash = WadHash::from_name(wad_name);
        self.wad_indices
            .iter()
            .position(|&h| h == wad_hash)
            .map(|idx| WadIndex::new(idx as u32))
    }

    /// Get the WAD name for a given WAD index, or `None` if the index is invalid.
    pub fn wad_name_for_index(&self, wad_index: WadIndex) -> Option<&str> {
        let wad_hash = self.wad_indices.get(wad_index.value() as usize)?;
        self.wads.get(wad_hash).map(|s| s.as_str())
    }

    /// Get the chunk keys for a given (wad_index, layer_index) pair.
    ///
    /// Returns an empty slice if no chunks match.
    pub fn chunks_for_wad_layer(
        &self,
        wad_index: WadIndex,
        layer_index: LayerIndex,
    ) -> &[ChunkKey] {
        self.chunks_by_wad_layer
            .get(&(wad_index, layer_index))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Load and decompress multiple chunks in offset-sorted order for better I/O performance.
    ///
    /// Returns `(key, data)` entries in arbitrary order.
    pub fn load_chunks_batch(
        &mut self,
        keys: &[ChunkKey],
    ) -> Result<Vec<BatchChunkEntry>, ModpkgError> {
        // Resolve keys to chunks and sort by data_offset for sequential I/O
        let mut sorted: Vec<_> = keys
            .iter()
            .filter_map(|&key| self.chunks.get(&key).map(|c| (key, *c)))
            .collect();
        sorted.sort_by_key(|(_, c)| c.data_offset);

        let mut results = Vec::with_capacity(sorted.len());
        let mut decoder = ModpkgDecoder {
            source: &mut self.source,
        };
        for (key, chunk) in &sorted {
            let data = decoder.load_chunk_decompressed(chunk)?;
            results.push((*key, data));
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
    use std::io::Cursor;

    #[test]
    fn test_compression_for_extension() {
        // Wwise audio containers are never compressed
        assert_eq!(
            ModpkgCompression::for_extension(Some("bnk")),
            ModpkgCompression::None
        );
        assert_eq!(
            ModpkgCompression::for_extension(Some("WPK")),
            ModpkgCompression::None
        );

        // Everything else requests Zstd (the builder falls back to raw storage
        // per chunk when compression doesn't pay)
        assert_eq!(
            ModpkgCompression::for_extension(Some("dds")),
            ModpkgCompression::Zstd
        );
        assert_eq!(
            ModpkgCompression::for_extension(Some("bin")),
            ModpkgCompression::Zstd
        );
        assert_eq!(
            ModpkgCompression::for_extension(None),
            ModpkgCompression::Zstd
        );
    }

    #[test]
    fn test_load_chunk() {
        // Create a test modpkg in memory
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let test_data = [0xAA; 100];
        let path = "test.bin";
        let layer_name = "base";
        let key = ChunkKey::new(
            ChunkPath::new(path).hash(),
            LayerHash::from_name(layer_name),
        );

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(path)
                    .with_compression(ModpkgCompression::Zstd),
            );

        builder
            .build_to_writer(&mut cursor, |_| Ok(test_data.to_vec()))
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Test raw loading by hash
        let raw_data = modpkg.load_chunk_raw(key).unwrap();
        let chunk = *modpkg.chunks().get(&key).unwrap();
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
            .build_to_writer(&mut cursor, |_| Ok(test_data.to_vec()))
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        println!("{:?}", modpkg.layers());
        println!("{:?}", modpkg.chunks());

        // Test loading by hex path (uses hex base of file name)
        let data_by_hex_path = modpkg
            .load_chunk_decompressed_by_path(test_chunk_path, Some(layer_name))
            .unwrap();
        assert_eq!(&data_by_hex_path[..], &test_data[..]);
    }

    #[test]
    fn test_has_chunk_and_lookup() {
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
                    .with_compression(ModpkgCompression::None),
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_hashed_chunk_name(hex_path)
                    .unwrap()
                    .with_compression(ModpkgCompression::None),
            );

        builder
            .build_to_writer(&mut cursor, |_| Ok(test_data.to_vec()))
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Test has_chunk
        assert!(modpkg.has_chunk(path, Some(layer_name)));
        assert!(modpkg.has_chunk(hex_path, Some(layer_name)));
        assert!(!modpkg.has_chunk("nonexistent", Some(layer_name)));

        // Test chunk lookup
        let chunk = modpkg.chunk(path, Some(layer_name)).unwrap();
        assert_eq!(chunk.uncompressed_size, 100);
        assert_eq!(chunk.compression, ModpkgCompression::None);
        assert!(chunk.layer().is_some()); // Layer should be present

        let hex_chunk = modpkg.chunk(hex_path, Some(layer_name)).unwrap();
        assert_eq!(hex_chunk.uncompressed_size, 100);
        assert_eq!(hex_chunk.compression, ModpkgCompression::None);
        assert!(hex_chunk.layer().is_some()); // Layer should be present

        assert!(modpkg.chunk("nonexistent", Some(layer_name)).is_err());
    }
}
