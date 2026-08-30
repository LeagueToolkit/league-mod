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
//! That last one is [`ltk_wad::WadRebasePlan::tail`]'s job rather than this
//! module's: the layout recorded here is [`WadTailLayout`], and a later build
//! hands it back to `ltk_wad` to rewrite the file in place. What stays here is
//! the full rebuild that lays a WAD out this way to begin with, and the codec
//! policy deciding what an override's bytes are encoded with.
//!
//! # Compression Strategy
//!
//! **Transient chunks** - those no mod overrides, which merely transit the build -
//! are passed through as raw compressed bytes from the original WAD: no
//! decompression or recompression occurs. This is the fast path for the vast
//! majority of chunks.
//!
//! **Override chunks** arrive already encoded, as [`EncodedChunk`] values.
//! [`OverrideEncoding::compress`] auto-detects an override's file type via
//! [`LeagueFileKind::identify_from_bytes`] and applies the ideal compression:
//!
//! - **Audio files** (Wwise Bank / Wwise Package): stored uncompressed (`None`).
//! - **Everything else**: compressed with Zstd at level 3.
//!
//! An override whose mod already holds it as a WAD chunk skips both steps:
//! [`OverrideEncoding::pass_through`] adopts those stored bytes verbatim, so
//! nothing is decoded or re-encoded on the way into the overlay.
//!
//! League validates a chunk shared across WADs by its **compressed** checksum, so
//! every WAD holding a given chunk must receive the same bytes. Compressing once
//! and sharing the [`EncodedChunk`] guarantees that structurally, without
//! relying on the compressor being deterministic across versions.

use crate::content::CompressedChunk;
use crate::error::{CorruptionError, Error, Result, WadLimitError, WadRegion};
use byteorder::{LE, WriteBytesExt};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_file::LeagueFileKind;
use ltk_wad::{FileExt as _, Wad, WadChunk, WadChunkCompression, WadChunks, WadHash};

// Both appear in this module's own public signatures - `PatchedWadStats::layout`
// is a `WadTailLayout`, and `build_patched_wad` resolves overrides to
// `EncodedChunk`s - so they stay nameable from here rather than making a caller
// reach into `ltk_wad` to spell this module's types.
pub use ltk_wad::{EncodedChunk, WadTailLayout};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, SeekFrom, Write};
use std::ops::Range;
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
/// in a real session. The capacity is still recorded and honoured throughout,
/// and both writers zero the slots they leave unfilled, so enabling slack once
/// that is proven is this constant alone.
///
/// While it is zero, capacity equals the entry count, so any change to a WAD's
/// entry set fails the capacity precondition and takes the full-rebuild path -
/// which also means nothing exercises the zero-fill until this is raised.
const TOC_SLACK_ENTRIES: u32 = 0;

/// Highest byte offset the WAD v3.4 format's `u32` offset fields can address.
const MAX_WAD_OFFSET: u64 = u32::MAX as u64;

/// The three ways this crate obtains an override's encoded bytes.
///
/// Encode them fresh ([`compress`](Self::compress)), recover them from an
/// overlay this crate wrote earlier ([`from_wad_bytes`](Self::from_wad_bytes)),
/// or adopt them from a mod that already holds the chunk encoded
/// ([`pass_through`](Self::pass_through)). Which codecs are acceptable in each
/// case is the overlay's policy and not the WAD format's, so it stays here
/// while the chunk type it produces lives in `ltk_wad` - where a rebase can
/// take one without depending on this crate.
///
/// An extension trait rather than a wrapper type: the four accessors a newtype
/// would forward are already [`EncodedChunk`]'s own, so wrapping would buy a
/// layer that every call site unwraps again before handing the chunk to a
/// rebase. Import it for its constructors and nothing else - `use ... as _` is
/// enough. It is sealed: [`EncodedChunk`] is the only sensible implementer, and
/// these return this crate's [`Result`], which no foreign type could produce
/// without manufacturing an [`Error`].
///
/// Building an [`EncodedChunk`] hashes its bytes, so every one of these
/// produces a TOC checksum that describes the bytes it points at rather than
/// one some container claimed. That matters because League validates a chunk
/// shared across WADs by its compressed checksum, and kills the process over a
/// chunk whose checksum disagrees with its content.
pub trait OverrideEncoding: sealed::Sealed + Sized {
    /// Compress `data` with the ideal codec for its detected file type.
    ///
    /// Audio is stored uncompressed, everything else Zstd level 3 (see the
    /// [module docs](self)). `path_hash` only names the chunk in error
    /// messages.
    ///
    /// # Errors
    ///
    /// Returns an error when the compressed or uncompressed size overflows the
    /// WAD v3.4 format's `u32` size fields.
    fn compress(path_hash: WadHash, data: &[u8]) -> Result<Self>;

