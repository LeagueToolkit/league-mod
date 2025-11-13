use std::io::{Read, Seek, SeekFrom};

use crate::{chunk::ModpkgChunk, error::ModpkgError, ModpkgCompression};

pub struct ModpkgDecoder<'modpkg, TSource: Read + Seek> {
    pub(crate) source: &'modpkg mut TSource,
}

impl<TSource> ModpkgDecoder<'_, TSource>
where
    TSource: Read + Seek,
{
    /// Load the raw compressed data of a chunk
    pub fn load_chunk_raw(&mut self, chunk: &ModpkgChunk) -> Result<Box<[u8]>, ModpkgError> {
        let mut data = vec![0; chunk.compressed_size as usize];

        self.source.seek(SeekFrom::Start(chunk.data_offset))?;
        self.source.read_exact(&mut data)?;

        Ok(data.into_boxed_slice())
    }

    /// Load and decompress the data of a chunk
    pub fn load_chunk_decompressed(
        &mut self,
        chunk: &ModpkgChunk,
    ) -> Result<Box<[u8]>, ModpkgError> {
        match chunk.compression {
            ModpkgCompression::None => self.load_chunk_raw(chunk),
            ModpkgCompression::Zstd => self.decode_zstd_chunk(chunk),
        }
    }

    fn decode_zstd_chunk(&mut self, chunk: &ModpkgChunk) -> Result<Box<[u8]>, ModpkgError> {
        self.source.seek(SeekFrom::Start(chunk.data_offset))?;

        let mut data: Vec<u8> = vec![0; chunk.uncompressed_size as usize];

        zstd::Decoder::new(&mut self.source)
            .map_err(|e| ModpkgError::Io(std::io::Error::other(e)))?
            .read_exact(&mut data)?;

        Ok(data.into_boxed_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder},
        Modpkg,
    };
    use std::io::{Cursor, Write};

    #[test]
    fn test_decoder() {
        // Create a test modpkg in memory
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let test_data = [0xAA; 100];

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("test.bin")
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

        // Get the test.bin chunk (not the metadata chunk)
        let test_path_hash = crate::hash_chunk_name("test.bin");
        let base_layer_hash = crate::hash_layer_name("base");
        let chunk = modpkg
            .chunks
            .get(&(test_path_hash, base_layer_hash))
            .expect("test.bin chunk not found");

        // Create a decoder and test it
        let mut decoder = ModpkgDecoder {
            source: &mut modpkg.source,
        };

        // Test raw loading
        let raw_data = decoder.load_chunk_raw(chunk).unwrap();
        assert_eq!(raw_data.len(), chunk.compressed_size as usize);

        // Test decompressed loading
        let decompressed_data = decoder.load_chunk_decompressed(chunk).unwrap();
        assert_eq!(decompressed_data.len(), chunk.uncompressed_size as usize);
        assert_eq!(&decompressed_data[..], &test_data[..]);
    }
}
