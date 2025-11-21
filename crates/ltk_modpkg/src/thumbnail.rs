use std::io::{Read, Seek};

use crate::{
    chunk::{NO_LAYER_INDEX, NO_WAD_INDEX},
    error::ModpkgError,
    Modpkg,
};

/// The path to the thumbnail chunk.
pub const THUMBNAIL_CHUNK_PATH: &str = "_meta_/thumbnail.webp";

impl<TSource: Read + Seek> Modpkg<TSource> {
    /// Load the thumbnail chunk from the mod package.
    pub fn load_thumbnail(&mut self) -> Result<Vec<u8>, ModpkgError> {
        let chunk = *self.get_chunk(THUMBNAIL_CHUNK_PATH, None)?;

        if chunk.layer_index != NO_LAYER_INDEX || chunk.wad_index != NO_WAD_INDEX {
            return Err(ModpkgError::InvalidMetaChunk);
        }

        let thumbnail_data = self.load_chunk_decompressed(&chunk)?;

        Ok(thumbnail_data.into_vec())
    }
}
