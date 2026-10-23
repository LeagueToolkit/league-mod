//! The object index: every bin object of an installation with every declaring chunk.

pub(crate) mod build;
mod read;

use std::fmt;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::time::Duration;

use camino::Utf8Path;
use ltk_hash::BinHash;
use ltk_wad::WadHash;
use serde::{Deserialize, Serialize};

use crate::{ArchiveId, CacheError, Fingerprint, GameIndex, ResolveWadPath, SkippedArchive, cache};

pub use read::for_each_declaration;

/// The cache format version of the object index.
///
/// Bumped on any change to [`Declaration`], [`ObjectStats`], [`SkippedArchive`], or the
/// container layout.
pub const OBJECT_CACHE_FORMAT_VERSION: u32 = 2;

/// One object in one declaring chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Declaration {
    /// The object.
    pub object: BinHash,
    /// The class the chunk declares the object as.
    pub class: BinHash,
    /// The declaring chunk.
    pub chunk: WadHash,
    /// The copy of `chunk` the build read.
    pub archive: ArchiveId,
    /// Whether `chunk` is a `PROP` bin or a `PTCH` override.
    pub kind: ChunkKind,
}

/// The kind of bin a declaring chunk is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ChunkKind {
    /// A property bin, magic `PROP`.
    Prop,
    /// A property override, magic `PTCH`.
    Patch,
}

/// The ordinal of a declaration in [`ObjectIndex::rows`].
///
/// Ids are dense from zero and stable for one index and its cache. They are not stable across
/// builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeclarationId(u32);

impl DeclarationId {
    /// The id at storage ordinal `index`.
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// The storage ordinal, as an index into [`ObjectIndex::rows`].
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl From<DeclarationId> for u32 {
    fn from(id: DeclarationId) -> Self {
        id.0
    }
}

/// Counts of one object index build.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ObjectStats {
    /// Archives with at least one chunk read or sniffed.
    pub archives: u32,
    /// Bin chunks read, named or sniffed, readable or not.
    pub bins: u32,
    /// Chunks decoded to their magic.
    pub sniffed: u32,
    /// Sniffed chunks whose magic is a bin's.
    pub sniffed_bins: u32,
    /// Declarations stored.
    pub declarations: u32,
    /// Chunks that did not read.
    pub skipped_chunks: u32,
    /// Decompressed bytes read.
    pub bytes: u64,
    /// Wall-clock time of the build.
    pub elapsed: Duration,
    /// Threads the build ran on.
    pub workers: u32,
}

/// An object index build that did not finish.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ObjectBuildError {
    /// `called_off` returned `true` before a job.
    #[error("the object index build was called off")]
    CalledOff,
}

/// How an object index build runs.
#[derive(Default)]
#[non_exhaustive]
pub struct BuildOptions<'a> {
    /// Names chunks. `None` sniffs every chunk.
    pub resolver: Option<&'a dyn ResolveWadPath>,
    /// Archives read at once. `None` is the available parallelism.
    pub workers: Option<NonZeroUsize>,
    /// Polled before each archive. `true` stops the build.
    pub called_off: Option<&'a (dyn Fn() -> bool + Sync)>,
    /// Called with `(done, total)` after each archive's job, from the job's thread.
    pub on_archive: Option<&'a (dyn Fn(u32, u32) + Sync)>,
}

impl fmt::Debug for BuildOptions<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildOptions")
            .field("resolver", &self.resolver.is_some())
            .field("workers", &self.workers)
            .field("called_off", &self.called_off.is_some())
            .field("on_archive", &self.on_archive.is_some())
            .finish()
    }
}

/// The cached body of an object index. The lookups are rebuilt on load.
#[derive(Deserialize)]
pub(crate) struct Body {
    declarations: Vec<Declaration>,
    stats: ObjectStats,
    skipped: Vec<SkippedArchive>,
}

/// [`Body`] borrowed from an index, for writing.
#[derive(Serialize)]
pub(crate) struct BodyRef<'a> {
    declarations: &'a [Declaration],
    stats: &'a ObjectStats,
    skipped: &'a [SkippedArchive],
}

