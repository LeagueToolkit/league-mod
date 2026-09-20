//! [`apply_delta`]: repair an archive's content without repacking it.
//!
//! A repair changes a handful of files in an archive that may be hundreds of
//! megabytes. Packing the mod's project again re-encodes every chunk in it, so
//! the cost is the archive's size rather than the change's.
//!
//! An [`ArchiveDelta`] is that change stated on its own: the chunks and entries
//! whose bytes differ, and nothing about the archive they are going into. This
//! does the same repair by writing only what the delta names. A chunk inside a
//! packed WAD is re-encoded and appended to that WAD's tail with its TOC entry
//! rewritten in place ([`ltk_wad::WadRebasePlan`]); every other chunk keeps the
//! bytes and the TOC entry it already had. Whole archive entries - hashtables,
//! files outside a `.wad.client` directory - are replaced as
//! [`replace_entries`](crate::replace_entries) replaces them, and every entry
//! nobody named is raw-copied, wrong CRC32 values included.
//!
//! A chunk dropped by a delta is simply not written.
//!
//! The archive is written through a temporary file and renamed over `dest`, so
//! `source` and `dest` may be the same path and an interrupted repair never
//! leaves a half-written archive where a mod should be.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_file::LeagueFileKind;
use ltk_wad::{
    EncodedChunk, FileExt as _, Wad, WadChunk, WadChunkCompression, WadHash, WadRebaseError,
    WadRebasePlan, WadTailLayout,
};

use crate::error::{IoResultExt as _, PathIo};
use crate::reader::is_packed_wad;
use crate::{
    FantomeEntry, FantomeExtractError, FantomeReader, FantomeWriteError, FantomeWriter,
    classify_entry,
};

/// Size of the WAD v3.4 header, which every other offset in the file follows.
///
/// 2 bytes of `RW` magic, 2 of version, 256 of RSA signature and 8 of checksum.
/// The `u32` chunk count sits directly on top of it and the TOC directly on top
/// of that.
const WAD_HEADER_SIZE: u64 = 268;

/// Size of a single v3.4 WAD TOC entry.
const TOC_ENTRY_SIZE: u64 = 32;

/// The `RW` magic every WAD starts with.
const WAD_MAGIC: [u8; 2] = [0x52, 0x57];

/// The only WAD version a rebase can write.
///
/// [`WadChunk::write_v3_4`](ltk_wad::WadChunk::write_v3_4) is what a rebase
/// emits TOC entries with, and an older WAD's entries are neither that shape
/// nor that size, so rewriting one as v3.4 would move every chunk offset the
/// game reads.
const REBASABLE_WAD_VERSION: (u8, u8) = (3, 4);

/// Zstd level a replaced chunk is compressed at.
///
/// The same level [`ltk_wad::WadBuilder`] writes a fresh chunk at, so a chunk
/// this repairs and one a repack would have written are compressed alike.
const ZSTD_LEVEL: i32 = 3;

/// The change to make to an archive: chunks inside packed WADs, whole entries,
/// or both.
///
/// Built up with [`chunk`](Self::chunk) and [`entry`](Self::entry), then applied
/// with [`apply_delta`]. Both take the new bytes uncompressed, as the file's own
/// content: the chunk half encodes them under a WAD codec, and the entry half
/// stores them the way the archive stores that entry.
///
/// A delta names only what differs, and is written without reference to the
/// archive it will be applied to, so the same one can be built from a repair's
/// findings before any archive is opened.
///
/// A chunk is addressed by its [`WadHash`] rather than by the path it was
/// extracted under, because the extraction's naming is not invertible by
/// spelling alone - a resolver may name a chunk with sixteen hex digits, a
/// collided path gains a `.ltk` suffix, and a name the file system refused
/// falls back to the bare hash. Derive the hash with
/// [`ltk_wad::chunk_hash_of`], which reads all three back.
#[derive(Default, Clone)]
pub struct ArchiveDelta<'a> {
    /// Keyed by the WAD's lower-cased name, since the archive matches its
    /// `WAD/` entries case-insensitively and two spellings must not become two
    /// rebases of one WAD.
    chunks: BTreeMap<String, WadDelta<'a>>,
    /// Keyed by the entry path lower-cased, on the same terms.
    entries: BTreeMap<String, (String, Cow<'a, [u8]>)>,
    /// The entries to drop, in `entries`' key space and disjoint from it.
    removed_entries: BTreeSet<String>,
}

/// The part of a delta that lands inside one packed WAD.
#[derive(Default, Clone)]
struct WadDelta<'a> {
    /// The WAD's name as the caller first spelled it, for error messages.
    name: String,
    chunks: BTreeMap<WadHash, Cow<'a, [u8]>>,
    /// The chunks to drop, disjoint from `chunks`.
    removed: BTreeSet<WadHash>,
}

