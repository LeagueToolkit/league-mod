//! WAD patching: applying mod overrides to game WAD files.
//!
//! The core function is [`build_patched_wad`], which takes an original game WAD file
//! and a set of override chunks, and produces a new WAD file containing all original
//! chunks plus the overrides.
//!
//! # Signature Preservation
//!
//! The output header carries the original WAD's signature and checksum **verbatim**.
//! Riot's RSA signature covers the original TOC, so it does not validate the patched
//! TOC - it is preserved as provenance: verifiers (e.g. `ltk_sig`'s `WadMod` records)
//! use it to prove which Riot-signed WAD the overlay was derived from.
//!
//! # Compression Strategy
//!
//! **Non-overridden chunks** are passed through as raw compressed bytes from the
//! original WAD - no decompression or recompression occurs. This is the fast path
//! for the vast majority of chunks.
//!
//! **Override chunks** are provided as uncompressed data. The builder auto-detects
//! each override's file type via [`LeagueFileKind::identify_from_bytes`] and applies
//! the ideal compression:
//!
//! - **Audio files** (Wwise Bank / Wwise Package): stored uncompressed (`None`).
//! - **Everything else**: compressed with Zstd at level 3.
//!
//! League validates a chunk shared across WADs by its **compressed** checksum, so
//! a chunk routed to several WADs must be given to each build as the same
//! uncompressed input — deterministic compression then yields identical copies.

use crate::error::{Error, Result};
use byteorder::{LE, WriteBytesExt};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_file::LeagueFileKind;
use ltk_wad::{FileExt as _, Wad, WadChunk, WadChunkCompression};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, SeekFrom, Write};
use xxhash_rust::xxh3::xxh3_64;

/// Size of a single v3.4 WAD TOC entry.
const TOC_ENTRY_SIZE: usize = 32;

/// Write buffer size for the output WAD
const WRITE_BUFFER_SIZE: usize = 1 << 20; // 1 MiB

/// Build statistics returned by [`build_patched_wad`].
#[derive(Debug, Clone)]
pub struct PatchedWadStats {
    /// Total number of chunks in the output WAD (original + overrides).
    pub chunks_written: usize,
    /// Number of chunks that were replaced by mod overrides.
    pub overrides_applied: usize,
    /// Number of new entries added (not present in the original WAD).
    pub new_entries_added: usize,
    /// Number of chunks passed through unchanged from the original WAD.
    pub chunks_passed_through: usize,
    /// Wall-clock time to build this WAD, in milliseconds.
    pub elapsed_ms: u128,
}

/// Build a patched WAD by overlaying mod chunks on top of an original game WAD.
///
/// The output WAD preserves the original chunk order and contains *all* chunks from
/// the source - those present in `override_hashes` get their data replaced, everything
/// else is passed through as raw bytes from the original. Override hashes that don't
/// exist in the source WAD are treated as **new entries** and inserted at the correct
/// sorted position in the TOC.
///
/// The original WAD's header signature and checksum are copied through verbatim
/// (see the [module docs](self) on signature preservation).
///
/// Parent directories for `dst_wad_path` are created automatically.
///
/// The WAD is written to a sibling `.tmp` file and renamed into place atomically once
/// complete.
///
/// # Arguments
///
/// * `src_wad_path` - Absolute path to the original game WAD file.
/// * `dst_wad_path` - Absolute path where the patched WAD will be written.
/// * `override_hashes` - Set of path hashes that have overrides available.
///   Used to plan the TOC layout (new entries, merge order) without requiring
///   the actual data upfront.
/// * `resolve_override` - Callback invoked once per override hash during the
///   write pass. Returns the override's **uncompressed** bytes; the builder
///   compresses them. This allows the caller to lazily load override data on
///   demand instead of holding everything in memory.
///
/// # Returns
///
/// [`PatchedWadStats`] with build metrics (chunk counts, timing).
pub fn build_patched_wad<B: AsRef<[u8]>>(
    src_wad_path: &Utf8Path,
    dst_wad_path: &Utf8Path,
    override_hashes: &HashSet<u64>,
    resolve_override: impl FnMut(u64) -> Result<B>,
) -> Result<PatchedWadStats> {
    if let Some(parent) = dst_wad_path.parent() {
        std::fs::create_dir_all(parent.as_std_path())
            .map_err(|source| Error::write(parent, source))?;
    }

    let tmp_path = Utf8PathBuf::from(format!("{dst_wad_path}.tmp"));
    match write_patched_wad(
        src_wad_path,
        &tmp_path,
        dst_wad_path,
        override_hashes,
        resolve_override,
    ) {
        Ok(stats) => {
            std::fs::rename(tmp_path.as_std_path(), dst_wad_path.as_std_path())
                .map_err(|source| Error::write(dst_wad_path, source))?;
            Ok(stats)
        }
        Err(e) => {
            let _ = std::fs::remove_file(tmp_path.as_std_path());
            Err(e)
        }
    }
}

