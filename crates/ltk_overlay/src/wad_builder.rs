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
//! # File Layout
//!
//! A patched WAD is written in three regions:
//!
//! ```text
//! [header][chunk count][TOC: 32 bytes x toc_capacity]
//! [source data region - the game WAD's data, copied intact]
//! [override tail    - one entry per overridden or new chunk]
//! ```
//!
//! The format fixes the TOC directly after the header, but every TOC entry
//! carries an absolute `data_offset`, so where the *data* sits is free. Copying
//! the source data as one block, rather than interleaving overrides into it,
//! buys three things:
//!
//! - the copy is sequential, with no per-chunk bookkeeping;
//! - an override that is later *removed* is a TOC edit alone, because the
//!   original bytes are still in the file even while unreferenced;
//! - the only bytes an incremental rebuild must rewrite are the tail and the
//!   TOC, which is what makes rebuilding a WAD cost O(override bytes).
//!
//! # Compression Strategy
//!
//! **Non-overridden chunks** are passed through as raw compressed bytes from the
//! original WAD - no decompression or recompression occurs. This is the fast path
//! for the vast majority of chunks.
//!
//! **Override chunks** arrive already compressed, as [`PreparedOverride`] values.
//! [`PreparedOverride::compress`] auto-detects an override's file type via
//! [`LeagueFileKind::identify_from_bytes`] and applies the ideal compression:
//!
//! - **Audio files** (Wwise Bank / Wwise Package): stored uncompressed (`None`).
//! - **Everything else**: compressed with Zstd at level 3.
//!
//! League validates a chunk shared across WADs by its **compressed** checksum, so
//! every WAD holding a given chunk must receive the same bytes. Compressing once
//! and sharing the [`PreparedOverride`] guarantees that structurally, without
//! relying on the compressor being deterministic across versions.

use crate::error::{Error, Result};
use byteorder::{LE, WriteBytesExt};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_file::LeagueFileKind;
use ltk_wad::{FileExt as _, Wad, WadChunk, WadChunkCompression, WadChunks, WadHash};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Cursor, Seek, SeekFrom, Write};
use std::ops::Range;
use std::sync::Arc;
use xxhash_rust::xxh3::xxh3_64;

/// Size of a single v3.4 WAD TOC entry.
const TOC_ENTRY_SIZE: usize = 32;

/// Write buffer size for the output WAD
const WRITE_BUFFER_SIZE: usize = 1 << 20; // 1 MiB

/// Bytes moved per write when copying the source data region.
///
/// One `write_all` of the whole region would hand the OS a multi-gigabyte
/// buffer; stepping keeps each write in a size the page cache handles well.
const REGION_COPY_STEP: usize = 8 << 20; // 8 MiB

/// TOC entries reserved beyond a patched WAD's current entry count.
///
/// Zero, deliberately. Reserving slack would let a WAD gain or lose a chunk
/// without moving any data, but it leaves a gap between the last TOC entry and
/// the first data byte, and the game has not been observed tolerating that gap
/// in a real session. The capacity is still recorded and honoured throughout, so
/// enabling slack once that is proven is this constant alone.
///
/// While it is zero, capacity equals the entry count, so any change to a WAD's
/// entry set fails the capacity precondition and takes the full-rebuild path.
const TOC_SLACK_ENTRIES: u32 = 0;

/// Highest byte offset the WAD v3.4 format's `u32` offset fields can address.
const MAX_WAD_OFFSET: u64 = u32::MAX as u64;

/// An override chunk's compressed bytes plus the TOC fields describing them.
///
/// Built once per distinct override content and shared by every WAD the override
/// routes to, so all copies of a chunk carry identical bytes and therefore one
/// compressed checksum - which is what the game validates a shared chunk by.
///
/// Cloning is cheap: the bytes live behind an [`Arc`].
#[derive(Debug, Clone)]
pub struct PreparedOverride {
    compressed: Arc<[u8]>,
    uncompressed_size: u32,
    compression: WadChunkCompression,
    /// xxh3_64 of `compressed`, the chunk's TOC checksum field.
    checksum: u64,
}

impl PreparedOverride {
    /// Compress `data` with the ideal codec for its detected file type.
    ///
    /// Audio is stored uncompressed, everything else Zstd level 3 (see the
    /// [module docs](self)). `path_hash` only names the chunk in error messages.
    ///
    /// # Errors
    ///
    /// Returns an error when the compressed or uncompressed size overflows the
    /// WAD v3.4 format's `u32` size fields.
    pub fn compress(path_hash: u64, data: &[u8]) -> Result<Self> {
        let compression = LeagueFileKind::identify_from_bytes(data).ideal_compression();
        let compressed = compress_by_type(data, compression)?;
        ensure_chunk_fits(path_hash, compressed.len(), data.len(), "Override")?;
        Ok(Self {
            checksum: xxh3_64(&compressed),
            compressed: Arc::from(compressed),
            uncompressed_size: data.len() as u32,
            compression,
        })
    }