/// Every bin object of an installation, keyed by object hash, with every declaring chunk.
///
/// Declarations are stored in archive id order, and within one archive in the order the
/// build read its bin chunks: named `.bin` chunks in ascending chunk hash order, then
/// bare-named chunks, then unnamed chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectIndex {
    /// Storage order.
    declarations: Vec<Declaration>,
    /// Ids in storage order, stably sorted by object.
    by_object: Vec<u32>,
    /// Every object, ascending.
    objects: Vec<BinHash>,
    /// `(chunk, start, len)` into `declarations`, sorted by chunk.
    by_chunk: Vec<(WadHash, u32, u32)>,
    stats: ObjectStats,
    skipped: Vec<SkippedArchive>,
    fingerprint: Fingerprint,
}

impl ObjectIndex {
    pub(crate) fn from_parts(
        declarations: Vec<Declaration>,
        stats: ObjectStats,
        skipped: Vec<SkippedArchive>,
        fingerprint: Fingerprint,
    ) -> Self {
        let mut by_object: Vec<u32> = (0..declarations.len() as u32).collect();
        by_object.sort_by_key(|&id| declarations[id as usize].object);
        let mut objects: Vec<BinHash> = by_object
            .iter()
            .map(|&id| declarations[id as usize].object)
            .collect();
        objects.dedup();

        let mut by_chunk: Vec<(WadHash, u32, u32)> = Vec::new();
        for (start, run) in runs_by_chunk(&declarations) {
            by_chunk.push((run[0].chunk, start as u32, run.len() as u32));
        }
        by_chunk.sort_unstable_by_key(|&(chunk, _, _)| chunk);

        Self {
            declarations,
            by_object,
            objects,
            by_chunk,
            stats,
            skipped,
            fingerprint,
        }
    }

    /// Reads every bin chunk of `game` with default options: no resolver, every chunk sniffed.
    ///
    /// # Errors
    ///
    /// Never, with default options. The signature matches [`build_with`](Self::build_with).
    pub fn build(game: &GameIndex) -> Result<Self, ObjectBuildError> {
        Self::build_with(game, &BuildOptions::default())
    }

    /// Reads every bin chunk of `game` for what it declares.
    ///
    /// The build partitions the chunk index's rows by first holder, one job per archive. With a
    /// resolver, a named chunk whose path ends in `.bin` is read, a bare-named chunk is
    /// sniffed, and any other named chunk is not read. An unnamed chunk is sniffed. An archive
    /// that does not open or mount is a skipped archive.
    ///
    /// # Errors
    ///
    /// [`ObjectBuildError::CalledOff`] when `called_off` returns `true` before a job.
    pub fn build_with(
        game: &GameIndex,
        options: &BuildOptions<'_>,
    ) -> Result<Self, ObjectBuildError> {
        let built = build::build(game, options, &build::Declarations)?;
        Ok(Self::from_parts(
            built.declarations,
            built.stats,
            built.skipped,
            game.fingerprint(),
        ))
    }

    /// Reads the cache at `cache_path` for the installation `game` was built from.
    ///
    /// # Errors
    ///
    /// [`CacheError::Read`], [`CacheError::Decode`], [`CacheError::Version`] for a format
    /// version other than [`OBJECT_CACHE_FORMAT_VERSION`], and [`CacheError::Stale`] when the
    /// cached fingerprint differs from `game.fingerprint()`.
    pub fn load_for(cache_path: &Utf8Path, game: &GameIndex) -> Result<Self, CacheError> {
        let cache::Document { fingerprint, body } =
            cache::read::<Body>(cache_path, OBJECT_CACHE_FORMAT_VERSION)?;
        if fingerprint != game.fingerprint() {
            return Err(CacheError::Stale {
                path: cache_path.to_path_buf(),
                cached: fingerprint,
                current: game.fingerprint(),
            });
        }
        Ok(Self::from_body(body, fingerprint))
    }

    /// The index a cached body holds, for the installation `fingerprint` names.
    pub(crate) fn from_body(body: Body, fingerprint: Fingerprint) -> Self {
        Self::from_parts(body.declarations, body.stats, body.skipped, fingerprint)
    }