impl<'a> ArchiveDelta<'a> {
    /// An empty delta, which [`apply_delta`] turns into a copy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the chunk `path_hash` of the packed WAD `wad_name`.
    ///
    /// `wad_name` is the WAD's `.wad.client` name as the archive's `WAD/` entry
    /// spells it, matched case-insensitively. `bytes` are the chunk's content
    /// uncompressed; what it is stored under is read off those bytes, so naming
    /// one hash in two WADs lands one encoding in both. Naming one hash twice
    /// keeps the last bytes given, and naming one already given to
    /// [`remove_chunk`](Self::remove_chunk) takes it back.
    pub fn chunk(
        &mut self,
        wad_name: &str,
        path_hash: WadHash,
        bytes: impl Into<Cow<'a, [u8]>>,
    ) -> &mut Self {
        let wad = self.wad_mut(wad_name);
        wad.removed.remove(&path_hash);
        wad.chunks.insert(path_hash, bytes.into());
        self
    }

    /// Drop the chunk `path_hash` from the packed WAD `wad_name`.
    ///
    /// `wad_name` is matched as [`chunk`](Self::chunk) matches it. Naming a
    /// chunk the WAD does not hold does nothing, and naming one already given
    /// to [`chunk`](Self::chunk) takes it back.
    pub fn remove_chunk(&mut self, wad_name: &str, path_hash: WadHash) -> &mut Self {
        let wad = self.wad_mut(wad_name);
        wad.chunks.remove(&path_hash);
        wad.removed.insert(path_hash);
        self
    }

    /// Replace the archive entry at `entry_path`, or add it where there is none.
    ///
    /// `entry_path` is the path the archive names the entry by
    /// (`META/hashes/game.hashes.txt`, `RAW/assets/x.bin`), matched
    /// case-insensitively. Naming one path twice keeps the last bytes given,
    /// and naming one already given to [`remove_entry`](Self::remove_entry)
    /// takes it back.
    pub fn entry(&mut self, entry_path: &str, bytes: impl Into<Cow<'a, [u8]>>) -> &mut Self {
        let key = entry_path.to_ascii_lowercase();
        self.removed_entries.remove(&key);
        self.entries
            .insert(key, (entry_path.to_owned(), bytes.into()));
        self
    }

    /// Drop the archive entry at `entry_path`.
    ///
    /// `entry_path` is matched as [`entry`](Self::entry) matches it. Naming an
    /// entry the archive does not hold does nothing, and naming one already
    /// given to [`entry`](Self::entry) takes it back.
    pub fn remove_entry(&mut self, entry_path: &str) -> &mut Self {
        let key = entry_path.to_ascii_lowercase();
        self.entries.remove(&key);
        self.removed_entries.insert(key);
        self
    }

    /// Whether nothing is named.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty() && self.entries.is_empty() && self.removed_entries.is_empty()
    }

    /// The delta's record for `wad_name`, added empty where there is none.
    fn wad_mut(&mut self, wad_name: &str) -> &mut WadDelta<'a> {
        self.chunks
            .entry(wad_name.to_ascii_lowercase())
            .or_insert_with(|| WadDelta {
                name: wad_name.to_owned(),
                chunks: BTreeMap::new(),
                removed: BTreeSet::new(),
            })
    }

    /// How many chunks are named for replacement, across every WAD.
    fn chunk_count(&self) -> usize {
        self.chunks.values().map(|wad| wad.chunks.len()).sum()
    }

    /// How many chunks are named for removal, across every WAD.
    fn removed_chunk_count(&self) -> usize {
        self.chunks.values().map(|wad| wad.removed.len()).sum()
    }
}

impl fmt::Debug for ArchiveDelta<'_> {
    /// Counts rather than content: a delta carries file bytes, and a repair of a
    /// map mod carries megabytes of them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArchiveDelta")
            .field("wads", &self.chunks.len())
            .field("chunks", &self.chunk_count())
            .field("chunks_removed", &self.removed_chunk_count())
            .field("entries", &self.entries.len())
            .field("entries_removed", &self.removed_entries.len())
            .finish()
    }
}

/// What an [`apply_delta`] step is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeltaStep {
    /// Rebasing one packed WAD, which costs a copy of that WAD.
    RebaseWad,
    /// Writing one archive entry into the destination.
    WriteEntry,
}