    /// Recover a prepared override from bytes already compressed inside a WAD.
    ///
    /// Lets an unchanged override be lifted straight out of an overlay's
    /// existing tail instead of being re-read from its mod and compressed
    /// again. The bytes are re-emitted verbatim, so a WAD that builds the same
    /// content fresh in the same build still matches them byte for byte.
    ///
    /// # Errors
    ///
    /// Fails when the bytes do not hash to `chunk.checksum`, which means the
    /// WAD they came from is not what its own TOC says it is.
    pub(crate) fn from_wad_bytes(chunk: &WadChunk, compressed: Box<[u8]>) -> Result<Self> {
        let checksum = xxh3_64(&compressed);
        if checksum != chunk.checksum {
            return Err(Error::Other(format!(
                "Chunk {:016x} does not match its recorded checksum \
                 (found {checksum:016x}, expected {:016x})",
                chunk.path_hash, chunk.checksum
            )));
        }

        Ok(Self {
            compressed: Arc::from(compressed),
            uncompressed_size: chunk.uncompressed_size as u32,
            compression: chunk.compression_type,
            checksum,
        })
    }

    /// The compressed bytes, written into the WAD verbatim.
    pub fn compressed(&self) -> &[u8] {
        &self.compressed
    }

    /// Size of the chunk before compression.
    pub fn uncompressed_size(&self) -> u32 {
        self.uncompressed_size
    }

    /// Codec [`compressed`](Self::compressed) is encoded with.
    pub fn compression(&self) -> WadChunkCompression {
        self.compression
    }

    /// xxh3_64 of the compressed bytes - the chunk's TOC checksum.
    pub fn checksum(&self) -> u64 {
        self.checksum
    }
}

/// Where a patched WAD's regions sit, and how source offsets map into it.
///
/// Recorded after a build so a later rebuild of the same source WAD can verify
/// the file it finds on disk and rewrite only its tail. See the
/// [module docs](self) on the file layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WadTailLayout {
    /// Absolute offset where the copied source data region starts.
    pub data_region_offset: u64,
    /// Added to a source chunk's `data_offset` to get its offset in this file.
    ///
    /// Signed: the patched TOC can be shorter than the source's own when the
    /// source WAD pads between its TOC and its first chunk.
    pub offset_delta: i64,
    /// Absolute offset where the override tail starts (the region's end).
    pub tail_offset: u64,
    /// TOC entries the file has room for without moving any data.
    ///
    /// Equal to the entry count while `TOC_SLACK_ENTRIES` is zero.
    pub toc_capacity: u32,
}

impl WadTailLayout {
    /// Absolute offset of the first TOC entry.
    ///
    /// Derived from the region offset rather than the header size, so the
    /// 272-byte header stays a fact of the writer alone.
    pub fn toc_offset(&self) -> u64 {
        self.data_region_offset - u64::from(self.toc_capacity) * TOC_ENTRY_SIZE as u64
    }

    /// Absolute offset of the `u32` chunk count, which precedes the TOC.
    pub fn chunk_count_offset(&self) -> u64 {
        self.toc_offset() - size_of::<u32>() as u64
    }

    /// A source chunk's TOC entry with its offset moved into the copied region.
    ///
    /// Every other field carries over untouched: the bytes came out of a valid
    /// v3.4 WAD and were copied verbatim, so their sizes, compression, frame
    /// fields and checksum still describe them exactly.
    ///
    /// # Errors
    ///
    /// Fails when the shifted range falls outside what the format's `u32`
    /// offset fields can address.
    pub fn shift(&self, orig: &WadChunk) -> Result<WadChunk> {
        let shifted = orig.data_offset as i64 + self.offset_delta;
        let end = shifted + orig.compressed_size as i64;
        if shifted < 0 || end > MAX_WAD_OFFSET as i64 {
            return Err(Error::Other(format!(
                "Patched WAD cannot address chunk {:016x} at offset {shifted}",
                orig.path_hash
            )));
        }

        Ok(WadChunk {
            data_offset: shifted as usize,
            ..*orig
        })
    }