/// Write the patched WAD to `out_path` (the temp file). `dst_wad_path` is only
/// used for logging so messages show the real destination.
fn write_patched_wad<B: AsRef<[u8]>>(
    src_wad_path: &Utf8Path,
    out_path: &Utf8Path,
    dst_wad_path: &Utf8Path,
    override_hashes: &HashSet<u64>,
    mut resolve_override: impl FnMut(u64) -> Result<B>,
) -> Result<PatchedWadStats> {
    let start = std::time::Instant::now();

    let file = File::open(src_wad_path.as_std_path())
        .map_err(|source| Error::read(src_wad_path, source))?;
    let mmap =
        unsafe { memmap2::Mmap::map(&file).map_err(|source| Error::read(src_wad_path, source))? };
    let wad = Wad::mount(Cursor::new(&mmap[..]))?;
    let chunks = wad.chunks();

    // Collect new entry hashes (in overrides but not in the original WAD)
    let mut new_hashes: Vec<u64> = override_hashes
        .iter()
        .filter(|&&h| !chunks.contains(h))
        .copied()
        .collect();
    new_hashes.sort();

    if !new_hashes.is_empty() {
        tracing::info!(
            "Adding {} new entry/entries to WAD (src={} dst={})",
            new_hashes.len(),
            src_wad_path,
            dst_wad_path
        );
    }

    // Build a merged sorted list of ALL hashes (original + new).
    // WAD TOC must be sorted by path_hash; binary_search insertion maintains this.
    let mut ordered: Vec<u64> = chunks.iter().map(|c| c.path_hash).collect();
    for hash in &new_hashes {
        let pos = ordered.binary_search(hash).unwrap_or_else(|i| i);
        ordered.insert(pos, *hash);
    }
    let new_entries_added = new_hashes.len();

    let mut overrides_applied = 0usize;

    // Only the open is attributed; the byteorder writes that follow go to the
    // same file, and wrapping each one would drown the logic.
    let mut writer = BufWriter::with_capacity(
        WRITE_BUFFER_SIZE,
        std::fs::File::create(out_path.as_std_path())
            .map_err(|source| Error::write(out_path, source))?,
    );

    // Write header
    writer.write_u16::<LE>(0x5752)?; // "RW" magic
    writer.write_u8(3)?; // major version
    writer.write_u8(4)?; // minor version

    // Carry the source WAD's signature (256 bytes) + checksum (8 bytes) through
    // verbatim. The signature covers the *original* TOC, not the patched one -
    // preserving it lets verifiers (ltk_sig's WadMod records) recover the
    // Riot-signed original TOC provenance from the overlay file.
    writer.write_all(wad.signature())?;
    writer.write_u64::<LE>(wad.checksum())?;

    // Write chunk count
    writer.write_u32::<LE>(ordered.len() as u32)?;

    // Write dummy TOC (TOC_ENTRY_SIZE bytes per chunk) - overwritten with real offsets later.
    let toc_offset = writer.stream_position()?;
    for _ in &ordered {
        writer.write_all(&[0u8; TOC_ENTRY_SIZE])?;
    }

    let mut data_offset: u64 = toc_offset + (ordered.len() as u64) * TOC_ENTRY_SIZE as u64;

    // Write chunk data and build final TOC entries.
    let mut final_chunks: Vec<WadChunk> = Vec::with_capacity(ordered.len());

    for &path_hash in &ordered {
        if data_offset > u32::MAX as u64 {
            return Err(Error::Other(format!(
                "Patched WAD exceeds the 4 GiB limit of the WAD v3.4 format \
                 (chunk {path_hash:016x} would start at offset {data_offset})"
            )));
        }

        let resolved = if override_hashes.contains(&path_hash) {
            overrides_applied += 1;
            Some(resolve_override(path_hash)?)
        } else {
            None
        };

        let prepared = match &resolved {
            Some(bytes) => prepare_uncompressed(path_hash, bytes.as_ref())?,
            None => {
                let orig = chunks
                    .get(path_hash)
                    .ok_or_else(|| Error::Other(format!("Missing base chunk {path_hash:016x}")))?;
                prepare_passthrough(orig, &mmap[..])?
            }
        };

        writer.write_all(&prepared.data)?;
        final_chunks.push(prepared.to_chunk(path_hash, data_offset));
        data_offset += prepared.data.len() as u64;
    }

    // Seek back and write final TOC
    writer.seek(SeekFrom::Start(toc_offset))?;
    for chunk in &final_chunks {
        chunk.write_v3_4(&mut writer)?;
    }

    writer.flush()?;

    let elapsed_ms = start.elapsed().as_millis();
    let chunks_passed_through = ordered.len() - overrides_applied;

    tracing::info!(
        "Patched WAD complete dst={} chunks={} overrides={} new={} passed_through={} elapsed_ms={}",
        dst_wad_path,
        ordered.len(),
        overrides_applied,
        new_entries_added,
        chunks_passed_through,
        elapsed_ms
    );

    Ok(PatchedWadStats {
        chunks_written: ordered.len(),
        overrides_applied,
        new_entries_added,
        chunks_passed_through,
        elapsed_ms,
    })
}