    /// Recover an encoded chunk from bytes already compressed inside a WAD.
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
    fn from_wad_bytes(chunk: &WadChunk, compressed: Box<[u8]>) -> Result<Self>;

    /// Adopt a chunk's stored bytes verbatim, recomputing their checksum.
    ///
    /// The overlay TOC carries the checksum [`EncodedChunk::new`] computes and
    /// never the one the source claimed, so a container shipping wrong metadata
    /// cannot put that value in a WAD the game loads. Callers compare
    /// [`EncodedChunk::checksum`] against [`CompressedChunk::claimed_checksum`]
    /// to report the disagreement, which is a warning and never a failed build
    /// (`docs/adr/0001-*`).
    ///
    /// `None` when the chunk is stored under a codec this crate does not emit,
    /// which leaves the caller to decode and compress it as usual.
    ///
    /// A stored chunk's uncompressed size is derived from its own byte count
    /// rather than carried over: the two are the same number by definition, and
    /// a TOC where they differ makes the client read past the buffer it
    /// allocated for the chunk. Every other size is the source's, which is the
    /// trust a pass-through accepts in exchange for never decoding.
    ///
    /// # Errors
    ///
    /// Returns an error when the sizes overflow the WAD v3.4 format's `u32`
    /// size fields.
    fn pass_through(path_hash: WadHash, chunk: CompressedChunk) -> Result<Option<Self>>;
}

mod sealed {
    /// Keeps [`OverrideEncoding`](super::OverrideEncoding) unimplementable
    /// outside this crate, so its methods stay free to change.
    pub trait Sealed {}
    impl Sealed for ltk_wad::EncodedChunk {}
}

impl OverrideEncoding for EncodedChunk {
    fn compress(path_hash: WadHash, data: &[u8]) -> Result<Self> {
        let codec = OverrideCodec::for_data(data);
        let compressed = compress_with(data, codec)?;
        ensure_chunk_fits(path_hash, compressed.len(), data.len())?;
        Ok(Self::new(
            compressed,
            data.len() as u32,
            codec.as_wad_compression(),
        ))
    }

    fn from_wad_bytes(chunk: &WadChunk, compressed: Box<[u8]>) -> Result<Self> {
        let encoded = Self::new(
            compressed,
            chunk.uncompressed_size as u32,
            chunk.compression_type,
        );
        if encoded.checksum() != chunk.checksum {
            return Err(CorruptionError::ChunkChecksum {
                path_hash: chunk.path_hash,
                found: encoded.checksum(),
                expected: chunk.checksum,
            }
            .into());
        }

        Ok(encoded)
    }

    fn pass_through(path_hash: WadHash, chunk: CompressedChunk) -> Result<Option<Self>> {
        let Some(codec) = OverrideCodec::for_stored(chunk.compression) else {
            return Ok(None);
        };

        let compressed = chunk.compressed;
        let uncompressed_size = match codec {
            OverrideCodec::Stored => compressed.len(),
            OverrideCodec::Zstd => chunk.uncompressed_size,
        };
        ensure_chunk_fits(path_hash, compressed.len(), uncompressed_size)?;

        Ok(Some(Self::new(
            compressed,
            uncompressed_size as u32,
            codec.as_wad_compression(),
        )))
    }
}

/// A codec this crate writes an override with.
///
/// Narrower than [`WadChunkCompression`] on purpose: the WAD format names codecs
/// this crate reads but never emits, and taking the wider type would leave the
/// writer with unreachable arms to answer for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverrideCodec {
    /// Stored uncompressed - audio, which is already compressed.
    Stored,
    /// Zstd level 3 - everything else.
    Zstd,
}

impl OverrideCodec {
    /// The codec to write `data` with, from its detected file type.
    fn for_data(data: &[u8]) -> Self {
        match LeagueFileKind::identify_from_bytes(data).ideal_compression() {
            WadChunkCompression::None => Self::Stored,
            // Zstd is what this crate writes for everything that is not audio,
            // so any other codec `ltk_file` names maps to the same choice.
            _ => Self::Zstd,
        }
    }

    /// The codec to re-emit a chunk already stored under `compression`, or
    /// `None` when this crate cannot copy those bytes verbatim.
    ///
    /// GZip and Satellite are codecs the crate never writes. A `ZstdMulti`
    /// chunk's bytes are only decodable alongside the subchunk table that
    /// describes them, which lives in its source WAD rather than in the chunk,
    /// so copying one on its own would produce an overlay the game cannot read.
    fn for_stored(compression: WadChunkCompression) -> Option<Self> {
        match compression {
            WadChunkCompression::None => Some(Self::Stored),
            WadChunkCompression::Zstd => Some(Self::Zstd),
            WadChunkCompression::GZip
            | WadChunkCompression::Satellite
            | WadChunkCompression::ZstdMulti => None,
        }
    }