    /// Whether `entry_count` entries fit the TOC without opening a gap wider
    /// than the slack this layout deliberately reserved.
    ///
    /// A gap between the last TOC entry and the first data byte is exactly what
    /// `TOC_SLACK_ENTRIES` is gated on, so at slack zero this is equality.
    pub fn admits_entry_count(&self, entry_count: usize) -> bool {
        let fewest = self.toc_capacity.saturating_sub(TOC_SLACK_ENTRIES);
        u32::try_from(entry_count).is_ok_and(|count| (fewest..=self.toc_capacity).contains(&count))
    }
}

/// What identifies the game WAD an overlay was built from.
///
/// Compared against the file on disk before an overlay built from it is
/// trusted: length and mtime are the cheap filter, the TOC hash the check that
/// actually proves the archive is the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceWadIdentity {
    /// Length of the source WAD in bytes.
    pub len: u64,
    /// Modification time in nanoseconds since the Unix epoch, or `0` when the
    /// platform does not report one.
    pub mtime: i64,
    /// xxh3_64 over the source WAD's TOC entries, in file order.
    pub toc_hash: u64,
}

impl SourceWadIdentity {
    /// Identify a WAD already open and mounted.
    ///
    /// # Errors
    ///
    /// Fails when the TOC cannot be re-encoded for hashing.
    pub fn new(metadata: &std::fs::Metadata, chunks: &WadChunks) -> Result<Self> {
        Ok(Self {
            len: metadata.len(),
            mtime: mtime_nanos(metadata),
            toc_hash: toc_hash(chunks)?,
        })
    }

    /// Read the identity of the WAD at `path`.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be opened or is not a mountable WAD.
    pub fn of(path: &Utf8Path) -> Result<Self> {
        let file = File::open(path.as_std_path()).map_err(|source| Error::read(path, source))?;
        let metadata = file
            .metadata()
            .map_err(|source| Error::read(path, source))?;
        let wad = Wad::mount(BufReader::new(&file))?;

        Self::new(&metadata, wad.chunks())
    }
}

/// The outcome of [`build_patched_wad`]: build metrics, the
/// [layout](WadTailLayout) of the file that was written, and the identity of
/// the source it was built from.
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
    /// Where this build put the source data region and the override tail.
    pub layout: WadTailLayout,
    /// The game WAD this output was built from.
    ///
    /// `None` for a [tail rewrite](rewrite_wad_tail), which never opens the
    /// source: its caller already verified the identity to get there.
    pub source: Option<SourceWadIdentity>,
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
///   write pass. Returns the override's already-compressed
///   [`PreparedOverride`]. This allows the caller to lazily hand over override
///   data on demand instead of holding everything in memory, and to compress
///   each distinct content once across all the WADs that hold it.
///
/// # Returns
///
/// [`PatchedWadStats`] with build metrics (chunk counts, timing).
pub fn build_patched_wad(
    src_wad_path: &Utf8Path,
    dst_wad_path: &Utf8Path,
    override_hashes: &HashSet<u64>,
    resolve_override: impl FnMut(u64) -> Result<PreparedOverride>,
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
fn write_patched_wad(
    src_wad_path: &Utf8Path,
    out_path: &Utf8Path,
    dst_wad_path: &Utf8Path,
    override_hashes: &HashSet<u64>,
    mut resolve_override: impl FnMut(u64) -> Result<PreparedOverride>,
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
        .filter(|&&h| !chunks.contains(WadHash(h)))
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
    let mut ordered: Vec<u64> = chunks.iter().map(|c| c.path_hash.0).collect();
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

    // Reserve the TOC - written for real once every data offset is known.
    let toc_offset = writer.stream_position()?;
    let toc_capacity = toc_capacity_for(ordered.len(), dst_wad_path)?;
    for _ in 0..toc_capacity {
        writer.write_all(&[0u8; TOC_ENTRY_SIZE])?;
    }

    let region = source_data_region(chunks, mmap.len(), src_wad_path)?;
    let layout = plan_layout(toc_offset, toc_capacity, &region, dst_wad_path)?;

    copy_source_region(&mut writer, &mmap[..], &region)?;

    // Overridden and new chunks all live in the tail; everything else keeps the
    // bytes just copied and only shifts its offset.
    let mut final_chunks: Vec<WadChunk> = Vec::with_capacity(ordered.len());
    let mut tail_cursor = layout.tail_offset;

    for &path_hash in &ordered {
        if override_hashes.contains(&path_hash) {
            overrides_applied += 1;
            let over = resolve_override(path_hash)?;
            final_chunks.push(tail_chunk(path_hash, &over, tail_cursor, dst_wad_path)?);
            writer.write_all(over.compressed())?;
            tail_cursor += over.compressed().len() as u64;
        } else {
            let orig = chunks
                .get(WadHash(path_hash))
                .ok_or_else(|| Error::Other(format!("Missing base chunk {path_hash:016x}")))?;
            final_chunks.push(layout.shift(orig)?);
        }
    }

    // Seek back and write final TOC. Any reserved-but-unused capacity keeps the
    // zeroes written above.
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
        layout,
        source: Some(SourceWadIdentity::new(
            &file
                .metadata()
                .map_err(|source| Error::read(src_wad_path, source))?,
            chunks,
        )?),
    })
}