/// One step of an [`apply_delta`], reported before the step is taken.
///
/// The steps are the WAD rebases followed by the entries the rewrite writes,
/// and `total` counts both, so a caller showing a bar sees it fill once. A
/// dropped entry is not a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeltaProgress<'a> {
    /// The WAD or archive entry the step names.
    pub name: &'a str,
    /// What the step is doing.
    pub step: DeltaStep,
    /// Which step this is, counting from 0.
    pub index: u32,
    /// How many steps applying the delta will take.
    pub total: u32,
}

/// What applying a delta wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeltaReport {
    /// How many packed WADs were rebased.
    pub wads_rebased: usize,
    /// How many chunks were written into those WADs' tails.
    pub chunks_replaced: usize,
    /// How many chunks were dropped from those WADs' tables of contents.
    ///
    /// Counts only chunks the WAD held.
    pub chunks_removed: usize,
    /// How many whole archive entries were replaced or added.
    pub entries_replaced: usize,
    /// How many whole archive entries were dropped, on the same terms.
    pub entries_removed: usize,
}

/// Failure to apply a delta to an archive.
///
/// Every variant but [`Write`](Self::Write) and [`Io`](Self::Io) is raised
/// before the destination is touched, so a caller whose fallback is a full
/// repack inherits an untouched archive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FantomeDeltaError {
    /// A file could not be read or written.
    #[error("Failed to access {path}")]
    Io {
        /// The file that failed.
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    /// The source archive could not be read.
    #[error(transparent)]
    Read(#[from] FantomeExtractError),

    /// The rewritten archive could not be written.
    #[error(transparent)]
    Write(#[from] FantomeWriteError),

    /// The archive holds no packed WAD under that name.
    ///
    /// A WAD the archive ships as a directory of loose files has no packed
    /// bytes to rebase; replace its files as entries instead.
    #[error("The archive holds no packed WAD named {wad}")]
    WadNotPacked {
        /// The WAD named, as the caller spelled it.
        wad: String,
    },

    /// The packed WAD is not the v3.4 a rebase writes.
    #[error("{wad} is a v{major}.{minor} WAD, which cannot be rebased")]
    UnsupportedWadVersion {
        /// The WAD named, as the caller spelled it.
        wad: String,
        /// Major version the WAD declares.
        major: u8,
        /// Minor version the WAD declares.
        minor: u8,
    },

    /// A chunk starts inside the WAD's own header or TOC.
    ///
    /// A rebase rewrites the TOC where the format puts it, so a WAD whose data
    /// begins before the TOC ends is one it would overwrite.
    #[error("Chunk {path_hash:016x} of {wad} starts at {data_offset}, inside the WAD's own TOC")]
    ChunkInsideToc {
        /// The WAD named, as the caller spelled it.
        wad: String,
        /// The chunk that starts too early.
        path_hash: WadHash,
        /// Where its TOC entry puts it.
        data_offset: u64,
    },

    /// The WAD holds no chunk under that hash.
    ///
    /// A rebase only rewrites chunks the WAD already has: adding one changes
    /// the entry count, which the format's zero-slack TOC has no room for.
    #[error("{wad} holds no chunk {path_hash:016x}")]
    ChunkAbsent {
        /// The WAD named, as the caller spelled it.
        wad: String,
        /// The chunk that is not there.
        path_hash: WadHash,
    },

    /// The chunk being replaced is a subchunked body.
    ///
    /// A [`ZstdMulti`](ltk_wad::WadChunkCompression::ZstdMulti) chunk decodes
    /// only alongside subchunk records that live elsewhere in the archive, so a
    /// rebase - which writes one run of bytes and zeroes the subchunk fields -
    /// cannot produce an entry the game can resolve. Only a chunk actually
    /// being replaced is refused; the others pass through untouched.
    #[error("Chunk {path_hash:016x} of {wad} is subchunked, which a rebase cannot rewrite")]
    SubchunkedChunk {
        /// The WAD named, as the caller spelled it.
        wad: String,
        /// The subchunked chunk.
        path_hash: WadHash,
    },

    /// A delta's chunk is larger than the format's `u32` size fields hold.
    #[error("Chunk {path_hash:016x} of {wad} would be {len} bytes, past the format's 4 GiB limit")]
    ChunkTooLarge {
        /// The WAD named, as the caller spelled it.
        wad: String,
        /// The chunk that would not fit.
        path_hash: WadHash,
        /// How many bytes the replacement holds.
        len: usize,
    },

    /// One WAD is named both as a whole entry and by its chunks.
    ///
    /// Which of the two wins would decide whether the repair lands, so it is
    /// refused rather than guessed.
    #[error("{wad} is named both as a whole entry and by its chunks")]
    ConflictingWad {
        /// The WAD named twice, as the archive spells its entry.
        wad: String,
    },

    /// The rebase refused the WAD.
    #[error("Failed to rebase {wad}")]
    Rebase {
        /// The WAD named, as the caller spelled it.
        wad: String,
        #[source]
        source: WadRebaseError,
    },
}

impl From<PathIo> for FantomeDeltaError {
    fn from(failed: PathIo) -> Self {
        Self::Io {
            path: failed.path,
            source: failed.source,
        }
    }
}

impl FantomeDeltaError {
    /// A failure reading the source archive, which is not one of `dest`'s.
    fn read(source: io::Error) -> Self {
        Self::Read(FantomeExtractError::Io(source))
    }
}

/// Rewrite the archive at `source` into `dest`, applying `delta` and nothing
/// else.
///
/// The cost is the changed chunks plus a byte copy of the archive, where
/// packing the mod's project again re-encodes every chunk in it. A chunk of a
/// packed WAD is re-encoded and appended to that WAD's tail with its TOC entry
/// rewritten; every chunk nobody named keeps both its bytes and its TOC entry
/// verbatim, subchunked bodies included. A named archive entry is written with
/// its new bytes and every other entry is raw-copied, wrong CRC32 values
/// included.
///
/// A chunk or entry the delta drops is left out of the rewrite, and naming one
/// the archive does not hold is not an error.
///
/// Packed WADs come last in the rewritten archive, so a later edit that grows
/// one moves only the central directory. `progress`, when given, is called once
/// before each WAD rebase and once before each entry written.
///
/// `dest` ends up holding the archive whatever the delta named, an empty one
/// included, so a caller does not have to know whether it had anything to do
/// before it can find the mod. The rewrite lands as a temporary file beside
/// `dest` and is renamed over it only once writing finishes cleanly, so
/// `source` and `dest` may be the same path and an interrupted repair leaves
/// the mod as it was.
///
/// # Errors
///
/// Returns an error if the source cannot be read, if the destination cannot be
/// written, or if a WAD cannot be rebased - a chunk it does not hold, a
/// subchunked body, a version older than v3.4, or a size past the format's
/// 4 GiB limit. Every refusal but a write failure comes before `dest` is
/// touched, so a caller falling back to a full repack inherits an untouched
/// archive.
pub fn apply_delta(
    source: &Utf8Path,
    dest: &Utf8Path,
    delta: &ArchiveDelta<'_>,
    mut progress: Option<&mut dyn FnMut(DeltaProgress<'_>)>,
) -> Result<DeltaReport, FantomeDeltaError> {
    let file = File::open(source.as_std_path()).at(source)?;
    let mut reader = FantomeReader::new(BufReader::new(file))?;

    let parent = match dest.parent() {
        Some(parent) if !parent.as_str().is_empty() => parent,
        _ => Utf8Path::new("."),
    };
    fs::create_dir_all(parent.as_std_path()).at(parent)?;

    let plan = ArchivePlan::build(&mut reader, delta)?;
    let total = plan.step_count();

    // The rebases run before the destination is created: their scratch files
    // are the only bytes a refusal here has written, and they go with the
    // error.
    let mut rebased: Vec<(String, File)> = Vec::with_capacity(plan.wads.len());
    let mut chunks_replaced = 0;
    let mut chunks_removed = 0;
    for (index, wad) in plan.wads.iter().enumerate() {
        report(
            &mut progress,
            DeltaProgress {
                name: &wad.name,
                step: DeltaStep::RebaseWad,
                index: index as u32,
                total,
            },
        );
        let (scratch, removed) = rebase_wad(&mut reader, wad, parent)?;
        rebased.push((wad.entry_name.clone(), scratch));
        chunks_replaced += wad.chunks.len();
        chunks_removed += removed;
    }

    let mut temp = tempfile::NamedTempFile::new_in(parent.as_std_path()).at(parent)?;
    let entries_replaced = write_archive(
        &mut reader,
        temp.as_file_mut(),
        delta,
        &plan,
        rebased,
        &mut progress,
    )?;
    drop(reader);

    temp.persist(dest.as_std_path()).at(dest)?;

    Ok(DeltaReport {
        wads_rebased: plan.wads.len(),
        chunks_replaced,
        chunks_removed,
        entries_replaced,
        entries_removed: plan.entries_removed(),
    })
}

fn report(progress: &mut Option<&mut dyn FnMut(DeltaProgress<'_>)>, step: DeltaProgress<'_>) {
    if let Some(callback) = progress.as_mut() {
        callback(step);
    }
}

/// One packed WAD a replace will rebase.
struct PlannedWad<'a> {
    /// The WAD's name as the caller spelled it, for error messages.
    name: String,
    /// The archive entry holding it, as the archive spells it.
    entry_name: String,
    chunks: &'a BTreeMap<WadHash, Cow<'a, [u8]>>,
    removed: &'a BTreeSet<WadHash>,
}

/// One entry the source archive holds, and what the rewrite does with it.
struct SourceEntry {
    /// The entry's name, as the archive spells it.
    name: String,
    /// Whether it is a packed WAD.
    packed: bool,
    /// Whether the delta drops it.
    removed: bool,
}

/// What the rewrite will emit, decided before anything is written.
struct ArchivePlan<'a> {
    wads: Vec<PlannedWad<'a>>,
    /// Every entry the archive holds, dropped ones included.
    ///
    /// Indexed by the archive's own entry index, which is what the raw copies
    /// look each entry up by, so this cannot drift from what it names.
    source_entries: Vec<SourceEntry>,
    /// Delta entries the archive does not hold, each flagged as a packed WAD.
    added_entries: Vec<(String, bool)>,
}

impl<'a> ArchivePlan<'a> {
    /// Resolve every WAD and entry the delta names against the archive.
    ///
    /// Only entry names are read, so planning costs the archive no
    /// decompression.
    fn build<R: Read + Seek>(
        reader: &mut FantomeReader<R>,
        delta: &'a ArchiveDelta<'a>,
    ) -> Result<Self, FantomeDeltaError> {
        // Walked by index rather than through `entry_names`, because the copy
        // pass addresses each entry by index and nothing promises the name
        // iteration is in that order.
        let mut source_entries: Vec<SourceEntry> = Vec::with_capacity(reader.entry_count());
        for index in 0..reader.entry_count() {
            let name = reader
                .zip_archive_mut()
                .by_index_raw(index)
                .map_err(FantomeExtractError::from)?
                .name()
                .to_owned();
            let packed = is_packed_wad(&name);
            let removed = delta.removed_entries.contains(&name.to_ascii_lowercase());
            source_entries.push(SourceEntry {
                name,
                packed,
                removed,
            });
        }

        let mut wads = Vec::with_capacity(delta.chunks.len());
        for (key, wad) in &delta.chunks {
            let entry_name = source_entries
                .iter()
                .find(|entry| match classify_entry(&entry.name) {
                    Some(FantomeEntry::PackedWad(packed)) => packed.eq_ignore_ascii_case(key),
                    _ => false,
                })
                .map(|entry| entry.name.clone())
                .ok_or_else(|| FantomeDeltaError::WadNotPacked {
                    wad: wad.name.clone(),
                })?;

            // A WAD given whole and by its chunks would need one of the two
            // dropped, and neither answer is the caller's stated intent.
            // Dropping it whole while editing its chunks asks the same
            // question.
            let entry_key = entry_name.to_ascii_lowercase();
            if delta.entries.contains_key(&entry_key) || delta.removed_entries.contains(&entry_key)
            {
                return Err(FantomeDeltaError::ConflictingWad { wad: entry_name });
            }

            wads.push(PlannedWad {
                name: wad.name.clone(),
                entry_name,
                chunks: &wad.chunks,
                removed: &wad.removed,
            });
        }

        let added_entries = delta
            .entries
            .iter()
            .filter(|(key, _)| {
                !source_entries
                    .iter()
                    .any(|entry| entry.name.eq_ignore_ascii_case(key))
            })
            .map(|(_, (name, _))| (name.clone(), is_packed_wad(name)))
            .collect();

        Ok(Self {
            wads,
            source_entries,
            added_entries,
        })
    }

    /// How many steps the replace reports, rebases and entries together.
    fn step_count(&self) -> u32 {
        let steps = self.wads.len() + self.written_entry_count() + self.added_entries.len();
        u32::try_from(steps).unwrap_or(u32::MAX)
    }

    /// How many of the archive's own entries the rewrite carries over.
    fn written_entry_count(&self) -> usize {
        self.source_entries
            .iter()
            .filter(|entry| !entry.removed)
            .count()
    }

    /// How many of the archive's own entries the delta drops.
    fn entries_removed(&self) -> usize {
        self.source_entries.len() - self.written_entry_count()
    }
}

/// Rebase one packed WAD into a scratch file, positioned at its first byte,
/// and report how many of its chunks the delta dropped.
///
/// Every check a rebase can make without a target is made before a byte of the
/// scratch file is written, and the scratch file goes with the error either
/// way, so a refusal leaves the destination archive untouched.
fn rebase_wad<R: Read + Seek>(
    reader: &mut FantomeReader<R>,
    wad: &PlannedWad<'_>,
    scratch_dir: &Utf8Path,
) -> Result<(File, usize), FantomeDeltaError> {
    let mut source =
        reader
            .packed_wad_source(&wad.name)?
            .ok_or_else(|| FantomeDeltaError::WadNotPacked {
                wad: wad.name.clone(),
            })?;

    // Read off the bytes rather than off the mount: `Wad` mounts a v3.1 TOC as
    // readily as a v3.4 one and reports neither, and only v3.4 entries are the
    // shape a rebase writes back.
    let mut version = [0u8; 4];
    source
        .read_exact(&mut version)
        .map_err(FantomeDeltaError::read)?;
    if version[..2] != WAD_MAGIC || (version[2], version[3]) != REBASABLE_WAD_VERSION {
        return Err(FantomeDeltaError::UnsupportedWadVersion {
            wad: wad.name.clone(),
            major: version[2],
            minor: version[3],
        });
    }
    source
        .seek(SeekFrom::Start(0))
        .map_err(FantomeDeltaError::read)?;

    let mounted = Wad::mount(source).map_err(FantomeExtractError::from)?;
    let chunks = mounted.chunks().as_slice();
    let rebase = RebaseLayout::of(&wad.name, chunks, wad.removed)?;
    let layout = rebase.target;
    let tail = encode_tail(&wad.name, chunks, wad.chunks)?;
    let base_entries = base_entries(&wad.name, &layout, chunks, wad.removed)?;
    let chunks_removed = chunks.len() - base_entries.len();

    let plan = WadRebasePlan::tail(&layout, base_entries, &tail).map_err(|source| {
        FantomeDeltaError::Rebase {
            wad: wad.name.clone(),
            source,
        }
    })?;

    let mut scratch = tempfile::tempfile_in(scratch_dir.as_std_path()).at(scratch_dir)?;
    let (mut bytes, _) = mounted.into_parts();

    // The header alone, because everything above it - the chunk count, the TOC
    // and the tail - is what the rebase writes.
    copy_region(&mut bytes, 0, WAD_HEADER_SIZE, &mut scratch, &wad.name)?;
    // Then the chunks the WAD keeps, moved down by whatever a removal freed.
    // The bytes between the two are the shortened TOC's, which the rebase fills
    // to the byte.
    scratch
        .seek(SeekFrom::Start(layout.data_region_offset))
        .at(scratch_dir)?;
    copy_region(
        &mut bytes,
        rebase.source_region,
        layout.tail_offset - layout.data_region_offset,
        &mut scratch,
        &wad.name,
    )?;

    plan.write(&mut scratch, 0)
        .map_err(|source| FantomeDeltaError::Rebase {
            wad: wad.name.clone(),
            source,
        })?;
    scratch.seek(SeekFrom::Start(0)).at(scratch_dir)?;

    Ok((scratch, chunks_removed))
}

/// Copy `len` bytes of `source` from `from` into `dest` where it stands.
///
/// # Errors
///
/// Fails when the source refuses a seek, or runs out before `len` bytes.
fn copy_region(
    mut source: impl Read + Seek,
    from: u64,
    len: u64,
    dest: &mut impl Write,
    wad_name: &str,
) -> Result<(), FantomeDeltaError> {
    source
        .seek(SeekFrom::Start(from))
        .map_err(FantomeDeltaError::read)?;
    let copied = io::copy(&mut source.take(len), dest).map_err(FantomeDeltaError::read)?;
    if copied != len {
        return Err(FantomeDeltaError::read(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("{wad_name} is truncated: {copied} of the {len} bytes at {from}"),
        )));
    }
    Ok(())
}

/// A packed WAD's own geometry, and the layout the rewrite gives it.
///
/// Two records rather than one because a removal makes them differ: the copy
/// reads the kept chunks at the source's region offset and writes them at the
/// shorter TOC's.
struct RebaseLayout {
    /// Where the source WAD's data region starts.
    source_region: u64,
    /// Where the rewrite puts every region, and how far a kept chunk moves.
    target: WadTailLayout,
}

impl RebaseLayout {
    /// Where `chunks` put a WAD's regions, and where dropping `removed` puts
    /// them.
    ///
    /// The target's `offset_delta` is the 32 bytes per TOC entry a removal
    /// frees, and is zero where nothing is removed. Both region offsets come
    /// from the header and an entry count rather than from the first chunk.
    ///
    /// # Errors
    ///
    /// Reports a WAD holding more chunks than the format's `u32` count, and a
    /// chunk starting inside the WAD's own TOC.
    fn of(
        wad_name: &str,
        chunks: &[WadChunk],
        removed: &BTreeSet<WadHash>,
    ) -> Result<Self, FantomeDeltaError> {
        let source_capacity = toc_capacity(wad_name, chunks.len())?;
        let dropped = chunks
            .iter()
            .filter(|chunk| removed.contains(&chunk.path_hash))
            .count();
        let kept_capacity = toc_capacity(wad_name, chunks.len() - dropped)?;

        let source_region = region_offset(source_capacity);
        let data_region_offset = region_offset(kept_capacity);
        let offset_delta = data_region_offset as i64 - source_region as i64;

        let mut tail_offset = source_region;
        for chunk in chunks {
            let start = chunk.data_offset as u64;
            if start < source_region {
                return Err(FantomeDeltaError::ChunkInsideToc {
                    wad: wad_name.to_owned(),
                    path_hash: chunk.path_hash,
                    data_offset: start,
                });
            }
            if removed.contains(&chunk.path_hash) {
                continue;
            }
            tail_offset = tail_offset.max(start + chunk.compressed_size as u64);
        }

        Ok(Self {
            source_region,
            target: WadTailLayout {
                data_region_offset,
                offset_delta,
                tail_offset: tail_offset.saturating_add_signed(offset_delta),
                toc_capacity: kept_capacity,
            },
        })
    }
}

/// The TOC capacity `entry_count` entries need.
///
/// # Errors
///
/// Fails when the count is past what the format's `u32` chunk count holds.
fn toc_capacity(wad_name: &str, entry_count: usize) -> Result<u32, FantomeDeltaError> {
    u32::try_from(entry_count).map_err(|_| FantomeDeltaError::Rebase {
        wad: wad_name.to_owned(),
        source: WadRebaseError::TocCapacity {
            needed: entry_count,
            reserved: u32::MAX,
        },
    })
}

/// Where the data region of a v3.4 WAD reserving `toc_capacity` entries starts.
const fn region_offset(toc_capacity: u32) -> u64 {
    WAD_HEADER_SIZE + size_of::<u32>() as u64 + toc_capacity as u64 * TOC_ENTRY_SIZE
}

/// The TOC entry every chunk the WAD keeps carries into the rewrite.
///
/// Only the offset moves, and only by what a removal freed - subchunked bodies
/// and their frame fields carry over untouched, and nothing removed makes the
/// shift the identity, so each entry is then the source's own byte for byte.
/// The rebase then overwrites the entries of the chunks it actually appends.
fn base_entries(
    wad_name: &str,
    layout: &WadTailLayout,
    chunks: &[WadChunk],
    removed: &BTreeSet<WadHash>,
) -> Result<BTreeMap<WadHash, WadChunk>, FantomeDeltaError> {
    chunks
        .iter()
        .filter(|chunk| !removed.contains(&chunk.path_hash))
        .map(|chunk| {
            let shifted = layout
                .shifted(chunk)
                .map_err(|source| FantomeDeltaError::Rebase {
                    wad: wad_name.to_owned(),
                    source,
                })?;
            Ok((chunk.path_hash, shifted))
        })
        .collect()
}

/// Encode each of the delta's new chunk bodies against the chunk it replaces.
///
/// The chunk being replaced decides only whether the replacement is allowed -
/// it must exist, and it must not be a subchunked body, which no rebase can
/// rewrite. What the replacement is *stored* as comes from its own bytes; see
/// [`codec_for`].
fn encode_tail(
    wad_name: &str,
    chunks: &[WadChunk],
    bodies: &BTreeMap<WadHash, Cow<'_, [u8]>>,
) -> Result<Vec<(WadHash, EncodedChunk)>, FantomeDeltaError> {
    let mut tail = Vec::with_capacity(bodies.len());
    for (&path_hash, bytes) in bodies {
        let source = chunks
            .iter()
            .find(|chunk| chunk.path_hash == path_hash)
            .ok_or_else(|| FantomeDeltaError::ChunkAbsent {
                wad: wad_name.to_owned(),
                path_hash,
            })?;
        if source.compression_type == WadChunkCompression::ZstdMulti {
            return Err(FantomeDeltaError::SubchunkedChunk {
                wad: wad_name.to_owned(),
                path_hash,
            });
        }

        let uncompressed_size =
            u32::try_from(bytes.len()).map_err(|_| FantomeDeltaError::ChunkTooLarge {
                wad: wad_name.to_owned(),
                path_hash,
                len: bytes.len(),
            })?;
        let codec = codec_for(bytes);
        let compressed = encode_chunk(bytes, codec).map_err(FantomeDeltaError::read)?;
        tail.push((
            path_hash,
            EncodedChunk::new(compressed, uncompressed_size, codec),
        ));
    }

    Ok(tail)
}

/// Bytes of a replacement that have to be read before its type can be named.
///
/// The longest magic `ltk_file` matches on, and the shortest run its patterns
/// are safe to be handed - see [`codec_for`].
const MAGIC_BYTES: std::ops::RangeInclusive<usize> = 4..=ltk_file::MAX_MAGIC_SIZE;

/// The codec a replacement's own bytes ask to be stored under.
///
/// Read off the content rather than off the chunk being replaced, because the
/// content is the one thing every WAD sharing a chunk agrees on. League
/// validates a chunk that appears in several WADs by its compressed checksum
/// and kills the process when two copies disagree, and two WADs may perfectly
/// well hold one hash under different codecs - so deriving the codec from the
/// source chunk would write a raw body into one and a Zstd frame into the other
/// for a single repair. Deriving it from the bytes makes them agree by
/// construction.
///
/// The policy itself is [`FileExt::ideal_compression`]'s - audio stored,
/// because it is already compressed, and everything else Zstd - which is what
/// [`ltk_wad::WadBuilder`] and the overlay builder apply to the same content.
///
/// Only the head is identified, which is all the magics reach. A body too short
/// for any of them is called unknown rather than handed over, because
/// `ltk_file` 0.2.11 panics on a buffer of exactly three bytes: its JPEG
/// pattern declares a minimum length of three and then reads four. That changes
/// no answer, since the only two kinds that are not Zstd are named by magics of
/// four and eight bytes.
fn codec_for(bytes: &[u8]) -> WadChunkCompression {
    let head = &bytes[..bytes.len().min(*MAGIC_BYTES.end())];
    match head.len() >= *MAGIC_BYTES.start() {
        true => LeagueFileKind::identify_from_bytes(head).ideal_compression(),
        false => WadChunkCompression::Zstd,
    }
}

/// Compress `data` under `codec`.
///
/// Total over the two codecs [`codec_for`] chooses between, so there is no
/// unsupported case to report.
fn encode_chunk(data: &[u8], codec: WadChunkCompression) -> io::Result<Vec<u8>> {
    match codec {
        WadChunkCompression::None => Ok(data.to_vec()),
        _ => {
            let mut out = Vec::new();
            let mut encoder = zstd::Encoder::new(&mut out, ZSTD_LEVEL)?;
            encoder.write_all(data)?;
            encoder.finish()?;
            Ok(out)
        }
    }
}

/// Stream the archive into `sink`, packed WADs last, and report how many
/// entries were replaced or added.
fn write_archive<R: Read + Seek, W: Write + Seek>(
    reader: &mut FantomeReader<R>,
    sink: W,
    delta: &ArchiveDelta<'_>,
    plan: &ArchivePlan<'_>,
    rebased: Vec<(String, File)>,
    progress: &mut Option<&mut dyn FnMut(DeltaProgress<'_>)>,
) -> Result<usize, FantomeDeltaError> {
    let mut writer = FantomeWriter::new(sink);
    let mut rebased: BTreeMap<String, File> = rebased.into_iter().collect();
    let mut entries_replaced = 0;
    let mut index = u32::try_from(plan.wads.len()).unwrap_or(u32::MAX);
    let total = plan.step_count();

    // Loose entries first and packed WADs after them, so an edit that grows a
    // WAD moves only the central directory and the WAD's own bytes.
    for packed_wads in [false, true] {
        for (source_index, entry) in plan.source_entries.iter().enumerate() {
            if entry.removed || entry.packed != packed_wads {
                continue;
            }
            let name = &entry.name;
            report(
                progress,
                DeltaProgress {
                    name,
                    step: DeltaStep::WriteEntry,
                    index,
                    total,
                },
            );
            index = index.saturating_add(1);

            if let Some(mut scratch) = rebased.remove(name) {
                writer.write_entry(name, &mut scratch)?;
                continue;
            }
            match delta.entries.get(&name.to_ascii_lowercase()) {
                Some((_, bytes)) => {
                    writer.write_entry(name, &mut &bytes[..])?;
                    entries_replaced += 1;
                }
                None => {
                    let source = reader
                        .zip_archive_mut()
                        .by_index_raw(source_index)
                        .map_err(FantomeExtractError::from)?;
                    writer
                        .zip_mut()
                        .raw_copy_file(source)
                        .map_err(FantomeWriteError::from)?;
                }
            }
        }

        for (name, is_packed) in &plan.added_entries {
            if *is_packed != packed_wads {
                continue;
            }
            report(
                progress,
                DeltaProgress {
                    name,
                    step: DeltaStep::WriteEntry,
                    index,
                    total,
                },
            );
            index = index.saturating_add(1);

            let (_, bytes) = &delta.entries[&name.to_ascii_lowercase()];
            writer.write_entry(name, &mut &bytes[..])?;
            entries_replaced += 1;
        }
    }

    writer.finish()?;
    Ok(entries_replaced)
}

#[cfg(test)]
mod tests;
