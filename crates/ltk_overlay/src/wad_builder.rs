//! WAD patching: applying mod overrides to game WAD files.
//!
//! The core function is [`build_patched_wad`], which takes an original game WAD file
//! and a set of override chunks, and produces a new WAD file containing all original
//! chunks plus the overrides.
//!
//! # Compression Strategy
//!
//! Override data arrives uncompressed from content providers. The builder decides how
//! to compress each chunk:
//!
//! - **Audio files** (Wwise Bank / Wwise Package, detected by magic bytes): stored
//!   uncompressed for streaming performance.
//! - **ZstdMulti chunks**: If the original chunk uses League's `ZstdMulti` format
//!   (an uncompressed header prefix followed by zstd-compressed data), the builder
//!   preserves this structure by compressing only the payload portion.
//! - **Everything else**: compressed with Zstd at level 3.
//!
//! # Deduplication
//!
//! Chunks with identical compressed data (same xxHash3 checksum) are tracked to avoid
//! writing duplicate bytes. Statistics on deduplication savings are returned in
//! [`PatchedWadStats`].

use crate::error::{Error, Result};
use ltk_file::LeagueFileKind;
use ltk_wad::{Wad, WadBuilder, WadChunkBuilder, WadChunkCompression};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use xxhash_rust::xxh3::xxh3_64;

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// Build statistics returned by [`build_patched_wad`].
#[derive(Debug, Clone)]
pub struct PatchedWadStats {
    /// Total number of chunks in the output WAD (original + overrides).
    pub chunks_written: usize,
    /// Number of chunks that were replaced by mod overrides.
    pub overrides_applied: usize,
    /// Number of override chunks kept uncompressed (audio files).
    pub audio_uncompressed: usize,
    /// Number of chunks that shared data with another chunk (deduplicated).
    pub chunks_deduplicated: usize,
    /// Bytes saved by deduplication.
    pub bytes_saved_dedup: usize,
    /// Wall-clock time to build this WAD, in milliseconds.
    pub elapsed_ms: u128,
}

