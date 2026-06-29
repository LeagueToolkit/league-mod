//! WAD patching: applying mod overrides to game WAD files.
//!
//! The core function is [`build_patched_wad`], which takes an original game WAD file
//! and a set of override chunks, and produces a new WAD file containing all original
//! chunks plus the overrides.
//!
//! # Compression Strategy
//!
//! **Non-overridden chunks** are passed through as raw compressed bytes from the
//! original WAD — no decompression or recompression occurs. This is the fast path
//! for the vast majority of chunks.
//!
//! **Override chunks** are provided as uncompressed data. The builder auto-detects
//! each override's file type via [`LeagueFileKind::identify_from_bytes`] and applies
//! the ideal compression:
//!
//! - **Audio files** (Wwise Bank / Wwise Package): stored uncompressed (`None`).
//! - **Everything else**: compressed with Zstd at level 3.

use crate::error::{Error, Result};
use byteorder::{WriteBytesExt, LE};
use camino::Utf8Path;
use ltk_file::LeagueFileKind;
use ltk_wad::{FileExt as _, Wad, WadChunk, WadChunkCompression};
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
/// the source — those present in `override_hashes` get their data replaced, everything
/// else is passed through as raw bytes from the original. Override hashes that don't
/// exist in the source WAD are treated as **new entries** and inserted at the correct
/// sorted position in the TOC.
///
/// Parent directories for `dst_wad_path` are created automatically.
///
/// # Arguments
///
/// * `src_wad_path` — Absolute path to the original game WAD file.
/// * `dst_wad_path` — Absolute path where the patched WAD will be written.
/// * `override_hashes` — Set of path hashes that have overrides available.
///   Used to plan the TOC layout (new entries, merge order) without requiring
///   the actual data upfront.
/// * `resolve_override` — Callback invoked once per override hash during the
///   write pass. Must return the **uncompressed** file data for the given hash.
///   This allows the caller to lazily load override data on demand instead of
///   holding everything in memory.
///
/// # Returns
///
/// [`PatchedWadStats`] with build metrics (chunk counts, timing).
pub fn build_patched_wad<B: AsRef<[u8]>>(
    src_wad_path: &Utf8Path,
    dst_wad_path: &Utf8Path,
    override_hashes: &HashSet<u64>,
    mut resolve_override: impl FnMut(u64) -> Result<B>,
) -> Result<PatchedWadStats> {
    let start = std::time::Instant::now();

    let file = File::open(src_wad_path.as_std_path())?;
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
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

    if let Some(parent) = dst_wad_path.parent() {
        std::fs::create_dir_all(parent.as_std_path())?;
    }

    let mut writer = BufWriter::with_capacity(
        WRITE_BUFFER_SIZE,
        std::fs::File::create(dst_wad_path.as_std_path())?,
    );

    // Write header
    writer.write_u16::<LE>(0x5752)?; // "RW" magic
    writer.write_u8(3)?; // major version
    writer.write_u8(4)?; // minor version

    // Write dummy ECDSA signature (256 bytes) + checksum (8 bytes)
    writer.write_all(&[0u8; 256])?;
    writer.write_u64::<LE>(0)?;

    // Write chunk count
    writer.write_u32::<LE>(ordered.len() as u32)?;

    // Write dummy TOC (TOC_ENTRY_SIZE bytes per chunk) — overwritten with real offsets later.
    let toc_offset = writer.stream_position()?;
    for _ in &ordered {
        writer.write_all(&[0u8; TOC_ENTRY_SIZE])?;
    }

    let mut data_offset: u64 = toc_offset + (ordered.len() as u64) * TOC_ENTRY_SIZE as u64;

    // Write chunk data and build final TOC entries
    let mut final_chunks: Vec<WadChunk> = Vec::with_capacity(ordered.len());

    for &path_hash in &ordered {
        if data_offset > u32::MAX as u64 {
            return Err(Error::Other(format!(
                "Patched WAD exceeds the 4 GiB limit of the WAD v3.4 format \
                 (chunk {:016x} would start at offset {})",
                path_hash, data_offset
            )));
        }

        let bytes_written = if override_hashes.contains(&path_hash) {
            let override_bytes = resolve_override(path_hash)?;
            let override_data = override_bytes.as_ref();
            overrides_applied += 1;

            let kind = LeagueFileKind::identify_from_bytes(override_data);
            let compression = kind.ideal_compression();
            let compressed = compress_by_type(override_data, compression)?;

            if compressed.len() > u32::MAX as usize || override_data.len() > u32::MAX as usize {
                return Err(Error::Other(format!(
                    "Override chunk {:016x} is too large for the WAD v3.4 format \
                     (compressed {} / uncompressed {} bytes)",
                    path_hash,
                    compressed.len(),
                    override_data.len()
                )));
            }

            let compressed_checksum = xxh3_64(&compressed);

            writer.write_all(&compressed)?;

            final_chunks.push(WadChunk {
                path_hash,
                data_offset: data_offset as usize,
                compressed_size: compressed.len(),
                uncompressed_size: override_data.len(),
                compression_type: compression,
                is_duplicated: false,
                frame_count: 0,
                start_frame: 0,
                checksum: compressed_checksum,
            });

            compressed.len()
        } else {
            // Pass-through: copy the raw compressed bytes straight from the source.
            let orig = chunks
                .get(path_hash)
                .ok_or_else(|| Error::Other(format!("Missing base chunk {:016x}", path_hash)))?;
            let end = orig.data_offset + orig.compressed_size;
            let raw = mmap.get(orig.data_offset..end).ok_or_else(|| {
                Error::Other(format!("Base chunk {:016x} data out of bounds", path_hash))
            })?;
            writer.write_all(raw)?;

            final_chunks.push(WadChunk {
                path_hash,
                data_offset: data_offset as usize,
                compressed_size: orig.compressed_size,
                uncompressed_size: orig.uncompressed_size,
                compression_type: orig.compression_type,
                is_duplicated: false,
                frame_count: orig.frame_count,
                start_frame: orig.start_frame,
                checksum: orig.checksum,
            });

            raw.len()
        };

        data_offset += bytes_written as u64;
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