    /// The TOC compression field for this codec.
    fn as_wad_compression(self) -> WadChunkCompression {
        match self {
            Self::Stored => WadChunkCompression::None,
            Self::Zstd => WadChunkCompression::Zstd,
        }
    }
}

/// One slot in a patched WAD's TOC, before its data offset is known.
///
/// A chunk is *transient* when it merely transits the build: no mod overrides
/// it, so its bytes arrive in the copied source region untouched and only its
/// recorded offset moves. The opposite is an override, whose bytes this build
/// writes itself. A transient entry carries the source entry it will be shifted
/// from rather than its hash alone, so "this hash should be in the source TOC
/// but is not" is not a state the writer can reach.
enum PlannedEntry {
    /// A source chunk keeping the bytes copied into the region.
    Transient(WadChunk),
    /// A chunk whose bytes are written into the override tail.
    Override(WadHash),
}

impl PlannedEntry {
    /// The path hash this entry will be sorted and written under.
    fn path_hash(&self) -> WadHash {
        match self {
            Self::Transient(chunk) => chunk.path_hash,
            Self::Override(path_hash) => *path_hash,
        }
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
}

/// What a patched-WAD write produced.
///
/// Build metrics, the [layout](WadTailLayout) of the file that was written, and
/// the identity of the source it was built from.
#[derive(Debug, Clone)]
pub struct PatchedWadStats {
    /// Total number of chunks in the output WAD (original + overrides).
    pub chunks_written: usize,
    /// Number of chunks that were replaced by mod overrides.
    pub overrides_applied: usize,
    /// Number of new entries added (not present in the original WAD).
    pub new_entries_added: usize,
    /// Number of *transient* chunks: those no override replaced, which keep the
    /// original WAD's bytes and only move (see the [module docs](self)).
    pub chunks_transient: usize,
    /// Wall-clock time to build this WAD, in milliseconds.
    pub elapsed_ms: u128,
    /// Where this build put the source data region and the override tail.
    pub layout: WadTailLayout,
    /// The game WAD this output was built from.
    pub source: SourceWadIdentity,
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
///   [`EncodedChunk`]. This allows the caller to lazily hand over override
///   data on demand instead of holding everything in memory, and to compress
///   each distinct content once across all the WADs that hold it.
///
/// # Returns
///
/// [`PatchedWadStats`] with build metrics (chunk counts, timing).
///
/// # Errors
///
/// Fails when the source WAD cannot be read or is truncated, when the output
/// cannot be created or renamed into place, when an override's bytes cannot be
/// resolved by `resolve_override`, or when the result would exceed what the WAD
/// v3.4 format can address - more chunks than a `u32` can count, or a file past
/// 4 GiB. A failed write deletes the temp file, so no half-written WAD survives
/// at either path; only a failed rename can leave one behind, and that one is
/// complete.
pub fn build_patched_wad(
    src_wad_path: &Utf8Path,
    dst_wad_path: &Utf8Path,
    override_hashes: &HashSet<WadHash>,
    resolve_override: impl FnMut(WadHash) -> Result<EncodedChunk>,
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
    override_hashes: &HashSet<WadHash>,
    mut resolve_override: impl FnMut(WadHash) -> Result<EncodedChunk>,
) -> Result<PatchedWadStats> {
    let start = std::time::Instant::now();

    let file = File::open(src_wad_path.as_std_path())
        .map_err(|source| Error::read(src_wad_path, source))?;
    let mmap =
        unsafe { memmap2::Mmap::map(&file).map_err(|source| Error::read(src_wad_path, source))? };
    let wad = Wad::mount(Cursor::new(&mmap[..]))?;
    let chunks = wad.chunks();

    // Collect new entry hashes (in overrides but not in the original WAD)
    let mut new_hashes: Vec<WadHash> = override_hashes
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

    // Build a merged sorted list of ALL entries (original + new).
    // WAD TOC must be sorted by path_hash; binary_search insertion maintains this.
    let mut ordered: Vec<PlannedEntry> = chunks
        .iter()
        .map(|chunk| {
            if override_hashes.contains(&chunk.path_hash) {
                PlannedEntry::Override(chunk.path_hash)
            } else {
                PlannedEntry::Transient(*chunk)
            }
        })
        .collect();
    for hash in &new_hashes {
        let pos = ordered
            .binary_search_by_key(hash, PlannedEntry::path_hash)
            .unwrap_or_else(|i| i);
        ordered.insert(pos, PlannedEntry::Override(*hash));
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

    for entry in &ordered {
        match entry {
            PlannedEntry::Override(path_hash) => {
                overrides_applied += 1;
                let over = resolve_override(*path_hash)?;
                final_chunks.push(write_tail_chunk(
                    &mut writer,
                    *path_hash,
                    &over,
                    &mut tail_cursor,
                    dst_wad_path,
                )?);
            }
            PlannedEntry::Transient(orig) => final_chunks.push(layout.shifted(orig)?),
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
    let chunks_transient = ordered.len() - overrides_applied;

    tracing::info!(
        "Patched WAD complete dst={} chunks={} overrides={} new={} transient={} elapsed_ms={}",
        dst_wad_path,
        ordered.len(),
        overrides_applied,
        new_entries_added,
        chunks_transient,
        elapsed_ms
    );

    Ok(PatchedWadStats {
        chunks_written: ordered.len(),
        overrides_applied,
        new_entries_added,
        chunks_transient,
        elapsed_ms,
        layout,
        source: SourceWadIdentity::new(
            &file
                .metadata()
                .map_err(|source| Error::read(src_wad_path, source))?,
            chunks,
        )?,
    })
}

/// Place the source region and the tail behind a TOC of `toc_capacity` entries.
///
/// The TOC starts at `toc_offset`, and the region directly follows it.
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
        return Err(WadLimitError::FileTooLarge {
            wad: dst_wad_path.to_path_buf(),
            region: WadRegion::SourceRegion,
            offset: tail_offset,
        }
        .into());
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

/// The byte range a source WAD's chunks occupy.
///
/// From its first chunk's offset to the end of its last, derived from the chunk
/// table rather than assumed to begin right after the
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
        return Err(CorruptionError::TruncatedWad {
            wad: src_wad_path.to_path_buf(),
            reach: end,
            len,
        }
        .into());
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

/// Append one override to the tail and return the TOC entry describing it.
///
/// Advances `cursor` past the bytes written, so the offset bookkeeping the tail
/// depends on lives in exactly one place.
///
/// # Errors
///
/// Fails when the chunk would end past what the format's `u32` offset fields
/// can address.
fn write_tail_chunk<W: Write>(
    writer: &mut W,
    path_hash: WadHash,
    over: &EncodedChunk,
    cursor: &mut u64,
    dst_wad_path: &Utf8Path,
) -> Result<WadChunk> {
    let compressed_size = over.compressed().len();
    let end = *cursor + compressed_size as u64;
    if end > MAX_WAD_OFFSET {
        return Err(WadLimitError::FileTooLarge {
            wad: dst_wad_path.to_path_buf(),
            region: WadRegion::OverrideTail,
            offset: end,
        }
        .into());
    }

    let chunk = WadChunk {
        path_hash,
        data_offset: *cursor as usize,
        compressed_size,
        uncompressed_size: over.uncompressed_size() as usize,
        compression_type: over.compression(),
        is_duplicated: false,
        frame_count: 0,
        start_frame: 0,
        checksum: over.checksum(),
    };

    writer.write_all(over.compressed())?;
    *cursor = end;

    Ok(chunk)
}

/// The number of TOC entries to reserve for `entry_count` chunks.
///
/// [`TOC_SLACK_ENTRIES`] must match the slack `ltk_wad` allows a rebase, which
/// it keeps privately: reserving more here than a rebase admits would build
/// WADs that every later rebase refuses, falling back to a full rebuild
/// forever. [`WadTailLayout::admits_entry_count`] is what enforces the
/// agreement, and both are zero today.
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
            Error::from(WadLimitError::TooManyChunks {
                wad: dst_wad_path.to_path_buf(),
                count: entry_count,
            })
        })
}

/// Reject chunk sizes that overflow the WAD v3.4 format's u32 size fields.
fn ensure_chunk_fits(path_hash: WadHash, compressed: usize, uncompressed: usize) -> Result<()> {
    if compressed > u32::MAX as usize || uncompressed > u32::MAX as usize {
        return Err(WadLimitError::ChunkTooLarge {
            path_hash,
            compressed,
            uncompressed,
        }
        .into());
    }
    Ok(())
}

/// Compress an override's bytes with `codec`.
///
/// Total over [`OverrideCodec`]: the two codecs this crate emits are the only
/// ones it can be asked for, so there is no unsupported case to report.
///
/// # Errors
///
/// Fails only when the Zstd encoder does, which it does for I/O reasons alone
/// when writing into a `Vec`.
fn compress_with(data: &[u8], codec: OverrideCodec) -> Result<Vec<u8>> {
    match codec {
        OverrideCodec::Stored => Ok(data.to_vec()),
        OverrideCodec::Zstd => {
            let mut out = Vec::new();
            let mut encoder = zstd::Encoder::new(BufWriter::new(&mut out), 3)?;
            encoder.write_all(data)?;
            encoder.finish()?;
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests;