/// A chunk's compressed bytes plus the TOC metadata to record for it, ready to be
/// written at the current data offset. Produced by the `prepare_*` helpers; the
/// only field the caller fills in is `data_offset`, via [`Self::to_chunk`].
struct PreparedChunk<'a> {
    /// Compressed bytes to write verbatim into the output WAD.
    data: Cow<'a, [u8]>,
    uncompressed_size: usize,
    compression_type: WadChunkCompression,
    frame_count: u8,
    start_frame: u32,
    checksum: u64,
}

impl PreparedChunk<'_> {
    fn to_chunk(&self, path_hash: u64, data_offset: u64) -> WadChunk {
        WadChunk {
            path_hash,
            data_offset: data_offset as usize,
            compressed_size: self.data.len(),
            uncompressed_size: self.uncompressed_size,
            compression_type: self.compression_type,
            is_duplicated: false,
            frame_count: self.frame_count,
            start_frame: self.start_frame,
            checksum: self.checksum,
        }
    }
}

/// Reject chunk sizes that overflow the WAD v3.4 format's u32 size fields.
fn ensure_chunk_fits(
    path_hash: u64,
    compressed: usize,
    uncompressed: usize,
    kind: &str,
) -> Result<()> {
    if compressed > u32::MAX as usize || uncompressed > u32::MAX as usize {
        return Err(Error::Other(format!(
            "{kind} chunk {path_hash:016x} is too large for the WAD v3.4 format \
             (compressed {compressed} / uncompressed {uncompressed} bytes)"
        )));
    }
    Ok(())
}

/// An uncompressed override: identify the file type, apply the ideal compression,
/// and checksum our *own* compressed output.
fn prepare_uncompressed(path_hash: u64, data: &[u8]) -> Result<PreparedChunk<'static>> {
    let compression = LeagueFileKind::identify_from_bytes(data).ideal_compression();
    let compressed = compress_by_type(data, compression)?;
    ensure_chunk_fits(path_hash, compressed.len(), data.len(), "Override")?;
    let checksum = xxh3_64(&compressed);
    Ok(PreparedChunk {
        data: Cow::Owned(compressed),
        uncompressed_size: data.len(),
        compression_type: compression,
        frame_count: 0,
        start_frame: 0,
        checksum,
    })
}

/// A non-overridden chunk: copy its raw compressed bytes straight from the source
/// WAD's mmap, no decompression or recompression.
fn prepare_passthrough<'a>(orig: &WadChunk, mmap: &'a [u8]) -> Result<PreparedChunk<'a>> {
    let end = orig.data_offset + orig.compressed_size;
    let data = mmap.get(orig.data_offset..end).ok_or_else(|| {
        Error::Other(format!(
            "Base chunk {:016x} data out of bounds",
            orig.path_hash
        ))
    })?;
    Ok(PreparedChunk {
        data: Cow::Borrowed(data),
        uncompressed_size: orig.uncompressed_size,
        compression_type: orig.compression_type,
        frame_count: orig.frame_count,
        start_frame: orig.start_frame,
        checksum: orig.checksum,
    })
}

/// Compress data using the specified compression type.
fn compress_by_type(data: &[u8], compression: WadChunkCompression) -> Result<Vec<u8>> {
    match compression {
        WadChunkCompression::None => Ok(data.to_vec()),
        WadChunkCompression::Zstd => {
            let mut out = Vec::new();
            let mut encoder = zstd::Encoder::new(BufWriter::new(&mut out), 3)?;
            encoder.write_all(data)?;
            encoder.finish()?;
            Ok(out)
        }
        other => Err(Error::Other(format!(
            "Unsupported compression type for writing: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_by_type_none() {
        let data = b"Hello, world!";
        let result = compress_by_type(data, WadChunkCompression::None).unwrap();
        assert_eq!(result, data);
    }

    #[test]
    fn test_compress_by_type_zstd() {
        let data = b"Hello, world!".repeat(100);
        let compressed = compress_by_type(&data, WadChunkCompression::Zstd).unwrap();
        assert!(compressed.len() < data.len());
    }
}