/// Rebuild a patched WAD by rewriting only its override tail and its TOC.
///
/// The file keeps its header and its copied source data region - the bytes that
/// dominate a game WAD - so the work is bounded by the override bytes, not by
/// the WAD's size. Everything this needs to be safe has been decided by the
/// caller: `layout` and `base_entries` describe a file whose identity and TOC
/// it already verified against the game WAD and the recorded layout.
///
/// # Arguments
///
/// * `wad_path` - The overlay WAD to rewrite in place.
/// * `layout` - The layout recorded when this file was built.
/// * `base_entries` - TOC entries whose data already sits in the region, keyed
///   by path hash. Every chunk of the result that is not an override.
/// * `tail_hashes` - Path hashes whose data goes into the new tail, ascending.
/// * `resolve_override` - Supplies each tail hash's compressed bytes, in the
///   order they are written.
///
/// # Errors
///
/// Fails when the file cannot be opened or written, when the resulting entry
/// count no longer fits the reserved TOC capacity, or when the tail would push
/// the file past the format's 4 GiB limit. The caller's fallback for all of
/// these is a full rebuild.
pub fn rewrite_wad_tail(
    wad_path: &Utf8Path,
    layout: &WadTailLayout,
    base_entries: &BTreeMap<u64, WadChunk>,
    tail_hashes: &[u64],
    mut resolve_override: impl FnMut(u64) -> Result<PreparedOverride>,
) -> Result<PatchedWadStats> {
    let start = std::time::Instant::now();

    let entry_count = base_entries.len() + tail_hashes.len();
    if !layout.admits_entry_count(entry_count) {
        return Err(Error::Other(format!(
            "Overlay WAD {wad_path} has room for {} TOC entries, not the {entry_count} \
             this rebuild needs",
            layout.toc_capacity
        )));
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(wad_path.as_std_path())
        .map_err(|source| Error::write(wad_path, source))?;
    file.set_len(layout.tail_offset)
        .map_err(|source| Error::write(wad_path, source))?;

    let mut writer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);
    writer.seek(SeekFrom::Start(layout.tail_offset))?;

    let mut entries = base_entries.clone();
    let mut tail_cursor = layout.tail_offset;
    for &path_hash in tail_hashes {
        let over = resolve_override(path_hash)?;
        entries.insert(
            path_hash,
            tail_chunk(path_hash, &over, tail_cursor, wad_path)?,
        );
        writer.write_all(over.compressed())?;
        tail_cursor += over.compressed().len() as u64;
    }

    // `entries` is a BTreeMap, so this walks the TOC in ascending hash order.
    writer.seek(SeekFrom::Start(layout.chunk_count_offset()))?;
    writer.write_u32::<LE>(entries.len() as u32)?;
    for chunk in entries.values() {
        chunk.write_v3_4(&mut writer)?;
    }

    writer.flush()?;

    let elapsed_ms = start.elapsed().as_millis();
    tracing::info!(
        "Rewrote WAD tail dst={} chunks={} overrides={} tail_bytes={} elapsed_ms={}",
        wad_path,
        entries.len(),
        tail_hashes.len(),
        tail_cursor - layout.tail_offset,
        elapsed_ms
    );

    Ok(PatchedWadStats {
        chunks_written: entries.len(),
        overrides_applied: tail_hashes.len(),
        new_entries_added: 0,
        chunks_passed_through: base_entries.len(),
        elapsed_ms,
        layout: *layout,
        source: None,
    })
}

