//! The term index: an object index with the terms every declaration holds.

use std::io::Cursor;

use camino::Utf8Path;
use ltk_meta::BinOverride;
use ltk_meta::stream::BinStream;
use serde::{Deserialize, Serialize};

use super::bytes;
use super::intern::{self, Interner};
use super::sheet::Sheet;
use super::terms::{TermKind, TermKinds, TermSink, TermVisitor, collect_terms};
use crate::objects::build::{self, ChunkAt, ChunkRead, JobRead, read_declarations};
use crate::objects::{Body as ObjectBody, BodyRef as ObjectBodyRef};
use crate::{
    BuildOptions, CacheError, ChunkKind, DeclarationId, Fingerprint, GameIndex, ObjectBuildError,
    ObjectIndex, cache,
};

/// The cache format version of the term index.
///
/// Bumped on any change to [`TermStats`], the postings, [`Sheet`], the object index body, or
/// the container layout.
pub const TERM_CACHE_FORMAT_VERSION: u32 = 1;

/// Counts of one term build, beyond the object build's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TermStats {
    /// Distinct `(kind, hash)` keys.
    pub hash_terms: u32,
    /// Distinct string terms.
    pub string_terms: u32,
    /// Bytes of the strings sheet.
    pub string_bytes: u64,
    /// Bin chunks whose declarations read and whose values did not decode.
    pub failed_chunks: u32,
}

/// The postings of every hash term, keyed by `(kind, hash)`.
///
/// Keys sort by kind, then hash. `kinds[k]..kinds[k + 1]` is the run of kind `k` in `hashes`.
/// `offsets` holds one more entry than `hashes`. A key's posting is
/// `ids[offsets[i]..offsets[i + 1]]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct HashPostings {
    kinds: [u32; 9],
    #[serde(with = "bytes")]
    hashes: Vec<u64>,
    #[serde(with = "bytes")]
    offsets: Vec<u32>,
    #[serde(with = "bytes")]
    ids: Vec<DeclarationId>,
}

impl HashPostings {
    /// The postings of `(kind, hash, id)` triples, sorted and deduplicated.
    fn from_sorted(triples: &[(u8, u64, DeclarationId)]) -> Self {
        let mut postings = Self {
            ids: Vec::with_capacity(triples.len()),
            ..Self::default()
        };
        let mut last: Option<(u8, u64)> = None;
        for &(kind, hash, id) in triples {
            if last != Some((kind, hash)) {
                postings.hashes.push(hash);
                postings.offsets.push(postings.ids.len() as u32);
                for start in &mut postings.kinds[kind as usize + 1..] {
                    *start = postings.hashes.len() as u32;
                }
                last = Some((kind, hash));
            }
            postings.ids.push(id);
        }
        postings.offsets.push(postings.ids.len() as u32);
        postings
    }

    fn get(&self, kind: TermKind, hash: u64) -> &[DeclarationId] {
        self.key(kind, hash).map_or(&[], |key| self.posting(key))
    }

    /// The position of key `(kind, hash)`, when a declaration holds it.
    fn key(&self, kind: TermKind, hash: u64) -> Option<u32> {
        let kind = kind.ordinal();
        let run = self.kinds[kind] as usize..self.kinds[kind + 1] as usize;
        let at = self.hashes.get(run.clone())?.binary_search(&hash).ok()?;
        Some((run.start + at) as u32)
    }

    /// The posting of the key at `key`.
    fn posting(&self, key: u32) -> &[DeclarationId] {
        let key = key as usize;
        &self.ids[self.offsets[key] as usize..self.offsets[key + 1] as usize]
    }

    fn len(&self) -> usize {
        self.hashes.len()
    }

    fn is_consistent(&self) -> bool {
        self.offsets.len() == self.hashes.len() + 1
            && self.kinds.is_sorted()
            && self.kinds[8] as usize == self.hashes.len()
            && self.offsets.is_sorted()
            && self
                .offsets
                .last()
                .is_some_and(|&end| end as usize == self.ids.len())
    }
}

/// The postings of every string term, keyed by term id. Every id has a posting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct StringPostings {
    #[serde(with = "bytes")]
    offsets: Vec<u32>,
    #[serde(with = "bytes")]
    ids: Vec<DeclarationId>,
}

impl StringPostings {
    /// The postings of `(term, id)` pairs, sorted and deduplicated, over `terms` term ids.
    fn from_sorted(pairs: &[(u32, DeclarationId)], terms: usize) -> Self {
        let mut postings = Self {
            offsets: Vec::with_capacity(terms + 1),
            ids: Vec::with_capacity(pairs.len()),
        };
        let mut pairs = pairs.iter().peekable();
        for term in 0..terms as u32 {
            postings.offsets.push(postings.ids.len() as u32);
            while let Some(&(_, id)) = pairs.next_if(|(at, _)| *at == term) {
                postings.ids.push(id);
            }
        }
        postings.offsets.push(postings.ids.len() as u32);
        postings
    }

