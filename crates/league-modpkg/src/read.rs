use binrw::BinRead;
use byteorder::{ReadBytesExt, LE};
use std::{
    collections::HashMap,
    io::{BufReader, Read, Seek, SeekFrom},
};

use io_ext::ReaderExt;

use crate::{
    chunk::ModpkgChunk, error::ModpkgError, hash_chunk_name, hash_layer_name, hash_wad_name,
    Modpkg, ModpkgLayer, ModpkgMetadata,
};

impl<TSource: Read + Seek> Modpkg<TSource> {
    const MAGIC: [u8; 8] = *b"_modpkg_";

    pub fn mount_from_reader(mut source: TSource) -> Result<Self, ModpkgError> {
        let mut reader = BufReader::new(&mut source);

        let magic = reader.read_u64::<LE>()?;
        if magic != u64::from_le_bytes(Self::MAGIC) {
            return Err(ModpkgError::InvalidMagic(magic));
        }

        let version = reader.read_u32::<LE>()?;
        if version != 1 {
            return Err(ModpkgError::InvalidVersion(version));
        }

        let signature_size = reader.read_u32::<LE>()?;
        let chunk_count = reader.read_u32::<LE>()?;

        let mut signature = vec![0; signature_size as usize];
        reader.read_exact(&mut signature)?;

        let (layer_indices, layers) = read_layers(&mut reader)?;
        let (chunk_path_indices, chunk_paths) = read_chunk_paths(&mut reader)?;
        let (wads_indices, wads) = read_wads(&mut reader)?;

        let metadata = ModpkgMetadata::read(&mut reader)?;

        // Skip alignment
        let position = reader.stream_position()?;
        reader.seek(SeekFrom::Current(((8 - (position % 8)) % 8) as i64))?;

        let mut chunks = HashMap::new();
        for _ in 0..chunk_count {
            let chunk = ModpkgChunk::read(&mut reader)?;
            chunks.insert(
                (chunk.path_hash, layer_indices[chunk.layer_index as usize]),
                chunk,
            );
        }

        Ok(Self {
            signature,
            layer_indices,
            layers,
            chunk_path_indices,
            chunk_paths,
            wads_indices,
            wads,
            metadata,
            chunks,
            source,
        })
    }
}

fn read_layers<R: Read + Seek>(
    reader: &mut R,
) -> Result<(Vec<u64>, HashMap<u64, ModpkgLayer>), ModpkgError> {
    let layer_count = reader.read_u32::<LE>()?;
    let mut layer_indices = Vec::with_capacity(layer_count as usize);
    let mut layers = HashMap::with_capacity(layer_count as usize);
    for _ in 0..layer_count {
        let layer = ModpkgLayer::read(reader)?;
        let layer_hash = hash_layer_name(&layer.name);
        layers.insert(layer_hash, layer);
        layer_indices.push(layer_hash);
    }
    Ok((layer_indices, layers))
}

fn read_chunk_paths<R: Read + Seek>(
    reader: &mut R,
) -> Result<(Vec<u64>, HashMap<u64, String>), ModpkgError> {
    let chunk_paths_count = reader.read_u32::<LE>()?;
    let mut chunk_path_indices = Vec::with_capacity(chunk_paths_count as usize);
    let mut chunk_paths = HashMap::with_capacity(chunk_paths_count as usize);
    for _ in 0..chunk_paths_count {
        let chunk_path = reader.read_str_until_nul()?;
        let chunk_path_hash = hash_chunk_name(&chunk_path);
        chunk_path_indices.push(chunk_path_hash);
        chunk_paths.insert(chunk_path_hash, chunk_path);
    }
    Ok((chunk_path_indices, chunk_paths))
}

fn read_wads<R: Read + Seek>(
    reader: &mut R,
) -> Result<(Vec<u64>, HashMap<u64, String>), ModpkgError> {
    let wads_count = reader.read_u32::<LE>()?;
    let mut wads_indices = Vec::with_capacity(wads_count as usize);
    let mut wads = HashMap::with_capacity(wads_count as usize);
    for _ in 0..wads_count {
        let wad = reader.read_str_until_nul()?;
        let wad_hash = hash_wad_name(&wad);
        wads.insert(wad_hash, wad);
        wads_indices.push(wad_hash);
    }
    Ok((wads_indices, wads))
}
