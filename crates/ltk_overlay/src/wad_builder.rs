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
//! **Transient chunks** - those no mod overrides, which merely transit the build -
//! are passed through as raw compressed bytes from the original WAD: no
//! decompression or recompression occurs. This is the fast path for the vast
//! majority of chunks.
//!
//! **Override chunks** arrive already compressed, as [`PreparedOverride`] values.
//! [`PreparedOverride::compress`] auto-detects an override's file type via
//! [`LeagueFileKind::identify_from_bytes`] and applies the ideal compression:
//!
//! - **Audio files** (Wwise Bank / Wwise Package): stored uncompressed (`None`).
//! - **Everything else**: compressed with Zstd at level 3.
//!
//! An override whose mod already holds it as a WAD chunk skips both steps:
//! `PreparedOverride::pass_through` adopts those stored bytes verbatim, so
//! nothing is decoded or re-encoded on the way into the overlay.
//!
//! League validates a chunk shared across WADs by its **compressed** checksum, so
//! every WAD holding a given chunk must receive the same bytes. Compressing once
//! and sharing the [`PreparedOverride`] guarantees that structurally, without
//! relying on the compressor being deterministic across versions.

use crate::content::CompressedChunk;
use crate::error::{CorruptionError, Error, Result, WadLimitError, WadRegion};
use byteorder::{LE, WriteBytesExt};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_file::LeagueFileKind;
use ltk_wad::{FileExt as _, Wad, WadChunk, WadChunkCompression, WadChunks, WadHash};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, SeekFrom, Write};
use std::ops::Range;
use std::sync::Arc;
use xxhash_rust::xxh3::xxh3_64;

/// Size of a single v3.4 WAD TOC entry.
const TOC_ENTRY_SIZE: usize = 32;

/// Size of the v3.4 WAD header, which every other offset in the file follows.
///
/// 2 bytes of `RW` magic, 2 of version, 256 of RSA signature and 8 of checksum.
/// The `u32` chunk count sits directly on top of it and the TOC directly on top
/// of that, so the first TOC entry of a well-formed v3.4 WAD begins at 272.
const WAD_HEADER_SIZE: u64 = 268;

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

