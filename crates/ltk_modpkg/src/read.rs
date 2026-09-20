use binrw::BinRead;
use byteorder::{ReadBytesExt, LE};
use std::{
    collections::HashMap,
    io::{BufReader, Read, Seek, SeekFrom},
};

use ltk_io_ext::ReaderExt;

use crate::{
    chunk::ModpkgChunk, error::ModpkgError, ChunkKey, ChunkPath, LayerHash, LayerIndex, Modpkg,
    ModpkgLayer, PathHash, WadIndex, WadNameHash,
};

impl<TSource: Read + Seek> Modpkg<TSource> {
    const MAGIC: [u8; 8] = *b"_modpkg_";

    pub fn mount_from_reader(mut source: TSource) -> Result<Self, ModpkgError> {
        let mut reader = BufReader::with_capacity(64 * 1024, &mut source);

        let magic = reader.read_u64::<LE>()?;
        if magic != u64::from_le_bytes(Self::MAGIC) {
            return Err(ModpkgError::InvalidMagic(magic));
        }

        let version = reader.read_u32::<LE>()?;
        if version != 1 {
            return Err(ModpkgError::UnsupportedFormatVersion(version));
        }

        let signature_size = reader.read_u32::<LE>()?;
        let chunk_count = reader.read_u32::<LE>()?;

        let mut signature = vec![0; signature_size as usize];
        reader.read_exact(&mut signature)?;

        let (layer_indices, layers) = read_layers(&mut reader)?;
        let (chunk_path_indices, chunk_paths) = read_chunk_paths(&mut reader)?;
        let (wad_indices, wads) = read_wads(&mut reader)?;

        // Skip alignment
        let position = reader.stream_position()?;
        reader.seek(SeekFrom::Current(((8 - (position % 8)) % 8) as i64))?;

        let mut chunks = HashMap::new();
        let mut chunks_by_wad_layer: HashMap<(WadIndex, LayerIndex), Vec<ChunkKey>> =
            HashMap::new();
        for _ in 0..chunk_count {
            let chunk = ModpkgChunk::read(&mut reader)?;
            let layer_hash = if chunk.layer_index == LayerIndex::NONE {
                LayerHash::NONE
            } else {
                layer_indices[chunk.layer_index.value() as usize]
            };

            // A chunk that cannot be named cannot be extracted, so the table
            // position it names is checked here, once, rather than partway
            // through an unpack. `Modpkg::chunk_path` and everything planning
            // through it are infallible for this reason.
            if chunk.path_index as usize >= chunk_path_indices.len() {
                return Err(ModpkgError::MissingChunk(chunk.path_hash));
            }

            let key = ChunkKey::new(chunk.path_hash, layer_hash);
            chunks_by_wad_layer
                .entry((chunk.wad_index, chunk.layer_index))
                .or_default()
                .push(key);

            if let Some(existing) = chunks.insert(key, chunk) {
                if (existing.uncompressed_checksum, existing.uncompressed_size)
                    != (chunk.uncompressed_checksum, chunk.uncompressed_size)
                {
                    return Err(ModpkgError::ChunksInconsistent(chunk.path_hash));
                }
            }
        }

        drop(reader);

        Ok(Self {
            signature,
            layer_indices,
            layers,
            chunk_path_indices,
            chunk_paths,
            wad_indices,
            wads,
            chunks,
            chunks_by_wad_layer,
            source,
        })
    }
}

fn read_layers<R: Read + Seek>(
    reader: &mut R,
) -> Result<(Vec<LayerHash>, HashMap<LayerHash, ModpkgLayer>), ModpkgError> {
    let layer_count = reader.read_u32::<LE>()?;
    let mut layer_indices = Vec::with_capacity(layer_count as usize);
    let mut layers = HashMap::with_capacity(layer_count as usize);
    for _ in 0..layer_count {
        let layer = ModpkgLayer::read(reader)?;
        check_contained(&layer.name)?;
        let layer_hash = LayerHash::from_name(&layer.name);
        layers.insert(layer_hash, layer);
        layer_indices.push(layer_hash);
    }
    Ok((layer_indices, layers))
}

