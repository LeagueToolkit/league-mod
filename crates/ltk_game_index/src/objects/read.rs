//! Reading one chunk for the objects it declares.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};

use ltk_file::{LeagueFileKind, MAX_MAGIC_SIZE};
use ltk_hash::BinHash;
use ltk_meta::BinOverride;
use ltk_meta::stream::BinStream;
use ltk_wad::{ChunkDecoder, Wad, WadChunk, WadError};

/// The magic a `PTCH` opens with. The streaming reader refuses it.
const PATCH_MAGIC: [u8; 4] = *b"PTCH";

/// Raw bytes a sniff reads from a chunk first.
///
/// The first block of nearly every chunk fits. One whose block does not gets a second read of
/// [`HEAD_MAX_RAW`].
const HEAD_FIRST_RAW: usize = 16 * 1024;

/// Most raw bytes a sniff reads from one chunk.
///
/// A zstd block decodes to at most 128 KiB and an incompressible block is no larger. The
/// bound holds the first block and its headers.
const HEAD_MAX_RAW: usize = 256 * 1024;

/// A mounted archive, read through a file handle.
pub(super) type MountedArchive = Wad<BufReader<File>>;

/// Visits `(object, class)` for every object one bin declares.
///
/// A `PROP` is swept through its object table without decoding values. A `PTCH` is read
/// whole. Its patch records declare nothing.
///
/// # Errors
///
/// Fails when `reader` cannot be read or is not a bin. Objects before the failure were
/// visited.
pub fn for_each_declaration(
    mut reader: impl Read + Seek,
    mut visit: impl FnMut(BinHash, BinHash),
) -> Result<(), ltk_meta::Error> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    reader.seek(SeekFrom::Start(0))?;

    if magic == PATCH_MAGIC {
        let patch = BinOverride::from_reader(&mut reader)?;
        for object in patch.objects.values() {
            visit(object.path_hash, object.class_hash);
        }
        return Ok(());
    }

    let mut stream: BinStream<_> = BinStream::mount(reader)?;
    for entry in stream.entries() {
        let entry = entry?;
        visit(entry.path_hash, entry.class_hash);
    }
    Ok(())
}

/// Whether the first bytes of `chunk` are a bin's magic.
///
/// The head alone is decoded. A chunk that does not decode that far is not a bin the build
/// could read either.
pub(super) fn sniffs_as_bin(
    wad: &mut MountedArchive,
    chunk: &WadChunk,
    decoder: &mut ChunkDecoder,
) -> Result<bool, WadError> {
    let head = chunk_head(wad, chunk, decoder, MAX_MAGIC_SIZE)?;
    Ok(matches!(
        LeagueFileKind::identify_from_bytes(&head),
        LeagueFileKind::PropertyBin | LeagueFileKind::PropertyBinOverride
    ))
}

/// At most `want` bytes from the start of `chunk`, decompressing no further.
///
/// A chunk holding fewer than `want` bytes answers with what it holds.
fn chunk_head(
    wad: &mut MountedArchive,
    chunk: &WadChunk,
    decoder: &mut ChunkDecoder,
    want: usize,
) -> Result<Vec<u8>, WadError> {
    let want = want.min(chunk.uncompressed_size);
    let ceiling = HEAD_MAX_RAW.max(want);
    let mut raw_limit = HEAD_FIRST_RAW.max(want);
    loop {
        let raw = wad.load_chunk_raw_prefix(chunk, raw_limit)?;
        let cut_short = raw.len() == raw_limit && raw_limit < ceiling;
        match decoder.decompress_chunk_prefix(&raw, chunk, wad.subchunk_toc(), want) {
            Ok(head) if head.len() >= want || !cut_short => return Ok(head),
            Err(e) if !cut_short => return Err(e),
            _ => raw_limit = ceiling,
        }
    }
}