    fn get(&self, term: u32) -> &[DeclarationId] {
        let term = term as usize;
        match (self.offsets.get(term), self.offsets.get(term + 1)) {
            (Some(&start), Some(&end)) => &self.ids[start as usize..end as usize],
            _ => &[],
        }
    }

    fn is_consistent(&self) -> bool {
        self.offsets.is_sorted()
            && self
                .offsets
                .last()
                .is_some_and(|&end| end as usize == self.ids.len())
    }
}

/// Every declaration of an installation with the terms each one holds.
///
/// The object index is complete: a term index needs no second object build. Its ids are the
/// ids of an object build over the same chunk index with the same resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermIndex {
    objects: ObjectIndex,
    hashes: HashPostings,
    strings: StringPostings,
    sheet: Sheet,
    stats: TermStats,
}

/// The cached body of a term index.
#[derive(Deserialize)]
struct Body {
    objects: ObjectBody,
    hashes: HashPostings,
    strings: StringPostings,
    sheet: Sheet,
    stats: TermStats,
}

/// [`Body`] borrowed from an index, for writing.
#[derive(Serialize)]
struct BodyRef<'a> {
    objects: ObjectBodyRef<'a>,
    hashes: &'a HashPostings,
    strings: &'a StringPostings,
    sheet: &'a Sheet,
    stats: &'a TermStats,
}

impl TermIndex {
    /// Reads every bin chunk of `game` with default options: no resolver, every chunk sniffed.
    ///
    /// # Errors
    ///
    /// Never, with default options. The signature matches [`build_with`](Self::build_with).
    pub fn build(game: &GameIndex) -> Result<Self, ObjectBuildError> {
        Self::build_with(game, &BuildOptions::default())
    }

    /// Reads every bin chunk of `game` in full for its declarations and terms.
    ///
    /// The partition, the read-sniff-skip policy, `workers`, `called_off`, `on_archive` and
    /// skipped archives are [`ObjectIndex::build_with`]'s.
    ///
    /// # Errors
    ///
    /// [`ObjectBuildError::CalledOff`] when `called_off` returns `true` before a job.
    pub fn build_with(
        game: &GameIndex,
        options: &BuildOptions<'_>,
    ) -> Result<Self, ObjectBuildError> {
        let built = build::build(game, options, &Terms)?;
        let objects = ObjectIndex::from_parts(
            built.declarations,
            built.stats,
            built.skipped,
            game.fingerprint(),
        );
        Ok(merge(objects, built.outs))
    }

    /// Reads the cache at `cache_path` for the installation `game` was built from.
    ///
    /// # Errors
    ///
    /// [`CacheError::Read`], [`CacheError::Decode`], [`CacheError::Version`] for a format
    /// version other than [`TERM_CACHE_FORMAT_VERSION`], [`CacheError::Stale`] when the
    /// cached fingerprint differs from `game.fingerprint()`, and [`CacheError::Inconsistent`]
    /// when the postings disagree with the declarations or the strings sheet.
    pub fn load_for(cache_path: &Utf8Path, game: &GameIndex) -> Result<Self, CacheError> {
        let cache::Document { fingerprint, body } =
            cache::read::<Body>(cache_path, TERM_CACHE_FORMAT_VERSION)?;
        if fingerprint != game.fingerprint() {
            return Err(CacheError::Stale {
                path: cache_path.to_path_buf(),
                cached: fingerprint,
                current: game.fingerprint(),
            });
        }
        let objects = ObjectIndex::from_body(body.objects, fingerprint);
        let declarations = objects.rows().len();
        let in_range = |ids: &[DeclarationId]| ids.iter().all(|id| id.index() < declarations);
        if !body.hashes.is_consistent()
            || !body.strings.is_consistent()
            || body.strings.offsets.len() != body.sheet.len() + 1
            || !in_range(&body.hashes.ids)
            || !in_range(&body.strings.ids)
        {
            return Err(CacheError::Inconsistent {
                path: cache_path.to_path_buf(),
            });
        }
        Ok(Self {
            objects,
            hashes: body.hashes,
            strings: body.strings,
            sheet: body.sheet,
            stats: body.stats,
        })
    }