/// Place the copied source region and the tail behind a TOC of `toc_capacity`
/// entries starting at `toc_offset`.
///
/// # Errors
///
/// Fails when the region alone would push the file past the format's 4 GiB
/// addressable limit.
fn plan_layout(
    toc_offset: u64,
    toc_capacity: u32,
    region: &Range<u64>,
    dst_wad_path: &Utf8Path,
) -> Result<WadTailLayout> {
    let data_region_offset = toc_offset + u64::from(toc_capacity) * TOC_ENTRY_SIZE as u64;
    let tail_offset = data_region_offset + (region.end - region.start);
    if tail_offset > MAX_WAD_OFFSET {
        return Err(Error::Other(format!(
            "Patched WAD {dst_wad_path} exceeds the 4 GiB limit of the WAD v3.4 format: \
             the source data region alone ends at offset {tail_offset}"
        )));
    }

    Ok(WadTailLayout {
        data_region_offset,
        offset_delta: data_region_offset as i64 - region.start as i64,
        tail_offset,
        toc_capacity,
    })
}

/// xxh3_64 over a WAD's TOC entries in file order.
///
/// Hashes the entries as they would be written rather than the raw bytes at
/// some offset, so the identity of an archive is independent of where its TOC
/// happens to start.
fn toc_hash(chunks: &WadChunks) -> Result<u64> {
    let mut buf = Vec::with_capacity(chunks.len() * TOC_ENTRY_SIZE);
    for chunk in chunks {
        chunk.write_v3_4(&mut buf)?;
    }
    Ok(xxh3_64(&buf))
}

/// A file's modification time in nanoseconds since the Unix epoch.
///
/// `0` when the platform does not report one, or the timestamp predates the
/// epoch: both sides of every comparison read it the same way, and the TOC hash
/// is what actually proves identity.
fn mtime_nanos(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_nanos()).ok())
        .unwrap_or(0)
}

/// The byte range of a source WAD's data region: from its first chunk's offset
/// to the end of its last.
///
/// Derived from the chunk table rather than assumed to begin right after the
/// TOC, so a WAD that pads between the two still copies exactly the bytes its
/// entries point into. An empty WAD has an empty region.
///
/// # Errors
///
/// Fails when a chunk points outside the file, which means the source WAD is
/// truncated or corrupt.
fn source_data_region(
    chunks: &WadChunks,
    len: usize,
    src_wad_path: &Utf8Path,
) -> Result<Range<u64>> {
    let Some(start) = chunks.iter().map(|c| c.data_offset).min() else {
        return Ok(0..0);
    };
    let end = chunks
        .iter()
        .map(|c| c.data_offset + c.compressed_size)
        .max()
        .unwrap_or(start);

    if end > len {
        return Err(Error::Other(format!(
            "Source WAD {src_wad_path} is truncated: its chunks reach offset {end} \
             but the file is {len} bytes"
        )));
    }

    Ok(start as u64..end as u64)
}

/// Copy the source WAD's data region into the output in sequential steps.
fn copy_source_region<W: Write>(writer: &mut W, mmap: &[u8], region: &Range<u64>) -> Result<()> {
    let region = region.start as usize..region.end as usize;
    for step in mmap[region].chunks(REGION_COPY_STEP) {
        writer.write_all(step)?;
    }
    Ok(())
}

/// The TOC entry for an override written at `offset` in the tail.
fn tail_chunk(
    path_hash: u64,
    over: &PreparedOverride,
    offset: u64,
    dst_wad_path: &Utf8Path,
) -> Result<WadChunk> {
    let compressed_size = over.compressed().len();
    if offset + compressed_size as u64 > MAX_WAD_OFFSET {
        return Err(Error::Other(format!(
            "Patched WAD {dst_wad_path} exceeds the 4 GiB limit of the WAD v3.4 format \
             (override {path_hash:016x} would end at offset {})",
            offset + compressed_size as u64
        )));
    }

    Ok(WadChunk {
        path_hash: WadHash(path_hash),
        data_offset: offset as usize,
        compressed_size,
        uncompressed_size: over.uncompressed_size() as usize,
        compression_type: over.compression(),
        is_duplicated: false,
        frame_count: 0,
        start_frame: 0,
        checksum: over.checksum(),
    })
}

/// The number of TOC entries to reserve for `entry_count` chunks.
///
/// # Errors
///
/// Fails when the entry count (plus [`TOC_SLACK_ENTRIES`]) overflows the
/// format's `u32` chunk count.
fn toc_capacity_for(entry_count: usize, dst_wad_path: &Utf8Path) -> Result<u32> {
    u32::try_from(entry_count)
        .ok()
        .and_then(|count| count.checked_add(TOC_SLACK_ENTRIES))
        .ok_or_else(|| {
            Error::Other(format!(
                "Patched WAD {dst_wad_path} has {entry_count} chunks, more than the \
                 WAD v3.4 format can index"
            ))
        })
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
