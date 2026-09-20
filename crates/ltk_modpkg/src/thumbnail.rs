use std::io::{Read, Seek};

use crate::{error::ModpkgError, Modpkg};

/// The path to the thumbnail chunk.
pub const THUMBNAIL_CHUNK_PATH: &str = "_meta_/thumbnail.webp";

impl<TSource: Read + Seek> Modpkg<TSource> {
    /// Load the thumbnail chunk from the mod package.
    pub fn load_thumbnail(&mut self) -> Result<Vec<u8>, ModpkgError> {
        let chunk = *self.chunk(THUMBNAIL_CHUNK_PATH, None)?;

        if chunk.layer().is_some() || chunk.wad().is_some() {
            return Err(ModpkgError::InvalidMetaChunk);
        }

        let thumbnail_data = self.decoder().load_chunk_decompressed(&chunk)?;

        Ok(thumbnail_data.into_vec())
    }
}