    /// Writes the index to `cache_path` atomically.
    ///
    /// # Errors
    ///
    /// [`CacheError::Write`] and [`CacheError::Encode`].
    pub fn save(&self, cache_path: &Utf8Path) -> Result<(), CacheError> {
        let body = BodyRef {
            objects: self.objects.body(),
            hashes: &self.hashes,
            strings: &self.strings,
            sheet: &self.sheet,
            stats: &self.stats,
        };
        cache::write(
            cache_path,
            TERM_CACHE_FORMAT_VERSION,
            self.fingerprint(),
            &body,
        )?;
        tracing::debug!("Term index cache saved to {cache_path}");
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
            "term index",
            cache_path,
            || Self::load_for(cache_path, game),
            || Self::build_with(game, options),
            |index| index.save(cache_path),
        )
    }

    /// The object index the build produced.
    #[must_use]
    pub fn objects(&self) -> &ObjectIndex {
        &self.objects
    }

    /// The declarations holding hash term `(kind, hash)`, ascending. Empty for
    /// [`TermKind::String`].
    #[must_use]
    pub fn hash_postings(&self, kind: TermKind, hash: u64) -> &[DeclarationId] {
        self.hashes.get(kind, hash)
    }

    /// The position of hash term `(kind, hash)` among the hash postings, when a declaration
    /// holds it.
    pub(super) fn hash_key(&self, kind: TermKind, hash: u64) -> Option<u32> {
        self.hashes.key(kind, hash)
    }

    /// The posting at `key`, a position [`hash_key`](Self::hash_key) gave.
    pub(super) fn hash_posting(&self, key: u32) -> &[DeclarationId] {
        self.hashes.posting(key)
    }

    /// The union of the postings of `hash` under every hash kind in `kinds`, ascending.
    #[must_use]
    pub fn postings_in(&self, kinds: TermKinds, hash: u64) -> Vec<DeclarationId> {
        let mut ids: Vec<DeclarationId> = kinds
            .iter()
            .flat_map(|kind| self.hash_postings(kind, hash))
            .copied()
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The declarations holding string term `term`, ascending.
    #[must_use]
    pub fn string_postings(&self, term: u32) -> &[DeclarationId] {
        self.strings.get(term)
    }

    /// Every string term, lowercased. An entry's value and index are its term id.
    #[must_use]
    pub fn strings(&self) -> &Sheet {
        &self.sheet
    }

    /// Counts of the build that produced the index.
    #[must_use]
    pub fn stats(&self) -> &TermStats {
        &self.stats
    }

    /// The fingerprint of the chunk index the term index was built from.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        self.objects.fingerprint()
    }
}

/// The full-decode read of the term build.
struct Terms;

/// One job's terms. Declaration ids and string ids are local to the job.
#[derive(Debug, Default)]
struct JobTerms {
    /// `(kind, hash, declaration)`, each declaration's terms once.
    hashes: Vec<(u8, u64, u32)>,
    /// `(string, declaration)`, each declaration's strings once.
    strings: Vec<(u32, u32)>,
    /// Every lowercased string of the job, in first-seen order.
    interned: Interner,
    failed_chunks: u32,
    /// The hash terms of the object being read, before deduplication.
    object_hashes: Vec<(u8, u64)>,
    /// The string terms of the object being read, before deduplication.
    object_strings: Vec<u32>,
    lowered: String,
}

impl JobTerms {
    fn intern(&mut self, lowered: &str) -> u32 {
        self.interned.intern(lowered, intern::hash(lowered))
    }

    /// Drops everything after the given lengths.
    fn truncate(&mut self, hashes: usize, strings: usize, interned: usize) {
        self.hashes.truncate(hashes);
        self.strings.truncate(strings);
        self.interned.truncate(interned);
    }

    /// Moves the object's terms into the job, each once, under declaration `id`.
    fn commit_object(&mut self, id: u32) {
        self.object_hashes.sort_unstable();
        self.object_hashes.dedup();
        self.object_strings.sort_unstable();
        self.object_strings.dedup();
        self.hashes.extend(
            self.object_hashes
                .drain(..)
                .map(|(kind, hash)| (kind, hash, id)),
        );
        self.strings
            .extend(self.object_strings.drain(..).map(|term| (term, id)));
    }
}

impl TermSink for JobTerms {
    fn hash(&mut self, kind: TermKind, hash: u64) {
        self.object_hashes.push((kind as u8, hash));
    }

    fn string(&mut self, value: &str) {
        if value.is_empty() || value.contains('\n') {
            return;
        }
        let mut lowered = std::mem::take(&mut self.lowered);
        lowered.clear();
        lowered.push_str(value);
        lowered.make_ascii_lowercase();
        let id = self.intern(&lowered);
        self.object_strings.push(id);
        self.lowered = lowered;
    }
}

impl ChunkRead for Terms {
    type Out = JobTerms;

