use binrw::BinWrite;
use byteorder::{WriteBytesExt, LE};
use std::collections::HashMap;
use std::io::{self, BufWriter, Cursor, Seek, SeekFrom, Write};
use std::path::Path;
use xxhash_rust::xxh3::xxh3_64;

use crate::{chunk::ModpkgChunk, metadata::ModpkgMetadata, ModpkgCompression};
use crate::{hash_chunk_name, hash_layer_name, hash_wad_name, utils};

#[derive(Debug, thiserror::Error)]
pub enum ModpkgBuilderError {
    #[error("io error")]
    IoError(#[from] io::Error),

    #[error("binrw error")]
    BinWriteError(#[from] binrw::Error),

    #[error("unsupported compression type: {0:?}")]
    UnsupportedCompressionType(ModpkgCompression),

    #[error("missing base layer")]
    MissingBaseLayer,

    #[error("layer not found: {0}")]
    LayerNotFound(String),

    #[error("invalid chunk name: {0}")]
    InvalidChunkName(String),
}

#[derive(Debug, Clone, Default)]
pub struct ModpkgBuilder {
    metadata: ModpkgMetadata,
    chunks: Vec<ModpkgChunkBuilder>,
    layers: Vec<ModpkgLayerBuilder>,
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
    pub fn with_metadata(mut self, metadata: ModpkgMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_layer(mut self, layer: ModpkgLayerBuilder) -> Self {
        self.layers.push(layer);
        self
    }

    pub fn with_chunk(mut self, chunk: ModpkgChunkBuilder) -> Self {
        self.chunks.push(chunk);
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

        // Collect all unique paths and layers
        let (chunk_paths, chunk_path_indices) = Self::collect_unique_paths(&self.chunks);
        let (layers, _) = Self::collect_unique_layers(&self.chunks);
        let (wads, wad_indices) = Self::collect_unique_wads(&self.chunks);

        Self::validate_layers(&self.layers, &layers)?;

        // Write the magic header
        writer.write_all(b"_modpkg_")?;

        // Write version (1)
        writer.write_u32::<LE>(1)?;

        // Write placeholder for signature size and chunk count
        writer.write_u32::<LE>(0)?; // Placeholder for signature size
        writer.write_u32::<LE>(self.chunks.len() as u32)?;

        // Write signature (empty for now)
        let signature = Vec::new();
        writer.write_all(&signature)?;

        // Write layers
        writer.write_u32::<LE>(self.layers.len() as u32)?;
        for layer in &self.layers {
            writer.write_u32::<LE>(layer.name.len() as u32)?;
            writer.write_all(layer.name.as_bytes())?;
            writer.write_i32::<LE>(layer.priority)?;
        }

        // Write chunk paths
        writer.write_u32::<LE>(chunk_paths.len() as u32)?;
        for path in &chunk_paths {
            writer.write_all(path.as_bytes())?;
            writer.write_all(&[0])?; // Null terminator
        }

        // Write wads
        writer.write_u32::<LE>(wads.len() as u32)?;
        for wad in &wads {
            writer.write_all(wad.as_bytes())?;
            writer.write_all(&[0])?; // Null terminator
        }

        // Write metadata
        self.metadata.write(&mut writer)?;

        // Align to 8 bytes for chunks
        let current_pos = writer.stream_position()?;
        let padding = (8 - (current_pos % 8)) % 8;
        for _ in 0..padding {
            writer.write_all(&[0])?;
        }

        // Write dummy chunk data
        let chunk_toc_offset = writer.stream_position()?;
        writer.write_all(&vec![0; self.chunks.len() * ModpkgChunk::size_of()])?;

        // Process chunks
        // Build layer index map from defined layers order
        let mut layer_index_map = HashMap::new();
        for (idx, layer) in self.layers.iter().enumerate() {
            layer_index_map.insert(hash_layer_name(&layer.name), idx as u32);
        }

        let final_chunks = Self::process_chunks(
            &self.chunks,
            &mut writer,
            provide_chunk_data,
            &chunk_path_indices,
            &layer_index_map,
            &wad_indices,
        )?;

        // Write chunks
        writer.seek(SeekFrom::Start(chunk_toc_offset))?;
        for chunk in &final_chunks {
            chunk.write(&mut writer)?;
        }

        Ok(())
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

    fn collect_unique_layers(chunks: &[ModpkgChunkBuilder]) -> (Vec<String>, HashMap<u64, u32>) {
        let mut layers = Vec::new();
        let mut layer_indices = HashMap::new();
        for chunk in chunks {
            let hash = hash_layer_name(&chunk.layer);
            layer_indices.entry(hash).or_insert_with(|| {
                let index = layers.len();
                layers.push(chunk.layer.clone());
                index as u32
            });
        }

        (layers, layer_indices)
    }

    fn collect_unique_paths(chunks: &[ModpkgChunkBuilder]) -> (Vec<String>, HashMap<u64, u32>) {
        let mut paths = Vec::new();
        let mut path_indices = HashMap::new();

        for chunk in chunks {
            path_indices.entry(chunk.path_hash).or_insert_with(|| {
                let index = paths.len();
                paths.push(chunk.path.clone());
                index as u32
            });
        }

        (paths, path_indices)
    }

    fn collect_unique_wads(chunks: &[ModpkgChunkBuilder]) -> (Vec<String>, HashMap<u64, u32>) {
        let mut wads = Vec::new();
        let mut wad_indices = HashMap::new();
        for chunk in chunks {
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
        if !defined_layers.iter().any(|layer| layer.name == "base") {
            return Err(ModpkgBuilderError::MissingBaseLayer);
        }

        // Check if all unique layers are defined
        for layer in unique_layers {
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
        chunks: &[ModpkgChunkBuilder],
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
            let layer_idx = match layer_indices.get(&hash_layer_name(&chunk_builder.layer)) {
                Some(idx) => *idx,
                None => {
                    return Err(ModpkgBuilderError::LayerNotFound(
                        chunk_builder.layer.clone(),
                    ))
                }
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
                layer_index: layer_idx,
                wad_index: *wad_indices
                    .get(&hash_wad_name(&chunk_builder.wad))
                    .unwrap_or(&0),
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
    /// Accepts values with or without the `0x` prefix. The builder will store the provided
    /// string (lowercased and without `0x` prefix) as the chunk's display path while using
    /// the parsed numeric value as the `path_hash`.
    pub fn with_hashed_chunk_name(mut self, hashed_name: &str) -> Result<Self, ModpkgBuilderError> {
        let provided = hashed_name.to_lowercase();

        // Store the path without 0x prefix
        let display_path = utils::sanitize_chunk_name(&provided).to_string();

        // Extract the hex part for hash parsing - find the base name before any extensions
        let path = Path::new(&display_path);
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&display_path);

        // Split on the first dot to get the base name without any extensions
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
            name: "base".to_string(),
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

        assert_eq!(modpkg.chunks.len(), 1);

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
        // Test with 0x prefix and extension
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("0xabcdef123456.bin.dds")
            .unwrap();
        assert_eq!(chunk.path_hash(), 0xabcdef123456);
        assert_eq!(chunk.path, "abcdef123456.bin.dds");

        // Test without 0x prefix but with extension
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("fedcba987654.txt")
            .unwrap();
        assert_eq!(chunk.path_hash(), 0xfedcba987654);
        assert_eq!(chunk.path, "fedcba987654.txt");

        // Test with multiple extensions
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("123abc456def.texture.dds")
            .unwrap();
        assert_eq!(chunk.path_hash(), 0x123abc456def);
        assert_eq!(chunk.path, "123abc456def.texture.dds");

        // Test without extension
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("0x789def")
            .unwrap();
        assert_eq!(chunk.path_hash(), 0x789def);
        assert_eq!(chunk.path, "789def");

        // Test invalid hex should fail
        assert!(ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("not_hex.bin")
            .is_err());
    }
}