/// Build a patched WAD by overlaying mod chunks on top of an original game WAD.
///
/// The output WAD preserves the original chunk order and contains *all* chunks from
/// the source — those present in `overrides` get their data replaced, everything else
/// is copied verbatim. Override hashes that don't exist in the source WAD are silently
/// ignored (with a warning log).
///
/// Parent directories for `dst_wad_path` are created automatically.
///
/// # Arguments
///
/// * `src_wad_path` — Absolute path to the original game WAD file.
/// * `dst_wad_path` — Absolute path where the patched WAD will be written.
/// * `overrides` — Map of `path_hash -> uncompressed_file_data` to overlay.
///
/// # Returns
///
/// [`PatchedWadStats`] with build metrics (chunk counts, dedup savings, timing).
pub fn build_patched_wad(
    src_wad_path: &Path,
    dst_wad_path: &Path,
    overrides: &HashMap<u64, Vec<u8>>,
) -> Result<PatchedWadStats> {
    let start = std::time::Instant::now();

    // Load original WAD
    let file = std::fs::File::open(src_wad_path)?;
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
            src_wad_path.display(),
            dst_wad_path.display()
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

    let ordered: Vec<u64> = chunks.iter().map(|c| c.path_hash).collect();
    let mut builder = WadBuilder::default();
    let mut chunk_data_map: HashMap<u64, Vec<u8>> = HashMap::new(); // path_hash -> compressed data
    let mut dedup_tracker: HashMap<u64, u64> = HashMap::new(); // checksum -> first path_hash
    let mut audio_chunks_uncompressed = 0usize;
    let mut chunks_deduplicated = 0usize;
    let mut bytes_saved_dedup = 0usize;

    // Process each chunk and prepare data
    for &path_hash in &ordered {
        let orig = chunks
            .get(path_hash)
            .ok_or_else(|| Error::Other(format!("Missing base chunk {:016x}", path_hash)))?;

        // Determine chunk data (either from override or original)
        let (chunk_data, _uncompressed_size, compression_type) =
            if let Some(bytes) = overrides.get(&path_hash) {
                // Optimization 1: Audio detection - keep audio uncompressed
                let is_audio = !should_compress(bytes);

                if is_audio {
                    audio_chunks_uncompressed += 1;
                    (bytes.clone(), bytes.len(), WadChunkCompression::None)
                } else if orig.compression_type == WadChunkCompression::ZstdMulti {
                    // Preserve ZstdMulti structure (uncompressed prefix + zstd data)
                    let raw = wad.load_chunk_raw(orig)?.to_vec();
                    let prefix_len = find_zstd_magic_offset(&raw).unwrap_or(0);

                    if prefix_len > 0 && bytes.len() >= prefix_len {
                        let mut combined = Vec::with_capacity(prefix_len + bytes.len());
                        combined.extend_from_slice(&bytes[..prefix_len]);
                        let rest = compress_zstd(&bytes[prefix_len..])?;
                        combined.extend_from_slice(&rest);
                        (combined, bytes.len(), WadChunkCompression::ZstdMulti)
                    } else {
                        let compressed = compress_zstd(bytes)?;
                        (compressed, bytes.len(), WadChunkCompression::Zstd)
                    }
                } else {
                    // Everything else: Zstd compression
                    let compressed = compress_zstd(bytes)?;
                    (compressed, bytes.len(), WadChunkCompression::Zstd)
                }
            } else {
                // No override: keep original chunk data
                let raw = wad.load_chunk_raw(orig)?.to_vec();
                (raw, orig.uncompressed_size, orig.compression_type)
            };

        // Optimization 2: Deduplication - check if this data was already seen
        let checksum = xxh3_64(&chunk_data);
        if let Some(&_existing_path_hash) = dedup_tracker.get(&checksum) {
            // This data is a duplicate, track it
            chunks_deduplicated += 1;
            bytes_saved_dedup += chunk_data.len();
            // Don't store duplicate data, WadBuilder will handle sharing
        } else {
            // First time seeing this data
            dedup_tracker.insert(checksum, path_hash);
            chunk_data_map.insert(path_hash, chunk_data.clone());
        }

        // Add chunk to builder using WadChunkBuilder
        // Note: WadBuilder doesn't support ZstdMulti, so we map it to Zstd
        // (we've already prepared the data correctly above)
        let builder_compression = match compression_type {
            WadChunkCompression::ZstdMulti => WadChunkCompression::Zstd,
            other => other,
        };

        let chunk_builder = WadChunkBuilder::default()
            .with_path(format!("{:016x}", path_hash)) // Use hash as path
            .with_force_compression(builder_compression);

        builder = builder.with_chunk(chunk_builder);
    }

    // Create parent directory if needed
    if let Some(parent) = dst_wad_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Build WAD using WadBuilder API
    let mut output = std::io::BufWriter::new(std::fs::File::create(dst_wad_path)?);

    builder.build_to_writer(&mut output, |path_hash, cursor| {
        // Provide chunk data - check if it's original or deduplicated
        if let Some(data) = chunk_data_map.get(&path_hash) {
            cursor.write_all(data)?;
        } else {
            // This chunk is deduplicated, find the original
            for (&checksum, &original_hash) in &dedup_tracker {
                if let Some(data) = chunk_data_map.get(&original_hash) {
                    let data_checksum = xxh3_64(data);
                    if data_checksum == checksum {
                        cursor.write_all(data)?;
                        break;
                    }
                }
            }
        }
        Ok(())
    })?;

    output.flush()?;

    let elapsed_ms = start.elapsed().as_millis();
    let saved_kb = bytes_saved_dedup / 1024;

    tracing::info!(
        "Patched WAD complete dst={} chunks={} overrides={} audio_uncompressed={} deduplicated={} saved_kb={} elapsed_ms={}",
        dst_wad_path.display(),
        ordered.len(),
        overrides.len(),
        audio_chunks_uncompressed,
        chunks_deduplicated,
        saved_kb,
        elapsed_ms
    );

    Ok(PatchedWadStats {
        chunks_written: ordered.len(),
        overrides_applied: overrides.len(),
        audio_uncompressed: audio_chunks_uncompressed,
        chunks_deduplicated,
        bytes_saved_dedup,
        elapsed_ms,
    })
}

/// Find the byte offset of the first Zstd frame magic (`0x28B52FFD`) in `raw`.
///
/// In `ZstdMulti` chunks, bytes before this offset are an uncompressed header
/// (e.g., a Wwise SoundBank descriptor) that must be preserved as-is.
fn find_zstd_magic_offset(raw: &[u8]) -> Option<usize> {
    raw.windows(ZSTD_MAGIC.len()).position(|w| w == ZSTD_MAGIC)
}

/// Compress `data` with Zstd at compression level 3.
pub fn compress_zstd(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = zstd::Encoder::new(std::io::BufWriter::new(&mut out), 3)?;
    encoder.write_all(data)?;
    encoder.finish()?;
    Ok(out)
}

/// Returns `true` if the data should be Zstd-compressed.
///
/// Audio files (Wwise Bank and Wwise Package) are excluded from compression
/// because the game streams them and benefits from direct access without
/// decompression overhead.
pub fn should_compress(data: &[u8]) -> bool {
    !matches!(
        LeagueFileKind::identify_from_bytes(data),
        LeagueFileKind::WwiseBank | LeagueFileKind::WwisePackage
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_zstd_magic() {
        let data = vec![0x00, 0x01, 0x28, 0xB5, 0x2F, 0xFD, 0x02];
        assert_eq!(find_zstd_magic_offset(&data), Some(2));
    }

    #[test]
    fn test_find_zstd_magic_not_found() {
        let data = vec![0x00, 0x01, 0x02, 0x03];
        assert_eq!(find_zstd_magic_offset(&data), None);
    }

    #[test]
    fn test_compress_zstd() {
        let data = b"Hello, world!".repeat(100);
        let compressed = compress_zstd(&data).unwrap();
        assert!(compressed.len() < data.len());
    }
}
