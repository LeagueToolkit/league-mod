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
use std::collections::HashMap;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::sync::Arc;
use xxhash_rust::xxh3::xxh3_64;

/// Build statistics returned by [`build_patched_wad`].
#[derive(Debug, Clone)]
pub struct PatchedWadStats {
    /// Total number of chunks in the output WAD (original + overrides).
    pub chunks_written: usize,
    /// Number of chunks that were replaced by mod overrides.
    pub overrides_applied: usize,
    /// Number of chunks passed through unchanged from the original WAD.
    pub chunks_passed_through: usize,
    /// Wall-clock time to build this WAD, in milliseconds.
    pub elapsed_ms: u128,
}

/// Build a patched WAD by overlaying mod chunks on top of an original game WAD.
///
/// The output WAD preserves the original chunk order and contains *all* chunks from
/// the source — those present in `overrides` get their data replaced, everything else
/// is passed through as raw bytes from the original. Override hashes that don't exist in the source
/// WAD are silently ignored (with a warning log).
///
/// Parent directories for `dst_wad_path` are created automatically.
///
/// # Arguments
///
/// * `src_wad_path` — Absolute path to the original game WAD file.
/// * `dst_wad_path` — Absolute path where the patched WAD will be written.
/// * `overrides` — Map of `path_hash -> uncompressed_file_data` to overlay.
///   Values are `Arc<[u8]>` to allow zero-copy sharing when the same override
///   is distributed to multiple WADs.
///
/// # Returns
///
/// [`PatchedWadStats`] with build metrics (chunk counts, timing).
pub fn build_patched_wad(
    src_wad_path: &Utf8Path,
    dst_wad_path: &Utf8Path,
    overrides: &HashMap<u64, Arc<[u8]>>,
) -> Result<PatchedWadStats> {
    let start = std::time::Instant::now();

    // Load original WAD
    let file = std::fs::File::open(src_wad_path.as_std_path())?;
    let mut wad = Wad::mount(file)?;
    let chunks = wad.chunks().clone();

    // Warn about unknown override hashes
    let unknown_override_hashes = overrides
        .keys()
        .filter(|&&h| !chunks.contains(h))
        .copied()
        .collect::<Vec<_>>();
    if !unknown_override_hashes.is_empty() {
        tracing::warn!(
            "Ignoring {} override chunk(s) not present in base WAD (src={} dst={})",
            unknown_override_hashes.len(),
            src_wad_path,
            dst_wad_path
        );
        tracing::debug!(
            "Unknown override hashes (first 16) = [{}]",
            unknown_override_hashes
                .iter()
                .take(16)
                .map(|h| format!("{:016x}", h))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // Collect chunk path hashes in original order (sorted by path_hash as WAD requires)
    let ordered: Vec<u64> = chunks.iter().map(|c| c.path_hash).collect();

    let mut overrides_applied = 0usize;

    if let Some(parent) = dst_wad_path.parent() {
        std::fs::create_dir_all(parent.as_std_path())?;
    }

    let mut writer = BufWriter::new(std::fs::File::create(dst_wad_path.as_std_path())?);

    // Write header
    writer.write_u16::<LE>(0x5752)?; // "RW" magic
    writer.write_u8(3)?; // major version
    writer.write_u8(4)?; // minor version

    // Write dummy ECDSA signature (256 bytes) + checksum (8 bytes)
    writer.write_all(&[0u8; 256])?;
    writer.write_u64::<LE>(0)?;

    // Write chunk count
    writer.write_u32::<LE>(ordered.len() as u32)?;

    // Write dummy TOC (32 bytes per chunk) — will be overwritten later
    let toc_offset = writer.stream_position()?;
    for _ in &ordered {
        writer.write_all(&[0u8; 32])?;
    }

    // Write chunk data and build final TOC entries
    let mut final_chunks: Vec<WadChunk> = Vec::with_capacity(ordered.len());

    for &path_hash in &ordered {
        let orig = chunks
            .get(path_hash)
            .ok_or_else(|| Error::Other(format!("Missing base chunk {:016x}", path_hash)))?;

        let data_offset = writer.stream_position()? as usize;

        if let Some(override_bytes) = overrides.get(&path_hash) {
            // Override path: detect file type, compress, and write
            overrides_applied += 1;

            let kind = LeagueFileKind::identify_from_bytes(override_bytes);
            let compression = kind.ideal_compression();
            let compressed = compress_by_type(override_bytes, compression)?;
            let compressed_checksum = xxh3_64(&compressed);

            writer.write_all(&compressed)?;

            final_chunks.push(WadChunk {
                path_hash,
                data_offset,
                compressed_size: compressed.len(),
                uncompressed_size: override_bytes.len(),
                compression_type: compression,
                is_duplicated: false,
                frame_count: 0,
                start_frame: 0,
                checksum: compressed_checksum,
            });
        } else {
            // Pass-through: read raw compressed bytes and copy them unchanged
            let raw = wad.load_chunk_raw(orig)?;
            writer.write_all(&raw)?;

            final_chunks.push(WadChunk {
                path_hash,
                data_offset,
                compressed_size: orig.compressed_size,
                uncompressed_size: orig.uncompressed_size,
                compression_type: orig.compression_type,
                is_duplicated: false,
                frame_count: orig.frame_count,
                start_frame: orig.start_frame,
                checksum: orig.checksum,
            });
        }
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
        "Patched WAD complete dst={} chunks={} overrides={} passed_through={} elapsed_ms={}",
        dst_wad_path,
        ordered.len(),
        overrides_applied,
        chunks_passed_through,
        elapsed_ms
    );

    Ok(PatchedWadStats {
        chunks_written: ordered.len(),
        overrides_applied,
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
