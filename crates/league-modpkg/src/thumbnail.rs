use std::io::{Read, Seek};

use crate::{
    chunk::{NO_LAYER_INDEX, NO_WAD_INDEX},
    error::ModpkgError,
    Modpkg,
};

pub const THUMBNAIL_CHUNK_PATH: &str = "_meta_/thumbnail.webp";

impl<TSource: Read + Seek> Modpkg<TSource> {
    pub fn retrieve_thumbnail_data(&mut self) -> Result<Vec<u8>, ModpkgError> {
        let chunk = *self.get_chunk(THUMBNAIL_CHUNK_PATH, None)?;

        if chunk.layer_index != NO_LAYER_INDEX || chunk.wad_index != NO_WAD_INDEX {
            return Err(ModpkgError::InvalidThumbnailChunk);
        }

        let thumbnail_data = self.load_chunk_decompressed(&chunk)?;

        Ok(thumbnail_data.into_vec())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use crate::{
        builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder},
        chunk::{NO_LAYER_INDEX, NO_WAD_INDEX},
        error::ModpkgError,
        Modpkg, ModpkgCompression,
    };

    use super::*;

    #[test]
    fn test_retrieve_thumbnail_valid() {
        // Create a modpkg with a valid thumbnail (no layer, no wad)
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let thumbnail_data = b"fake webp data";

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(THUMBNAIL_CHUNK_PATH)
                    .unwrap()
                    .with_layer("") // Empty layer means no layer
                    .with_compression(ModpkgCompression::None),
            );

        builder
            .build_to_writer(&mut cursor, |chunk, writer| {
                if chunk.path == THUMBNAIL_CHUNK_PATH {
                    writer.write_all(thumbnail_data)?;
                } else {
                    writer.write_all(&[0xAA; 100])?;
                }
                Ok(())
            })
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Verify the thumbnail chunk has correct indices (no layer, no wad)
        let chunk = modpkg.get_chunk(THUMBNAIL_CHUNK_PATH, None).unwrap();
        assert_eq!(chunk.layer_index, NO_LAYER_INDEX);
        assert_eq!(chunk.wad_index, NO_WAD_INDEX);

        // Test thumbnail retrieval should succeed
        let result = modpkg.retrieve_thumbnail_data().unwrap();
        assert_eq!(&result, thumbnail_data);
    }

    #[test]
    fn test_retrieve_thumbnail_not_found_with_layer() {
        // Create a modpkg with a thumbnail that belongs to a layer (should not be found)
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let thumbnail_data = b"fake webp data";

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(THUMBNAIL_CHUNK_PATH)
                    .unwrap()
                    .with_layer("base") // This makes it invalid!
                    .with_compression(ModpkgCompression::None),
            );

        builder
            .build_to_writer(&mut cursor, |chunk, writer| {
                if chunk.path == THUMBNAIL_CHUNK_PATH {
                    writer.write_all(thumbnail_data)?;
                } else {
                    writer.write_all(&[0xAA; 100])?;
                }
                Ok(())
            })
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Verify the thumbnail chunk has a layer index (making it invalid)
        let chunk = modpkg
            .get_chunk(THUMBNAIL_CHUNK_PATH, Some("base"))
            .unwrap();
        assert_ne!(chunk.layer_index, NO_LAYER_INDEX);

        // Test thumbnail retrieval should fail because it won't find the chunk (it only looks for chunks with no layer)
        let result = modpkg.retrieve_thumbnail_data();
        assert!(matches!(result, Err(ModpkgError::MissingChunk(_))));
    }

    #[test]
    fn test_retrieve_thumbnail_invalid_chunk_indices() {
        // This test demonstrates the InvalidThumbnailChunk error by manually creating
        // a chunk with the correct path and layer hash but wrong indices
        use crate::{chunk::ModpkgChunk, chunk::NO_LAYER_HASH, hash_chunk_name};

        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);
        let thumbnail_data = b"fake webp data";

        // Create a basic modpkg first
        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("dummy.txt")
                    .unwrap()
                    .with_compression(ModpkgCompression::None),
            );

        builder
            .build_to_writer(&mut cursor, |_chunk, writer| {
                writer.write_all(&[0xAA; 100])?;
                Ok(())
            })
            .expect("Failed to build Modpkg");

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Manually insert a thumbnail chunk with invalid indices
        let path_hash = hash_chunk_name(THUMBNAIL_CHUNK_PATH);
        let invalid_chunk = ModpkgChunk {
            path_hash,
            data_offset: 0,
            compression: ModpkgCompression::None,
            compressed_size: thumbnail_data.len() as u64,
            uncompressed_size: thumbnail_data.len() as u64,
            compressed_checksum: 0,
            uncompressed_checksum: 0,
            path_index: 0,
            layer_index: 0, // Invalid! Should be NO_LAYER_INDEX (-1)
            wad_index: NO_WAD_INDEX,
        };

        modpkg
            .chunks
            .insert((path_hash, NO_LAYER_HASH), invalid_chunk);

        // Test should fail with InvalidThumbnailChunk
        let result = modpkg.retrieve_thumbnail_data();
        assert!(matches!(result, Err(ModpkgError::InvalidThumbnailChunk)));
    }
}