fn read_chunk_paths<R: Read + Seek>(
    reader: &mut R,
) -> Result<(Vec<PathHash>, HashMap<PathHash, String>), ModpkgError> {
    let chunk_paths_count = reader.read_u32::<LE>()?;
    let mut chunk_path_indices = Vec::with_capacity(chunk_paths_count as usize);
    let mut chunk_paths = HashMap::with_capacity(chunk_paths_count as usize);
    for _ in 0..chunk_paths_count {
        let chunk_path = ChunkPath::new(reader.read_str_until_nul()?);
        check_contained(chunk_path.as_str())?;
        let chunk_path_hash = chunk_path.hash();
        chunk_path_indices.push(chunk_path_hash);
        chunk_paths.insert(chunk_path_hash, chunk_path.into_string());
    }
    Ok((chunk_path_indices, chunk_paths))
}

fn read_wads<R: Read + Seek>(
    reader: &mut R,
) -> Result<(Vec<WadNameHash>, HashMap<WadNameHash, String>), ModpkgError> {
    let wads_count = reader.read_u32::<LE>()?;
    let mut wads_indices = Vec::with_capacity(wads_count as usize);
    let mut wads = HashMap::with_capacity(wads_count as usize);
    for _ in 0..wads_count {
        let wad = reader.read_str_until_nul()?;
        check_contained(&wad)?;
        let wad_hash = WadNameHash::from_name(&wad);
        wads.insert(wad_hash, wad);
        wads_indices.push(wad_hash);
    }
    Ok((wads_indices, wads))
}

/// Whether a name the package stores stays inside the directory it is
/// extracted to.
///
/// Chunk paths, layer names and WAD names all come out of the file and are all
/// joined onto a caller's output directory by
/// [`ModpkgExtractor`](crate::ModpkgExtractor), so a package naming `../../x`
/// or `/etc/x` would have it write outside the directory the caller asked for.
/// That is the "zip slip" class of bug, and the only defence is to refuse the
/// name.
///
/// Refused: a `..` component, a name rooted at `/` or `\`, and a name carrying
/// a `:`. Windows reads `\` as a separator and a drive-qualified name like
/// `C:x` relative to that drive rather than to the join, so both are treated as
/// escapes on every platform - a package one host refuses and another unpacks
/// would be worse than either answer alone. [`ChunkPath`] has already folded
/// `\` into `/` by the time a chunk path is checked; a layer or WAD name has
/// not.
///
/// A `.` component is kept: it goes nowhere.
fn is_contained(name: &str) -> bool {
    !name.starts_with(['/', '\\'])
        && !name.contains(':')
        && name.split(['/', '\\']).all(|component| component != "..")
}

/// Refuse `name` if extracting it would write outside the output directory.
fn check_contained(name: &str) -> Result<(), ModpkgError> {
    match is_contained(name) {
        true => Ok(()),
        false => Err(ModpkgError::EscapingPath(name.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder},
        ModpkgCompression,
    };
    use std::io::Cursor;

    /// Build a one-chunk package whose chunk carries `path` and `wad`.
    fn package_with(path: &str, wad: &str) -> Cursor<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());

        ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(path)
                    .with_wad(wad)
                    .with_compression(ModpkgCompression::None),
            )
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 4]))
            .expect("Failed to build Modpkg");

        cursor.set_position(0);
        cursor
    }

    /// The extractor joins a chunk's stored path onto the caller's directory,
    /// so a path that climbs out of it writes wherever it says.
    #[test]
    fn a_chunk_path_that_escapes_refuses_the_package() {
        let Err(error) = Modpkg::mount_from_reader(package_with("../../pwned.bin", "")) else {
            panic!("a package with an escaping chunk path mounted");
        };

        assert!(
            matches!(&error, ModpkgError::EscapingPath(path) if path == "../../pwned.bin"),
            "{error:?}"
        );
    }

    /// A chunk's WAD name is a directory component of where it lands, so it can
    /// escape the same way its path can.
    #[test]
    fn a_wad_name_that_escapes_refuses_the_package() {
        let Err(error) = Modpkg::mount_from_reader(package_with("data.bin", "../../pwned")) else {
            panic!("a package with an escaping WAD name mounted");
        };

        assert!(
            matches!(&error, ModpkgError::EscapingPath(name) if name == "../../pwned"),
            "{error:?}"
        );
    }

    /// The check has to refuse what would escape without refusing what merely
    /// looks like it would.
    #[test]
    fn containment_refuses_escapes_and_keeps_ordinary_names() {
        for name in [
            "../x",
            "a/../../x",
            r"a\..\..\x",
            "/etc/x",
            r"\etc\x",
            "C:/x",
        ] {
            assert!(!is_contained(name), "{name} was accepted");
        }

        for name in ["a/b/c.bin", "..bin", "a..b/c", "./a/b", "a/./b"] {
            assert!(is_contained(name), "{name} was refused");
        }
    }
}
