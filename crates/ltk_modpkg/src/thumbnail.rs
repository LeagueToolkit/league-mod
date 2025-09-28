use std::io::{Read, Seek};

use crate::{
    chunk::{NO_LAYER_INDEX, NO_WAD_INDEX},
    error::ModpkgError,
    Modpkg,
};

/// The path of the README.md chunk.
pub const README_CHUNK_PATH: &str = "_meta_/readme.md";

impl<TSource: Read + Seek> Modpkg<TSource> {
    pub fn retrieve_readme_data(&mut self) -> Result<Vec<u8>, ModpkgError> {
        let chunk = *self.get_chunk(README_CHUNK_PATH, None)?;

        if chunk.layer_index != NO_LAYER_INDEX || chunk.wad_index != NO_WAD_INDEX {
            return Err(ModpkgError::InvalidMetaChunk);
        }

        let data = self.load_chunk_decompressed(&chunk)?;

        Ok(data.into_vec())
    }
}
