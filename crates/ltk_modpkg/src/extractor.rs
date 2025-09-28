use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};

use crate::{chunk::ModpkgChunk, error::ModpkgError, Modpkg};

/// Extractor for ModPkg archives.
///
/// This struct provides functionality to extract chunks from a ModPkg archive
/// to a specified directory, organized by layers.
pub struct ModpkgExtractor<'modpkg, TSource: Read + Seek> {
    modpkg: &'modpkg mut Modpkg<TSource>,
}

impl<'modpkg, TSource: Read + Seek> ModpkgExtractor<'modpkg, TSource> {
    /// Create a new extractor for the given ModPkg.
    pub fn new(modpkg: &'modpkg mut Modpkg<TSource>) -> Self {
        Self { modpkg }
    }

    /// Extract all chunks from the ModPkg to the specified output directory.
    ///
    /// Chunks will be organized by layer, with each layer having its own subdirectory.
    pub fn extract_all(&mut self, output_dir: impl AsRef<Path>) -> Result<(), ModpkgError> {
        let output_dir = output_dir.as_ref();

        // Create the output directory if it doesn't exist
        fs::create_dir_all(output_dir)?;

        // Group chunks by layer
        let mut chunks_by_layer: HashMap<u64, Vec<ModpkgChunk>> = HashMap::new();

        for (key, chunk) in &self.modpkg.chunks {
            let (_, layer_hash) = *key;
            chunks_by_layer.entry(layer_hash).or_default().push(*chunk);
        }

        // Extract chunks for each layer
        for (layer_hash, chunks) in chunks_by_layer {
            let layer_name = match self.modpkg.layers.get(&layer_hash) {
                Some(layer) => &layer.name,
                None => continue, // Skip if layer not found
            };

            let layer_dir = output_dir.join(layer_name);
            fs::create_dir_all(&layer_dir)?;

            for chunk in chunks {
                self.extract_chunk(&chunk, &layer_dir)?;
            }
        }

        Ok(())
    }

    /// Extract a specific chunk to the specified directory.
    pub fn extract_chunk(
        &mut self,
        chunk: &ModpkgChunk,
        output_dir: impl AsRef<Path>,
    ) -> Result<PathBuf, ModpkgError> {
        let output_dir = output_dir.as_ref();

        // Get the path for this chunk
        let path = match self.modpkg.chunk_paths.get(&chunk.path_hash) {
            Some(path) => path,
            None => return Err(ModpkgError::MissingChunk(chunk.path_hash)),
        };

        // Create the full output path
        let output_path = output_dir.join(path);

        // Create parent directories if they don't exist
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Load and decompress the chunk data
        let data = self.modpkg.decoder().load_chunk_decompressed(chunk)?;

        // Write the data to the output file
        let mut file = File::create(&output_path)?;
        file.write_all(&data)?;

        Ok(output_path)
    }

    /// Extract a specific chunk by its path and layer name.
    pub fn extract_chunk_by_path(
        &mut self,
        path: &str,
        layer: &str,
        output_dir: impl AsRef<Path>,
    ) -> Result<PathBuf, ModpkgError> {
        let chunk = *self.modpkg.get_chunk(path, Some(layer))?;
        self.extract_chunk(&chunk, output_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder},
        ModpkgCompression,
    };
    use std::io::{Cursor, Write};
    use tempfile::tempdir;

    #[test]
    fn test_extractor() {
        // Create a test modpkg in memory
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let test_data = [0xAA; 100];
        let path = "test.bin";
        let layer_name = "base";

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(path)
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

        // Create a temporary directory for extraction
        let temp_dir = tempdir().unwrap();
        let output_dir = temp_dir.path();

        // Create an extractor and extract all chunks
        let mut extractor = ModpkgExtractor::new(&mut modpkg);
        extractor.extract_all(output_dir).unwrap();

        // Verify the extracted file
        let extracted_file = output_dir.join(layer_name).join(path);
        assert!(extracted_file.exists());

        // Read the extracted file and verify its contents
        let extracted_data = fs::read(extracted_file).unwrap();
        assert_eq!(extracted_data, test_data);
    }

    #[test]
    fn test_extract_multiple_layers() {
        // Create a test modpkg with multiple layers
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let base_data = [0xAA; 100];
        let custom_data = [0xBB; 100];
        let path = "test.bin";
        let base_layer = "base";
        let custom_layer = "custom";

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_layer(ModpkgLayerBuilder::new(custom_layer).with_priority(1))
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(path)
                    .unwrap()
                    .with_compression(ModpkgCompression::None)
                    .with_layer(base_layer),
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(path)
                    .unwrap()
                    .with_compression(ModpkgCompression::None)
                    .with_layer(custom_layer),
            );

        builder
            .build_to_writer(&mut cursor, |chunk, cursor| {
                if chunk.layer == base_layer {
                    cursor.write_all(&base_data)?;
                } else {
                    cursor.write_all(&custom_data)?;
                }
                Ok(())
            })
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Create a temporary directory for extraction
        let temp_dir = tempdir().unwrap();
        let output_dir = temp_dir.path();

        // Create an extractor and extract all chunks
        let mut extractor = ModpkgExtractor::new(&mut modpkg);
        extractor.extract_all(output_dir).unwrap();

        // Verify the extracted files
        let base_file = output_dir.join(base_layer).join(path);
        let custom_file = output_dir.join(custom_layer).join(path);

        assert!(base_file.exists());
        assert!(custom_file.exists());

        // Read the extracted files and verify their contents
        let extracted_base_data = fs::read(base_file).unwrap();
        let extracted_custom_data = fs::read(custom_file).unwrap();

        assert_eq!(extracted_base_data, base_data);
        assert_eq!(extracted_custom_data, custom_data);
    }
}
