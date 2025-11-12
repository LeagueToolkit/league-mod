use binrw::BinWrite;
use byteorder::{WriteBytesExt, LE};
use std::collections::HashMap;
use std::io::{self, BufWriter, Cursor, Seek, SeekFrom, Write};
use std::path::Path;
use xxhash_rust::xxh3::xxh3_64;

use crate::{
    chunk::{ModpkgChunk, NO_LAYER_HASH, NO_LAYER_INDEX, NO_WAD_INDEX},
    metadata::{ModpkgMetadata, METADATA_CHUNK_PATH},
    ModpkgCompression,
};
use crate::{
    hash_chunk_name, hash_layer_name, hash_wad_name, utils, BASE_LAYER_NAME, README_CHUNK_PATH,
};

#[derive(Debug, thiserror::Error)]
pub enum ModpkgBuilderError {
    #[error("io error")]
    IoError(#[from] io::Error),

    #[error("binrw error")]
    BinWriteError(#[from] binrw::Error),

    #[error("modpkg error: {0}")]
    ModpkgError(#[from] crate::error::ModpkgError),

    #[error("unsupported compression type: {0:?}")]
    UnsupportedCompressionType(ModpkgCompression),

    #[error("missing base layer")]
    MissingBaseLayer,

    #[error("layer not found: {0}")]
    LayerNotFound(String),

    #[error("invalid chunk name: {0}")]
    InvalidChunkName(String),
}

#[derive(Debug, Clone)]
pub struct ModpkgBuilder {
    pub readme: Option<String>,
    pub metadata: ModpkgMetadata,
    pub chunks: HashMap<(u64, u64), ModpkgChunkBuilder>,
    pub meta_chunks: HashMap<(u64, u64), ModpkgChunkBuilder>,
    pub layers: Vec<ModpkgLayerBuilder>,
}

impl Default for ModpkgBuilder {
    fn default() -> Self {
        let mut builder = Self {
            readme: None,
            metadata: ModpkgMetadata::default(),
            chunks: HashMap::new(),
            meta_chunks: HashMap::new(),
            layers: Vec::new(),
        };

        // Always include metadata chunk by default
        let metadata_chunk = ModpkgChunkBuilder::new()
            .with_path(METADATA_CHUNK_PATH)
            .unwrap()
            .with_compression(ModpkgCompression::None)
            .with_layer("");
        builder.meta_chunks.insert(
            (hash_chunk_name(METADATA_CHUNK_PATH), NO_LAYER_HASH),
            metadata_chunk,
        );

        builder
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModpkgChunkBuilder {
    path_hash: u64,
    pub path: String,
    pub compression: ModpkgCompression,
    pub layer: String,
    pub wad: String,
}

#[derive(Debug, Clone, Default)]
pub struct ModpkgLayerBuilder {
    pub name: String,
    pub priority: i32,
}

impl ModpkgBuilder {
    /// Add a layer to the builder.
    pub fn with_layer(mut self, layer: ModpkgLayerBuilder) -> Self {
        self.layers.push(layer);
        self
    }

    /// Add a chunk to the builder.
    /// This will insert the chunk into the map, replacing any existing chunk with the same key.
    pub fn with_chunk(mut self, chunk: ModpkgChunkBuilder) -> Self {
        let key = chunk.key();
        self.chunks.insert(key, chunk);
        self
    }

    /// Build the Modpkg file and write it to the given writer.
    ///
    /// * `writer` - The writer to write the Modpkg file to.
    /// * `provide_chunk_data` - A function that provides the raw data for each chunk.
    pub fn build_to_writer<
        TWriter: io::Write + io::Seek,
        TChunkDataProvider: Fn(&ModpkgChunkBuilder, &mut Cursor<Vec<u8>>) -> Result<(), ModpkgBuilderError>,
    >(
        self,
        writer: &mut TWriter,
        provide_chunk_data: TChunkDataProvider,
    ) -> Result<(), ModpkgBuilderError> {
        let mut writer = BufWriter::new(writer);

        // Collect all unique paths, layers, and wads
        let (chunk_paths, chunk_path_indices) = self.collect_unique_paths();
        let (layers, _) = self.collect_unique_layers();
        let (wads, wad_indices) = self.collect_unique_wads();

        Self::validate_layers(&self.layers, &layers)?;

        let total_chunks = self.chunks.len() + self.meta_chunks.len();

        Self::write_header(&mut writer, total_chunks)?;
        Self::write_layers(&mut writer, &self.layers)?;
        Self::write_chunk_paths(&mut writer, &chunk_paths)?;
        Self::write_wads(&mut writer, &wads)?;
        Self::write_alignment(&mut writer)?;

        // Reserve space for chunk TOC
        let chunk_toc_offset = writer.stream_position()?;
        writer.write_all(&vec![0; total_chunks * ModpkgChunk::size_of()])?;

        let layer_index_map = Self::build_layer_index_map(&self.layers);

        let all_chunks = Self::process_all_chunks(
            &self.metadata,
            &self
                .chunks
                .values()
                .chain(self.meta_chunks.values())
                .collect::<Vec<_>>(),
            &mut writer,
            provide_chunk_data,
            &chunk_path_indices,
            &layer_index_map,
            &wad_indices,
        )?;

        // Go back and write the actual chunk TOC
        Self::write_chunk_toc(&mut writer, chunk_toc_offset, &all_chunks)?;

        Ok(())
    }

    fn write_header<W: io::Write>(
        writer: &mut W,
        total_chunks: usize,
    ) -> Result<(), ModpkgBuilderError> {
        // Write magic header
        writer.write_all(b"_modpkg_")?;

        // Write version
        writer.write_u32::<LE>(1)?;

        // Write signature size and chunk count
        writer.write_u32::<LE>(0)?; // Placeholder for signature size
        writer.write_u32::<LE>(total_chunks as u32)?;

        // Write signature (empty for now)
        let signature = Vec::new();
        writer.write_all(&signature)?;

        Ok(())
    }

    fn write_layers<W: io::Write>(
        writer: &mut W,
        layers: &[ModpkgLayerBuilder],
    ) -> Result<(), ModpkgBuilderError> {
        writer.write_u32::<LE>(layers.len() as u32)?;
        for layer in layers {
            writer.write_u32::<LE>(layer.name.len() as u32)?;
            writer.write_all(layer.name.as_bytes())?;
            writer.write_i32::<LE>(layer.priority)?;
        }
        Ok(())
    }

    fn write_chunk_paths<W: io::Write>(
        writer: &mut W,
        chunk_paths: &[String],
    ) -> Result<(), ModpkgBuilderError> {
        // Write count
        writer.write_u32::<LE>(chunk_paths.len() as u32)?;

        // Write all chunk paths (including meta chunks)
        for path in chunk_paths {
            writer.write_all(path.as_bytes())?;
            writer.write_all(&[0])?; // Null terminator
        }

        Ok(())
    }

    fn write_wads<W: io::Write>(writer: &mut W, wads: &[String]) -> Result<(), ModpkgBuilderError> {
        writer.write_u32::<LE>(wads.len() as u32)?;
        for wad in wads {
            writer.write_all(wad.as_bytes())?;
            writer.write_all(&[0])?; // Null terminator
        }
        Ok(())
    }

    fn write_alignment<W: io::Write + io::Seek>(writer: &mut W) -> Result<(), ModpkgBuilderError> {
        let current_pos = writer.stream_position()?;
        let padding = (8 - (current_pos % 8)) % 8;
        for _ in 0..padding {
            writer.write_all(&[0])?;
        }
        Ok(())
    }

    fn build_layer_index_map(layers: &[ModpkgLayerBuilder]) -> HashMap<u64, u32> {
        let mut layer_index_map = HashMap::new();
        for (idx, layer) in layers.iter().enumerate() {
            layer_index_map.insert(hash_layer_name(&layer.name), idx as u32);
        }
        layer_index_map
    }

    fn process_all_chunks<
        TWriter: io::Write + io::Seek,
        TChunkDataProvider: Fn(&ModpkgChunkBuilder, &mut Cursor<Vec<u8>>) -> Result<(), ModpkgBuilderError>,
    >(
        metadata: &ModpkgMetadata,
        user_chunks: &[&ModpkgChunkBuilder],
        writer: &mut BufWriter<TWriter>,
        provide_chunk_data: TChunkDataProvider,
        chunk_path_indices: &HashMap<u64, u32>,
        layer_index_map: &HashMap<u64, u32>,
        wad_indices: &HashMap<u64, u32>,
    ) -> Result<Vec<ModpkgChunk>, ModpkgBuilderError> {
        // Process metadata chunk first (it's always in meta_chunks by default)
        let metadata_path_hash = hash_chunk_name(METADATA_CHUNK_PATH);
        let metadata_chunk = Self::process_metadata_chunk(metadata, writer, chunk_path_indices)?;

        // Filter out metadata chunk from user chunks since we already processed it
        let user_chunks_filtered: Vec<_> = user_chunks
            .iter()
            .filter(|chunk| chunk.path_hash != metadata_path_hash)
            .copied()
            .collect();

        // Process remaining user chunks
        let mut user_chunks_processed = Self::process_chunks(
            &user_chunks_filtered,
            writer,
            provide_chunk_data,
            chunk_path_indices,
            layer_index_map,
            wad_indices,
        )?;

        // Combine metadata chunk with user chunks
        let mut all_chunks = vec![metadata_chunk];
        all_chunks.append(&mut user_chunks_processed);

        Ok(all_chunks)
    }

    fn write_chunk_toc<W: io::Write + io::Seek>(
        writer: &mut W,
        chunk_toc_offset: u64,
        chunks: &[ModpkgChunk],
    ) -> Result<(), ModpkgBuilderError> {
        writer.seek(SeekFrom::Start(chunk_toc_offset))?;
        for chunk in chunks {
            chunk.write(writer)?;
        }
        Ok(())
    }

    fn process_metadata_chunk<TWriter: io::Write + io::Seek>(
        metadata: &ModpkgMetadata,
        writer: &mut BufWriter<TWriter>,
        chunk_path_indices: &HashMap<u64, u32>,
    ) -> Result<ModpkgChunk, ModpkgBuilderError> {
        // Serialize metadata to msgpack
        let mut metadata_bytes = Vec::new();
        metadata.write(&mut metadata_bytes)?;

        let size = metadata_bytes.len();
        let checksum = xxh3_64(&metadata_bytes);

        let data_offset = writer.stream_position()?;
        writer.write_all(&metadata_bytes)?;

        Ok(ModpkgChunk {
            path_hash: hash_chunk_name(METADATA_CHUNK_PATH),
            data_offset,
            compression: ModpkgCompression::None,
            compressed_size: size as u64,
            uncompressed_size: size as u64,
            compressed_checksum: checksum,
            uncompressed_checksum: checksum,
            path_index: *chunk_path_indices
                .get(&hash_chunk_name(METADATA_CHUNK_PATH))
                .unwrap_or(&0),
            layer_index: NO_LAYER_INDEX,
            wad_index: NO_WAD_INDEX,
        })
    }

    fn compress_chunk_data(
        data: &[u8],
        compression: ModpkgCompression,
    ) -> Result<(Vec<u8>, ModpkgCompression), ModpkgBuilderError> {
        let mut compressed_data = Vec::new();
        match compression {
            ModpkgCompression::None => {
                compressed_data = data.to_vec();
            }
            ModpkgCompression::Zstd => {
                let mut encoder = zstd::Encoder::new(BufWriter::new(&mut compressed_data), 3)?;
                encoder.write_all(data)?;
                encoder.finish()?;
            }
        };

        Ok((compressed_data, compression))
    }

    fn collect_unique_layers(&self) -> (Vec<String>, HashMap<u64, u32>) {
        let mut layers = Vec::new();
        let mut layer_indices = HashMap::new();
        for chunk in self.chunks.values() {
            // Skip empty layer names (they represent chunks with no layer)
            if chunk.layer.is_empty() {
                continue;
            }
            let hash = hash_layer_name(&chunk.layer);
            layer_indices.entry(hash).or_insert_with(|| {
                let index = layers.len();
                layers.push(chunk.layer.clone());
                index as u32
            });
        }

        (layers, layer_indices)
    }

    fn collect_unique_paths(&self) -> (Vec<String>, HashMap<u64, u32>) {
        let mut paths = Vec::new();
        let mut path_indices = HashMap::new();

        // Collect paths from both regular chunks and meta chunks
        for chunk in self.chunks.values().chain(self.meta_chunks.values()) {
            path_indices.entry(chunk.path_hash).or_insert_with(|| {
                let index = paths.len();
                paths.push(chunk.path.clone());
                index as u32
            });
        }

        (paths, path_indices)
    }

    fn collect_unique_wads(&self) -> (Vec<String>, HashMap<u64, u32>) {
        let mut wads = Vec::new();
        let mut wad_indices = HashMap::new();
        for chunk in self.chunks.values().chain(self.meta_chunks.values()) {
            // Skip empty wad names (they represent chunks with no wad)
            if chunk.wad.is_empty() {
                continue;
            }
            wad_indices
                .entry(hash_wad_name(&chunk.wad))
                .or_insert_with(|| {
                    let index = wads.len();
                    wads.push(chunk.wad.clone());
                    index as u32
                });
        }
        (wads, wad_indices)
    }

    fn validate_layers(
        defined_layers: &[ModpkgLayerBuilder],
        unique_layers: &[String],
    ) -> Result<(), ModpkgBuilderError> {
        // Check if defined layers have base layer
        if !defined_layers
            .iter()
            .any(|layer| layer.name == BASE_LAYER_NAME)
        {
            return Err(ModpkgBuilderError::MissingBaseLayer);
        }

        // Check if all unique layers are defined
        for layer in unique_layers {
            // Skip validation for empty layer names (they represent chunks with no layer)
            if layer.is_empty() {
                continue;
            }
            if !defined_layers.iter().any(|l| l.name == layer.as_ref()) {
                return Err(ModpkgBuilderError::LayerNotFound(layer.to_string()));
            }
        }

        Ok(())
    }

    fn process_chunks<
        TWriter: io::Write + io::Seek,
        TChunkDataProvider: Fn(&ModpkgChunkBuilder, &mut Cursor<Vec<u8>>) -> Result<(), ModpkgBuilderError>,
    >(
        chunks: &[&ModpkgChunkBuilder],
        writer: &mut BufWriter<TWriter>,
        provide_chunk_data: TChunkDataProvider,
        chunk_path_indices: &HashMap<u64, u32>,
        layer_indices: &HashMap<u64, u32>,
        wad_indices: &HashMap<u64, u32>,
    ) -> Result<Vec<ModpkgChunk>, ModpkgBuilderError> {
        let mut final_chunks = Vec::new();
        for chunk_builder in chunks {
            let mut data_writer = Cursor::new(Vec::new());
            provide_chunk_data(chunk_builder, &mut data_writer)?;

            let uncompressed_data = data_writer.get_ref();
            let uncompressed_size = uncompressed_data.len();
            let uncompressed_checksum = xxh3_64(uncompressed_data);

            let (compressed_data, compression) =
                Self::compress_chunk_data(uncompressed_data, chunk_builder.compression)?;

            let compressed_size = compressed_data.len();
            let compressed_checksum = xxh3_64(&compressed_data);

            let data_offset = writer.stream_position()?;
            writer.write_all(&compressed_data)?;

            let path_hash = chunk_builder.path_hash;
            let layer_index = if chunk_builder.layer.is_empty() {
                NO_LAYER_INDEX
            } else {
                layer_indices
                    .get(&hash_layer_name(&chunk_builder.layer))
                    .copied()
                    .unwrap_or(NO_LAYER_INDEX)
            };
            let wad_index = if chunk_builder.wad.is_empty() {
                NO_WAD_INDEX
            } else {
                wad_indices
                    .get(&hash_wad_name(&chunk_builder.wad))
                    .copied()
                    .unwrap_or(NO_WAD_INDEX)
            };

            let chunk = ModpkgChunk {
                path_hash,
                data_offset,
                compression,
                compressed_size: compressed_size as u64,
                uncompressed_size: uncompressed_size as u64,
                compressed_checksum,
                uncompressed_checksum,
                path_index: *chunk_path_indices.get(&path_hash).unwrap_or(&0),
                layer_index,
                wad_index,
            };

            final_chunks.push(chunk);
        }

        Ok(final_chunks)
    }
}

impl ModpkgChunkBuilder {
    const DEFAULT_LAYER: &'static str = "base";

    /// Create a new chunk builder with the default layer.
    pub fn new() -> Self {
        Self {
            path_hash: 0,
            path: String::new(),
            compression: ModpkgCompression::None,
            layer: Self::DEFAULT_LAYER.to_string(),
            wad: String::new(),
        }
    }

    /// Set the path of the chunk (input path is case insensitive).
    ///
    /// This will always hash the provided path string using `hash_chunk_name`.
    pub fn with_path(mut self, path: &str) -> Result<Self, ModpkgBuilderError> {
        let path = path.to_lowercase();
        self.path_hash = hash_chunk_name(&path);
        self.path = path;
        Ok(self)
    }

    /// Set the path hash from a hex-encoded chunk name that represents the actual path hash.
    ///
    /// The input must have a base filename of exactly 16 hexadecimal characters. Any number of
    /// extensions after the base is allowed (only the base is parsed). The `0x` prefix is NOT
    /// allowed.
    /// The builder stores the provided (lowercased) string as the display path and parses the
    /// base as hexadecimal for the `path_hash`.
    pub fn with_hashed_chunk_name(mut self, hashed_name: &str) -> Result<Self, ModpkgBuilderError> {
        let provided = hashed_name.to_lowercase();
        let display_path = provided.clone();

        // Extract the hex part for hash parsing - find the base name before any extensions
        let path = Path::new(&display_path);
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&display_path);

        let hex_part = filename.split('.').next().unwrap_or(filename);

        if !utils::is_hex_chunk_name(hex_part) {
            return Err(ModpkgBuilderError::InvalidChunkName(provided));
        }

        self.path_hash = u64::from_str_radix(hex_part, 16)
            .map_err(|_| ModpkgBuilderError::InvalidChunkName(provided.clone()))?;
        self.path = display_path;

        Ok(self)
    }

    pub fn with_compression(mut self, compression: ModpkgCompression) -> Self {
        self.compression = compression;
        self
    }

    pub fn with_layer(mut self, layer: &str) -> Self {
        self.layer = layer.to_string();
        self
    }

    pub fn path_hash(&self) -> u64 {
        self.path_hash
    }

    pub fn layer(&self) -> &str {
        &self.layer
    }

    /// Compute the key for this chunk: (path_hash, layer_hash)
    /// This mirrors how chunks are keyed in the final Modpkg struct
    pub fn key(&self) -> (u64, u64) {
        let layer_hash = if self.layer.is_empty() {
            NO_LAYER_HASH
        } else {
            hash_layer_name(&self.layer)
        };
        (self.path_hash, layer_hash)
    }
}

impl ModpkgBuilder {
    /// Set the metadata for the builder.
    pub fn with_metadata(mut self, metadata: ModpkgMetadata) -> Result<Self, ModpkgBuilderError> {
        self.metadata = metadata;

        self.meta_chunks.insert(
            (hash_chunk_name(METADATA_CHUNK_PATH), NO_LAYER_HASH),
            ModpkgChunkBuilder::new()
                .with_path(METADATA_CHUNK_PATH)?
                .with_compression(ModpkgCompression::None)
                .with_layer(""),
        );

        Ok(self)
    }

    /// Set the readme for the builder.
    pub fn with_readme(mut self, readme: &str) -> Result<Self, ModpkgBuilderError> {
        self.readme = Some(readme.to_string());
        let readme_chunk = ModpkgChunkBuilder::new()
            .with_path(README_CHUNK_PATH)?
            .with_compression(ModpkgCompression::None)
            .with_layer("");

        let key = readme_chunk.key();
        self.meta_chunks.insert(key, readme_chunk);

        Ok(self)
    }
}

impl ModpkgLayerBuilder {
    pub fn new(name: impl AsRef<str>) -> Self {
        Self {
            name: name.as_ref().to_string(),
            priority: 0,
        }
    }

    pub fn with_name(mut self, name: impl AsRef<str>) -> Self {
        self.name = name.as_ref().to_string();
        self
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn base() -> Self {
        Self {
            name: BASE_LAYER_NAME.to_string(),
            priority: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Modpkg, ModpkgLayer};

    use super::*;

    use std::io::Cursor;

    #[test]
    fn test_modpkg_builder() {
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let builder = ModpkgBuilder::default()
            .with_metadata(ModpkgMetadata::default())
            .unwrap()
            .with_layer(ModpkgLayerBuilder::new("base").with_priority(0))
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("test.png")
                    .unwrap()
                    .with_compression(ModpkgCompression::Zstd)
                    .with_layer("base"),
            );

        builder
            .build_to_writer(&mut cursor, |_path, cursor| {
                cursor.write_all(&[0xAA; 100])?;
                Ok(())
            })
            .expect("Failed to build Modpkg");

        // Reset cursor and verify the file was created
        cursor.set_position(0);

        let modpkg = Modpkg::mount_from_reader(&mut cursor).unwrap();

        // Now we have 2 chunks: metadata + test.png
        assert_eq!(modpkg.chunks.len(), 2);

        let chunk = modpkg
            .chunks
            .get(&(hash_chunk_name("test.png"), hash_layer_name("base")))
            .unwrap();

        assert_eq!(
            modpkg.chunk_paths.get(&hash_chunk_name("test.png")),
            Some(&"test.png".to_string())
        );

        assert_eq!(chunk.compression, ModpkgCompression::Zstd);
        assert_eq!(chunk.uncompressed_size, 100);
        assert_eq!(chunk.compressed_size, 17);
        assert_eq!(chunk.uncompressed_checksum, xxh3_64(&[0xAA; 100]));
        assert_eq!(chunk.path_index, 0);

        assert_eq!(modpkg.layers.len(), 1);
        assert_eq!(
            modpkg.layers.get(&hash_layer_name("base")),
            Some(&ModpkgLayer {
                name: "base".to_string(),
                priority: 0,
            })
        );
    }

    #[test]
    fn test_with_hashed_chunk_name() {
        // Test with an extension
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("abcdef1234567890.dds")
            .unwrap();
        assert_eq!(chunk.path_hash(), 0xabcdef1234567890);
        assert_eq!(chunk.path, "abcdef1234567890.dds");

        // Test with an extension (no 0x prefix)
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("fedcba9876543210.txt")
            .unwrap();
        assert_eq!(chunk.path_hash(), 0xfedcba9876543210);
        assert_eq!(chunk.path, "fedcba9876543210.txt");

        // Test with an extension
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("1234abc456def789.dds")
            .unwrap();
        assert_eq!(chunk.path_hash(), 0x1234abc456def789);
        assert_eq!(chunk.path, "1234abc456def789.dds");

        // Test without extension
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("789def0011223344")
            .unwrap();
        assert_eq!(chunk.path_hash(), 0x789def0011223344);
        assert_eq!(chunk.path, "789def0011223344");

        // Test invalid hex should fail
        assert!(ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("not_hex.bin")
            .is_err());

        // Multiple extensions are allowed as long as base is valid
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("abcdef1234567890.texture.dds")
            .unwrap();
        assert_eq!(chunk.path_hash(), 0xabcdef1234567890);

        // 0x prefix should fail
        assert!(ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("0xabcdef1234567890.dds")
            .is_err());
    }
}