/// Why a WAD's tail could not be rewritten.
///
/// Everything the tail rewrite alone can refuse, named without reference to
/// where the WAD lives: it is handed a seekable target and a base offset, so it
/// has no path to put in a message. A caller that has one attaches it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WadTailError {
    /// The recorded layout does not place its regions past a header, a chunk
    /// count and a TOC of the recorded capacity.
    #[error(
        "a layout reserving {toc_capacity} TOC entries cannot put its data region at \
         {data_region_offset} and its tail at {tail_offset}"
    )]
    IncoherentLayout {
        /// TOC entries the layout reserved.
        toc_capacity: u32,
        /// Where the layout puts the copied source data region.
        data_region_offset: u64,
        /// Where the layout puts the override tail.
        tail_offset: u64,
    },

    /// The rewritten TOC no longer fits the capacity the file reserved.
    #[error("the WAD reserved {reserved} TOC entries, not the {needed} this rewrite needs")]
    TocCapacity {
        /// Entries the rewritten TOC would hold.
        needed: usize,
        /// Entries the file has room for.
        reserved: u32,
    },

    /// The tail would reach past what the format's `u32` offsets can address.
    #[error(
        "the override tail would reach offset {offset}, past the 4 GiB the WAD v3.4 \
         format can address"
    )]
    TailTooLarge {
        /// The offset the tail would have reached.
        offset: u64,
    },

    /// A source chunk's shifted range falls outside the addressable file.
    #[error("chunk {path_hash:016x} cannot be addressed at offset {offset}")]
    ChunkUnaddressable {
        /// The chunk that would not fit.
        path_hash: WadHash,
        /// Where shifting would have put it.
        offset: i64,
    },

    /// A TOC entry could not be encoded.
    #[error(transparent)]
    Wad(#[from] ltk_wad::WadError),

    /// The target refused a write or a seek.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// What a tail rewrite wrote.
///
/// The numbers the write itself produced, and nothing the caller already held:
/// the layout it passed in, the source it verified and the time it chose to
/// measure are all its own to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WadTailReport {
    /// Entries in the rewritten TOC.
    pub entry_count: usize,
    /// Bytes the override tail now occupies.
    pub tail_len: u64,
}

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
    pub fn compress(path_hash: WadHash, data: &[u8]) -> Result<Self> {
        let codec = OverrideCodec::for_data(data);
        let compressed = compress_with(data, codec)?;
        ensure_chunk_fits(path_hash, compressed.len(), data.len())?;
        Ok(Self {
            checksum: xxh3_64(&compressed),
            compressed: Arc::from(compressed),
            uncompressed_size: data.len() as u32,
            compression: codec.as_wad_compression(),
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
            return Err(CorruptionError::ChunkChecksum {
                path_hash: chunk.path_hash,
                found: checksum,
                expected: chunk.checksum,
            }
            .into());
        }

        Ok(Self {
            compressed: Arc::from(compressed),
            uncompressed_size: chunk.uncompressed_size as u32,
            compression: chunk.compression_type,
            checksum,
        })
    }

    /// Adopt a chunk's stored bytes verbatim, recomputing their checksum.
    ///
    /// The overlay TOC carries the checksum computed here and never the one the
    /// source claimed: the game kills the process over a chunk whose checksum
    /// disagrees with its bytes, so a container shipping wrong metadata must not
    /// be able to put that value in a WAD it loads. Recomputing runs at about
    /// memcpy speed, so the copy stays the cost. Callers compare
    /// [`checksum`](Self::checksum) against
    /// [`CompressedChunk::claimed_checksum`] to report the disagreement, which
    /// is a warning and never a failed build (`docs/adr/0001-*`).
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
    pub(crate) fn pass_through(path_hash: WadHash, chunk: CompressedChunk) -> Result<Option<Self>> {
        let Some(codec) = OverrideCodec::for_stored(chunk.compression) else {
            return Ok(None);
        };

        let compressed = chunk.compressed;
        let uncompressed_size = match codec {
            OverrideCodec::Stored => compressed.len(),
            OverrideCodec::Zstd => chunk.uncompressed_size,
        };
        ensure_chunk_fits(path_hash, compressed.len(), uncompressed_size)?;

        Ok(Some(Self {
            checksum: xxh3_64(&compressed),
            compressed: Arc::from(compressed),
            uncompressed_size: uncompressed_size as u32,
            compression: codec.as_wad_compression(),
        }))
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
    /// Derived by subtracting the TOC from the region offset rather than from
    /// the header size, so where the region sits stays the single fact the rest
    /// of the layout is read off.
    ///
    /// # Errors
    ///
    /// Fails when the layout does not place its region past a header, a chunk
    /// count and a TOC of the recorded capacity. Every field here is
    /// deserialized from a state file that anything could have written, and the
    /// result is a seek target in a file the game loads, so the subtraction
    /// reports rather than wraps.
    pub fn toc_offset(&self) -> std::result::Result<u64, WadTailError> {
        self.chunk_count_offset()
            .map(|offset| offset + size_of::<u32>() as u64)
    }

    /// Absolute offset of the `u32` chunk count, which precedes the TOC.
    ///
    /// # Errors
    ///
    /// Fails on the same layouts as [`toc_offset`](Self::toc_offset), of which
    /// this is the lower bound.
    pub fn chunk_count_offset(&self) -> std::result::Result<u64, WadTailError> {
        let below_region =
            u64::from(self.toc_capacity) * TOC_ENTRY_SIZE as u64 + size_of::<u32>() as u64;
        self.data_region_offset
            .checked_sub(below_region)
            .filter(|&offset| offset >= WAD_HEADER_SIZE)
            .ok_or(WadTailError::IncoherentLayout {
                toc_capacity: self.toc_capacity,
                data_region_offset: self.data_region_offset,
                tail_offset: self.tail_offset,
            })
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
    pub fn shifted(&self, orig: &WadChunk) -> std::result::Result<WadChunk, WadTailError> {
        let shifted = orig.data_offset as i64 + self.offset_delta;
        let end = shifted + orig.compressed_size as i64;
        if shifted < 0 || end > MAX_WAD_OFFSET as i64 {
            return Err(WadTailError::ChunkUnaddressable {
                path_hash: orig.path_hash,
                offset: shifted,
            });
        }

        Ok(WadChunk {
            data_offset: shifted as usize,
            ..*orig
        })
    }

    /// Whether `entry_count` entries fit this TOC without an unreserved gap.
    ///
    /// The count may not exceed the capacity, nor fall short of it by more than
    /// the reserved slack. A gap between the last TOC entry and the first data
    /// byte is exactly what `TOC_SLACK_ENTRIES` is gated on, so at slack zero
    /// this is equality.
    pub fn admits_entry_count(&self, entry_count: usize) -> bool {
        let fewest = self.toc_capacity.saturating_sub(TOC_SLACK_ENTRIES);
        u32::try_from(entry_count).is_ok_and(|count| (fewest..=self.toc_capacity).contains(&count))
    }

    /// Check the layout's own numbers hang together before its offsets are used.
    ///
    /// A layout comes back from a state file that anything could have written,
    /// and [`toc_offset`](Self::toc_offset) subtracts the TOC's size from the
    /// region offset to produce a seek target inside a file the game will load.
    /// Callers that got a layout from anywhere but a fresh build must run this
    /// first.
    ///
    /// # Errors
    ///
    /// Fails when the region does not start past a header, a chunk count and a
    /// TOC of the recorded capacity, or when the tail starts before the region
    /// does.
    pub fn validate(&self) -> std::result::Result<(), WadTailError> {
        self.chunk_count_offset()?;
        if self.tail_offset < self.data_region_offset {
            return Err(WadTailError::IncoherentLayout {
                toc_capacity: self.toc_capacity,
                data_region_offset: self.data_region_offset,
                tail_offset: self.tail_offset,
            });
        }
        Ok(())
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
///   [`PreparedOverride`]. This allows the caller to lazily hand over override
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
    resolve_override: impl FnMut(WadHash) -> Result<PreparedOverride>,
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
    mut resolve_override: impl FnMut(WadHash) -> Result<PreparedOverride>,
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

/// A checked plan to rewrite a WAD's override tail and its TOC.
///
/// The file keeps its header and its copied source data region - the bytes
/// that dominate a game WAD - so the work is bounded by the override bytes,
/// not by the WAD's size.
///
/// Building a plan performs every check that can be made without touching the
/// WAD; [`write`](Self::write) then consumes it. That split is the whole point
/// of the type. Rewriting in place is destructive by nature, so a caller has to
/// commit to it before a byte is written, by truncating a file or growing an
/// entry inside a container, and a plan is the proof that committing is safe.
/// [`tail_len`](Self::tail_len) reports how far a container's own bytes move,
/// which such a caller also needs before it starts.
///
/// Everything a plan needs to be *correct* has been decided by the caller:
/// `layout` and `base_entries` describe a WAD whose identity and TOC it has
/// already verified.
#[derive(Debug)]
pub struct WadTailPlan<'a> {
    layout: WadTailLayout,
    /// The TOC the write emits. A tail hash overwrites its entry here, so a
    /// chunk whose override is gone reverts to the bytes the region still
    /// holds. Consumed and written back out, which for a map WAD is tens of
    /// thousands of entries not worth copying.
    entries: BTreeMap<WadHash, WadChunk>,
    tail: &'a [(WadHash, PreparedOverride)],
    entry_count: usize,
    tail_end: u64,
}

impl<'a> WadTailPlan<'a> {
    /// Check that `tail` can be written into the WAD `layout` describes.
    ///
    /// # Arguments
    ///
    /// * `layout` - The layout recorded when the WAD was built.
    /// * `base_entries` - The TOC entry each chunk already in the copied region
    ///   would have with no override applied, keyed by path hash.
    /// * `tail` - The overrides whose bytes go into the new tail, in the order
    ///   they are written. Their order decides where the bytes land and nothing
    ///   else: each TOC entry carries its own offset, and the TOC itself comes
    ///   out in hash order whatever order the tail was given in. A hash may
    ///   appear once.
    ///
    /// # Errors
    ///
    /// Fails when the recorded layout is incoherent, when the resulting entry
    /// count no longer fits the reserved TOC capacity, or when the tail would
    /// push the WAD past the format's 4 GiB limit. The caller's fallback for
    /// all of these is a full rebuild - and because nothing has been written,
    /// it inherits an untouched WAD.
    pub fn new(
        layout: &WadTailLayout,
        base_entries: BTreeMap<WadHash, WadChunk>,
        tail: &'a [(WadHash, PreparedOverride)],
    ) -> std::result::Result<Self, WadTailError> {
        layout.validate()?;

        let entry_count = merged_entry_count(&base_entries, tail.iter().map(|(hash, _)| *hash));
        if !layout.admits_entry_count(entry_count) {
            return Err(WadTailError::TocCapacity {
                needed: entry_count,
                reserved: layout.toc_capacity,
            });
        }

        // Saturating rather than checked: any sum large enough to wrap a u64 is
        // far past the 4 GiB limit the next check rejects it by, so there is
        // nothing a separate overflow error would tell the caller.
        let tail_end = tail.iter().fold(layout.tail_offset, |end, (_, over)| {
            end.saturating_add(over.compressed().len() as u64)
        });
        if tail_end > MAX_WAD_OFFSET {
            return Err(WadTailError::TailTooLarge { offset: tail_end });
        }

        Ok(Self {
            layout: *layout,
            entries: base_entries,
            tail,
            entry_count,
            tail_end,
        })
    }

    /// Bytes the rewritten override tail will occupy.
    #[must_use]
    pub fn tail_len(&self) -> u64 {
        self.tail_end - self.layout.tail_offset
    }

    /// Entries the rewritten TOC will hold.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entry_count
    }

    /// Write the tail and the TOC into `wad`, whose first byte is at `base`.
    ///
    /// `base` is where the WAD begins inside whatever holds it: `0` for a file
    /// that is one WAD, and the entry's offset for a WAD stored inside an
    /// archive. It shifts every seek and no recorded offset - a TOC entry
    /// counts from the WAD's own first byte, because the game reads the WAD
    /// without knowing what it is stored in.
    ///
    /// The caller owns the container, so the caller makes room: a file is
    /// truncated to the layout's tail offset before this is called, and a WAD
    /// inside an archive has its entry grown or shrunk by the difference
    /// [`tail_len`](Self::tail_len) reports.
    ///
    /// # Errors
    ///
    /// Fails when the target refuses a write or a seek, or when a TOC entry
    /// cannot be encoded. Every check that does not need the target was made
    /// when the plan was built, so a failure here means the target itself gave
    /// out - and, the write being in place, may leave a torn WAD. That is what
    /// a caller's dirty marker exists to cover.
    pub fn write<W: Write + Seek>(
        mut self,
        wad: &mut W,
        base: u64,
    ) -> std::result::Result<WadTailReport, WadTailError> {
        let mut writer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, wad);
        writer.seek(SeekFrom::Start(base + self.layout.tail_offset))?;

        let mut cursor = self.layout.tail_offset;
        for (path_hash, over) in self.tail {
            let chunk = write_tail_chunk(&mut writer, *path_hash, over, &mut cursor)?;
            self.entries.insert(*path_hash, chunk);
        }
        let tail_len = cursor - self.layout.tail_offset;

        // `entries` is a BTreeMap, so this walks the TOC in ascending hash order.
        writer.seek(SeekFrom::Start(base + self.layout.chunk_count_offset()?))?;
        writer.write_u32::<LE>(self.entries.len() as u32)?;
        for chunk in self.entries.values() {
            chunk.write_v3_4(&mut writer)?;
        }
        // Reserved slots this build did not fill are zeroed, as the full-rebuild
        // path zeroes them: rewriting in place would otherwise leave the previous
        // build's entries sitting past the new chunk count. Empty while
        // `TOC_SLACK_ENTRIES` is zero, since capacity then equals the entry count.
        for _ in self.entries.len()..self.layout.toc_capacity as usize {
            writer.write_all(&[0u8; TOC_ENTRY_SIZE])?;
        }

        writer.flush()?;

        Ok(WadTailReport {
            entry_count: self.entries.len(),
            tail_len,
        })
    }
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
    over: &PreparedOverride,
    cursor: &mut u64,
) -> std::result::Result<WadChunk, WadTailError> {
    let compressed_size = over.compressed().len();
    let end = *cursor + compressed_size as u64;
    if end > MAX_WAD_OFFSET {
        return Err(WadTailError::TailTooLarge { offset: end });
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

/// How many TOC entries a base entry set plus a set of tail hashes comes to.
///
/// A tail hash that is already a base entry replaces it rather than adding one,
/// which is what lets an override that was removed revert to the game's bytes
/// without changing the WAD's shape.
pub(crate) fn merged_entry_count(
    base_entries: &BTreeMap<WadHash, WadChunk>,
    tail_hashes: impl IntoIterator<Item = WadHash>,
) -> usize {
    base_entries.len()
        + tail_hashes
            .into_iter()
            .filter(|hash| !base_entries.contains_key(hash))
            .count()
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