    /// The cached body of the index.
    pub(crate) fn body(&self) -> BodyRef<'_> {
        BodyRef {
            declarations: &self.declarations,
            stats: &self.stats,
            skipped: &self.skipped,
        }
    }

    /// Writes the index to `cache_path` atomically.
    ///
    /// # Errors
    ///
    /// [`CacheError::Write`] and [`CacheError::Encode`].
    pub fn save(&self, cache_path: &Utf8Path) -> Result<(), CacheError> {
        cache::write(
            cache_path,
            OBJECT_CACHE_FORMAT_VERSION,
            self.fingerprint,
            &self.body(),
        )?;
        tracing::debug!("Object index cache saved to {cache_path}");
        Ok(())
    }

    /// The cached index when it matches `game`, and a fresh build otherwise.
    ///
    /// A missing cache is logged at debug and any other cache error at warn. The built index
    /// is saved best-effort, with a warn on failure.
    ///
    /// # Errors
    ///
    /// Everything [`build_with`](Self::build_with) reports.
    pub fn load_or_build_with(
        game: &GameIndex,
        cache_path: &Utf8Path,
        options: &BuildOptions<'_>,
    ) -> Result<Self, ObjectBuildError> {
        cache::load_or_build(
            "object index",
            cache_path,
            || Self::load_for(cache_path, game),
            || Self::build_with(game, options),
            |index| index.save(cache_path),
        )
    }

    /// Whether any chunk declares `object`.
    #[must_use]
    pub fn declares(&self, object: BinHash) -> bool {
        self.objects.binary_search(&object).is_ok()
    }

    /// Every declaration of `object` with its id, in storage order.
    pub fn declarations(
        &self,
        object: BinHash,
    ) -> impl ExactSizeIterator<Item = (DeclarationId, &Declaration)> + '_ {
        let object_of = |id: &u32| self.declarations[*id as usize].object;
        let start = self.by_object.partition_point(|id| object_of(id) < object);
        let end = start + self.by_object[start..].partition_point(|id| object_of(id) == object);
        self.by_object[start..end]
            .iter()
            .map(|&id| (DeclarationId(id), &self.declarations[id as usize]))
    }

    /// Every declaration in `chunk` with its id, in storage order.
    pub fn chunk_declarations(
        &self,
        chunk: WadHash,
    ) -> impl ExactSizeIterator<Item = (DeclarationId, &Declaration)> + '_ {
        self.chunk_range(chunk)
            .map(|index| (DeclarationId(index as u32), &self.declarations[index]))
    }

    /// The storage range of the declarations in `chunk`. Empty for a chunk declaring nothing.
    fn chunk_range(&self, chunk: WadHash) -> Range<usize> {
        self.by_chunk
            .binary_search_by_key(&chunk, |&(chunk, _, _)| chunk)
            .ok()
            .map_or(0..0, |at| {
                let (_, start, len) = self.by_chunk[at];
                start as usize..(start + len) as usize
            })
    }

    /// Every declaration, in storage order. The position of a declaration is its id.
    #[must_use]
    pub fn rows(&self) -> &[Declaration] {
        &self.declarations
    }

    /// The declaration at `id`. `None` for an id past the end, from another index.
    #[must_use]
    pub fn row(&self, id: DeclarationId) -> Option<&Declaration> {
        self.declarations.get(id.index())
    }

    /// Every object, ascending.
    pub fn objects(&self) -> impl ExactSizeIterator<Item = BinHash> + '_ {
        self.objects.iter().copied()
    }

    /// The number of distinct objects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether no chunk declares an object.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Counts of the build that produced the index.
    #[must_use]
    pub fn stats(&self) -> &ObjectStats {
        &self.stats
    }

    /// Every archive the object build could not read, in id order.
    #[must_use]
    pub fn skipped(&self) -> &[SkippedArchive] {
        &self.skipped
    }

    /// The fingerprint of the chunk index the object index was built from.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }
}

/// Maximal runs of consecutive declarations sharing one chunk, with the start of each.
///
/// Each chunk is read once from one copy. Its declarations are contiguous in storage.
fn runs_by_chunk(declarations: &[Declaration]) -> impl Iterator<Item = (usize, &[Declaration])> {
    let mut start = 0;
    declarations
        .chunk_by(|a, b| a.chunk == b.chunk)
        .map(move |run| {
            let at = start;
            start += run.len();
            (at, run)
        })
}