    fn read(&self, bytes: &[u8], at: ChunkAt<'_>, read: &mut JobRead<JobTerms>) {
        let declarations = read.declarations.len();
        let (hashes, strings, interned) = (
            read.out.hashes.len(),
            read.out.strings.len(),
            read.out.interned.len(),
        );
        let outcome = match at.kind {
            ChunkKind::Prop => read_prop(bytes, at, read),
            ChunkKind::Patch => read_patch(bytes, at, read),
        };
        if let Err(e) = outcome {
            read.declarations.truncate(declarations);
            read.out.truncate(hashes, strings, interned);
            read.out.object_hashes.clear();
            read.out.object_strings.clear();
            let skipped = read.skipped_chunks;
            read_declarations(bytes, at, read);
            if read.skipped_chunks == skipped {
                read.out.failed_chunks += 1;
                tracing::debug!(
                    "Terms of {}/{:016x} not read: {e}",
                    at.archive_name,
                    at.chunk.0
                );
            }
        }
    }
}

/// Walks every object of a `PROP` over its streamed bytes. No object is materialized.
fn read_prop(
    bytes: &[u8],
    at: ChunkAt<'_>,
    read: &mut JobRead<JobTerms>,
) -> Result<(), ltk_meta::Error> {
    let mut stream = BinStream::mount(Cursor::new(bytes))?;
    let mut objects = stream.objects();
    while let Some(mut object) = objects.next()? {
        let id = read.declarations.len() as u32;
        read.declarations
            .push(at.declaration(object.path_hash(), object.class_hash()));
        object.walk(&mut TermVisitor {
            sink: &mut read.out,
        })?;
        read.out.commit_object(id);
    }
    Ok(())
}

/// Walks every object a `PTCH` embeds.
fn read_patch(
    bytes: &[u8],
    at: ChunkAt<'_>,
    read: &mut JobRead<JobTerms>,
) -> Result<(), ltk_meta::Error> {
    let patch = BinOverride::from_reader(&mut Cursor::new(bytes))?;
    for object in patch.objects.values() {
        let id = read.declarations.len() as u32;
        read.declarations
            .push(at.declaration(object.path_hash, object.class_hash));
        collect_terms(object, &mut read.out)?;
        read.out.commit_object(id);
    }
    Ok(())
}

/// Assembles the term index from the jobs' terms, in archive id order.
///
/// The jobs' strings intern into one buffer, which becomes the strings sheet.
fn merge(objects: ObjectIndex, outs: Vec<(u32, JobTerms)>) -> TermIndex {
    let mut stats = TermStats::default();
    let mut hashes: Vec<(u8, u64, DeclarationId)> =
        Vec::with_capacity(outs.iter().map(|(_, job)| job.hashes.len()).sum());
    let mut strings: Vec<(u32, DeclarationId)> =
        Vec::with_capacity(outs.iter().map(|(_, job)| job.strings.len()).sum());
    let mut interned = Interner::default();

    for (base, job) in outs {
        stats.failed_chunks += job.failed_chunks;
        let terms: Vec<u32> = (0..job.interned.len() as u32)
            .map(|id| interned.intern(job.interned.get(id), job.interned.hash_of(id)))
            .collect();
        hashes.extend(
            job.hashes
                .into_iter()
                .map(|(kind, hash, id)| (kind, hash, DeclarationId::new(base + id))),
        );
        strings.extend(
            job.strings
                .into_iter()
                .map(|(term, id)| (terms[term as usize], DeclarationId::new(base + id))),
        );
    }
    let (text, starts) = interned.into_parts();
    let sheet = Sheet::from_lowercase(text, starts);

    sort(&mut hashes);
    sort(&mut strings);
    let hashes = HashPostings::from_sorted(&hashes);
    let strings = StringPostings::from_sorted(&strings, sheet.len());
    stats.hash_terms = hashes.len() as u32;
    stats.string_terms = sheet.len() as u32;
    stats.string_bytes = sheet.text().len() as u64;
    tracing::info!(
        hash_terms = stats.hash_terms,
        string_terms = stats.string_terms,
        string_bytes = stats.string_bytes,
        failed_chunks = stats.failed_chunks,
        "Built the term index"
    );
    TermIndex {
        objects,
        hashes,
        strings,
        sheet,
        stats,
    }
}

/// Sorts `items`, in parallel with the `rayon` feature. Each declaration's terms are unique,
/// and the pairs of distinct declarations differ: the sorted items hold no repeat.
fn sort<T: Ord + Send>(items: &mut [T]) {
    #[cfg(feature = "rayon")]
    {
        use rayon::slice::ParallelSliceMut;
        items.par_sort_unstable();
    }
    #[cfg(not(feature = "rayon"))]
    items.sort_unstable();
}
