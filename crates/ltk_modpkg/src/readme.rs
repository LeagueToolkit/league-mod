use std::io::{Read, Seek};

use crate::{error::ModpkgError, Modpkg};

/// The path to the README.md chunk.
pub const README_CHUNK_PATH: &str = "_meta_/readme.md";

impl<TSource: Read + Seek> Modpkg<TSource> {
    /// Load the README.md chunk from the mod package.
    pub fn load_readme(&mut self) -> Result<Vec<u8>, ModpkgError> {
        let chunk = *self.chunk(README_CHUNK_PATH, None)?;

        if chunk.layer().is_some() || chunk.wad().is_some() {
            return Err(ModpkgError::InvalidMetaChunk);
        }

        let data = self.decoder().load_chunk_decompressed(&chunk)?;

        Ok(data.into_vec())
    }
}
