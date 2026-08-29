//! Between-passes distribution, Pass 2 resolve, and parallel WAD patching.
//!
//! Routes override hashes to affected WADs, partitions into rebuild/reuse sets,
//! re-reads bytes for WADs that need rebuilding, and patches WADs in parallel.

use crate::builder::incremental::TailRewrite;
use crate::builder::{
    ChecksumMismatch, OverlayBuilder, OverlayProgress, OverlayStage, OverrideMeta, OverrideSource,
    SharedBytes,
};
use crate::content::ModContentProvider;
use crate::error::{Error, Invariant, Result};
use crate::game_index::GameIndex;
use crate::state::OverlayState;
use crate::strings::{self, StringPatchPlan};
use crate::utils::{ContentHash, compute_wad_fingerprint_from_meta};
use crate::wad_builder::{PatchedWadStats, PreparedOverride, WadTailPlan, build_patched_wad};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_wad::WadHash;
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::OpenOptions;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Uncompressed override bytes as read in pass 2, before the
/// [`OverrideCompressor`] turns them into the compressed [`PreparedOverride`]s
/// the writer consumes.
pub(crate) type ResolvedChunk = SharedBytes;

struct ChunkSources<'a> {
    /// Overrides read from mod content providers, grouped by mod id. Their bytes
    /// are compressed by the writer.
    by_mod: HashMap<&'a str, Vec<WadHash>>,

    /// Synthetic stringtable patches, rebuilt from the game chunk + merged plan.
    string_patches: Vec<WadHash>,
}

impl<'a> ChunkSources<'a> {
    fn classify(
        needed_hashes: &HashSet<WadHash>,
        all_meta: &'a HashMap<WadHash, OverrideMeta>,
    ) -> Self {
        let mut sources = ChunkSources {
            by_mod: HashMap::new(),
            string_patches: Vec::new(),
        };

        for &path_hash in needed_hashes {
            let Some(meta) = all_meta.get(&path_hash) else {
                continue;
            };

            match &meta.source {
                OverrideSource::LayerWad { mod_id, .. } | OverrideSource::Raw { mod_id, .. } => {
                    sources
                        .by_mod
                        .entry(mod_id.as_str())
                        .or_default()
                        .push(path_hash);
                }
                OverrideSource::StringPatch { .. } => sources.string_patches.push(path_hash),
            }
        }
        sources
    }
}

/// Spread the flat `prepared` map into the per-WAD maps the patch step consumes:
/// each WAD gets a handle on the prepared override for every hash routed to it.
fn distribute_prepared_to_wads(
    wads_to_build: &[Utf8PathBuf],
    wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<WadHash>>,
    prepared: &HashMap<WadHash, PreparedOverride>,
) -> BTreeMap<Utf8PathBuf, HashMap<WadHash, PreparedOverride>> {
    let mut wad_overrides: BTreeMap<Utf8PathBuf, HashMap<WadHash, PreparedOverride>> =
        BTreeMap::new();
    for wad_path in wads_to_build {
        let Some(hashes) = wad_hash_sets.get(wad_path) else {
            continue;
        };
        let mut per_wad: HashMap<WadHash, PreparedOverride> = HashMap::with_capacity(hashes.len());
        for &hash in hashes {
            if let Some(chunk) = prepared.get(&hash) {
                per_wad.insert(hash, chunk.clone());
            }
        }
        wad_overrides.insert(wad_path.clone(), per_wad);
    }
    wad_overrides
}

/// How many uncompressed bytes accumulate before a batch is compressed.
///
/// The bound on pass 2's uncompressed residency: one batch plus per-thread
/// compression buffers, instead of every rebuilt WAD's bytes at once. 256 MiB
/// keeps every compression thread saturated between flushes while staying
/// immaterial next to the page cache on the 16 GiB machines this exists for.
const BATCH_BUDGET_BYTES: usize = 256 * 1024 * 1024;

/// Compresses needed overrides once per distinct content, in bounded batches.
///
/// The caller asks [`needs`](Self::needs) before reading a chunk's bytes and
/// supplies them only when asked to, so a content appearing under many path
/// hashes - a chunk routed to several WADs, or two overrides carrying
/// identical bytes - is read and compressed a single time and then shares one
/// [`PreparedOverride`]. That structural sharing is what keeps every copy of a
/// cross-WAD chunk on the same compressed checksum, which the game validates
/// (see the [`wad_builder`] module docs).
///
/// Supplied bytes accumulate up to the budget and are compressed in parallel
/// per batch, each uncompressed buffer dropped as soon as its compressed form
/// exists. Nothing downstream of the writer reads uncompressed override bytes.
///
/// `reused` seeds the memo with bytes already compressed in a previous build,
/// recovered from an overlay's tail. Anything whose content they already cover
/// is never read or compressed again, and every WAD needing that content emits
/// those exact bytes.
///
/// [`wad_builder`]: crate::wad_builder
struct OverrideCompressor<'a> {
    all_meta: &'a HashMap<WadHash, OverrideMeta>,
    reused: HashMap<WadHash, PreparedOverride>,
    /// Compressed overrides so far, keyed by *content* hash: that is what makes
    /// one buffer serve every chunk carrying identical bytes.
    memo: HashMap<ContentHash, PreparedOverride>,
    /// Every path hash `needs` or `supply` saw, mapped to its content identity,
    /// so `finish` can spread the memo back over path hashes.
    content_of: HashMap<WadHash, ContentHash>,
    /// Contents waiting in the current batch, so a duplicate arriving before
    /// the flush is not requested again.
    pending: HashSet<ContentHash>,
    batch: Vec<(ContentHash, WadHash, ResolvedChunk)>,
    batch_bytes: usize,
    budget: usize,
    compressed_total: usize,
    /// Contents copied in already compressed, which never reach a batch.
    passed_through: usize,
}

impl<'a> OverrideCompressor<'a> {
    fn new(
        all_meta: &'a HashMap<WadHash, OverrideMeta>,
        reused: HashMap<WadHash, PreparedOverride>,
        budget: usize,
    ) -> Self {
        let content_hash_of = |path_hash: WadHash| {
            all_meta
                .get(&path_hash)
                .map_or(ContentHash(path_hash.0), |meta| meta.content_hash)
        };
        let memo = reused
            .iter()
            .map(|(&path_hash, prepared)| (content_hash_of(path_hash), prepared.clone()))
            .collect();

        Self {
            all_meta,
            reused,
            memo,
            content_of: HashMap::new(),
            pending: HashSet::new(),
            batch: Vec::new(),
            batch_bytes: 0,
            budget,
            compressed_total: 0,
            passed_through: 0,
        }
    }

    /// The content identity a path hash is deduplicated under.
    ///
    /// A chunk with no metadata entry cannot be deduplicated against anything,
    /// so it keys on its own path hash - distinct by construction.
    fn record(&mut self, path_hash: WadHash) -> ContentHash {
        let content_hash = self
            .all_meta
            .get(&path_hash)
            .map_or(ContentHash(path_hash.0), |meta| meta.content_hash);
        self.content_of.insert(path_hash, content_hash);
        content_hash
    }

    /// Whether `path_hash`'s bytes still have to be read and supplied.
    ///
    /// False when its content is already compressed or waiting in the current
    /// batch; the path hash is remembered either way and lands on the shared
    /// buffer at [`finish`](Self::finish).
    fn needs(&mut self, path_hash: WadHash) -> bool {
        let content_hash = self.record(path_hash);
        !self.memo.contains_key(&content_hash) && !self.pending.contains(&content_hash)
    }

    /// Queue one chunk's uncompressed bytes, flushing when the batch is full.
    ///
    /// Bytes for a content already compressed or already queued are dropped.
    ///
    /// # Errors
    ///
    /// Fails when a flushed batch fails to compress.
    fn supply(&mut self, path_hash: WadHash, bytes: ResolvedChunk) -> Result<()> {
        let content_hash = self.record(path_hash);
        if self.memo.contains_key(&content_hash) || !self.pending.insert(content_hash) {
            return Ok(());
        }

        self.batch_bytes += bytes.len();
        self.batch.push((content_hash, path_hash, bytes));
        if self.batch_bytes >= self.budget {
            self.flush()?;
        }
        Ok(())
    }

    /// Record a chunk whose bytes arrived already compressed, skipping the
    /// batch entirely.
    ///
    /// A pass-through has nothing left to compress, so its buffer goes straight
    /// into the memo and serves every other path hash carrying the same content.
    /// Callers gate this on [`needs`](Self::needs) exactly as they gate
    /// [`supply`](Self::supply), so a content is never both queued and memoized.
    ///
    /// One content can reach a build both ways - passed through from one mod's
    /// packed WAD and read loose from another's - and whichever arrives first
    /// wins. The two encodings are equally valid and every WAD in the build
    /// still gets the one buffer, which is all the game's shared-chunk rule
    /// asks for; a later build settling on the other encoding only matters to a
    /// WAD being rewritten, and those seed the memo from the tail they keep.
    fn supply_prepared(&mut self, path_hash: WadHash, prepared: PreparedOverride) {
        let content_hash = self.record(path_hash);
        self.memo.insert(content_hash, prepared);
        self.passed_through += 1;
    }

    /// Compress the queued batch in parallel and drop its uncompressed bytes.
    fn flush(&mut self) -> Result<()> {
        if self.batch.is_empty() {
            return Ok(());
        }
        let batch = std::mem::take(&mut self.batch);
        self.batch_bytes = 0;
        self.compressed_total += batch.len();

        self.memo.par_extend(
            batch
                .into_par_iter()
                .map(|(content_hash, path_hash, bytes)| {
                    PreparedOverride::compress(path_hash, &bytes)
                        .map(|prepared| (content_hash, prepared))
                })
                .collect::<Result<Vec<_>>>()?,
        );
        self.pending.clear();
        Ok(())
    }

    /// Flush the last batch and spread the memo over every recorded path hash.
    ///
    /// A path hash whose content never arrived - a stringtable patch that
    /// failed to apply, say - is dropped rather than fatal; the writer falls
    /// back per chunk.
    ///
    /// # Errors
    ///
    /// Fails when the final batch fails to compress.
    fn finish(mut self) -> Result<HashMap<WadHash, PreparedOverride>> {
        self.flush()?;

        tracing::info!(
            "Compressed {} unique override chunk(s); passed {} through already compressed; \
             reused {} from the previous build",
            self.compressed_total,
            self.passed_through,
            self.reused.len()
        );

        let mut prepared = self.reused;
        prepared.reserve(self.content_of.len());
        for (path_hash, content_hash) in self.content_of {
            if let Some(chunk) = self.memo.get(&content_hash) {
                prepared.insert(path_hash, chunk.clone());
            }
        }
        Ok(prepared)
    }
}

impl OverlayBuilder {
    /// Distribute override path hashes to all affected WADs (lightweight).
    ///
    /// Returns a map of `relative_wad_path -> set of path_hashes`. No byte data
    /// is involved - only routing via [`OverrideMeta::route_targets`]: every
    /// game WAD that contains the hash, plus the mod's declared WAD for new
    /// entries and cross-WAD imports (chunks the mod ships under a WAD that
    /// doesn't already contain them).
    pub(crate) fn distribute_override_hashes(
        &self,
        all_meta: &HashMap<WadHash, OverrideMeta>,
        game_index: &GameIndex,
    ) -> BTreeMap<Utf8PathBuf, HashSet<WadHash>> {
        let mut wad_hash_sets: BTreeMap<&Utf8Path, HashSet<WadHash>> = BTreeMap::new();
        let mut new_entry_count = 0usize;
        let mut cross_import_count = 0usize;
        let mut dropped_count = 0usize;

        for (&path_hash, meta) in all_meta {
            let targets = meta.route_targets(path_hash, game_index);
            if targets.is_empty() {
                dropped_count += 1;
                tracing::debug!(
                    "Override {:016x} from mod '{}' ('{}') matches no game WAD and has no \
                     fallback target; skipping",
                    path_hash,
                    meta.source.mod_id(),
                    meta.source.rel_path(),
                );
                continue;
            }

            if game_index.find_wads_with_hash(path_hash).is_none() {
                new_entry_count += 1;
            } else if meta.is_cross_wad_import(path_hash, game_index) {
                cross_import_count += 1;
            }

            for wad_path in targets {
                wad_hash_sets.entry(wad_path).or_default().insert(path_hash);
            }
        }

        if new_entry_count > 0 {
            tracing::info!(
                "Routed {} new entries (not in any game WAD) via mod directory structure",
                new_entry_count
            );
        }
        if cross_import_count > 0 {
            tracing::info!(
                "Routed {} cross-WAD import(s) into their mods' declared WADs \
                 (chunk originates from a different game WAD)",
                cross_import_count
            );
        }
        if dropped_count > 0 {
            tracing::warn!(
                "{} override(s) could not be routed to any game WAD (no hash match and no \
                 fallback target) and were skipped - that mod content will not appear in-game",
                dropped_count
            );
        }
        tracing::info!(
            "Distributed override hashes to {} affected WAD files",
            wad_hash_sets.len()
        );

        wad_hash_sets
            .into_iter()
            .map(|(path, hashes)| (path.to_path_buf(), hashes))
            .collect()
    }

    /// Compute per-WAD fingerprints from metadata and partition into rebuild vs reuse.
    ///
    /// Returns `(wads_to_build, wads_to_reuse, new_wad_fingerprints)`.
    pub(crate) fn partition_wads_from_meta(
        &self,
        wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<WadHash>>,
        all_meta: &HashMap<WadHash, OverrideMeta>,
        prev_state: &Option<OverlayState>,
        can_incremental: bool,
    ) -> (Vec<Utf8PathBuf>, Vec<Utf8PathBuf>, BTreeMap<String, u64>) {
        let new_wad_fingerprints: BTreeMap<String, u64> = wad_hash_sets
            .iter()
            .map(|(wad_path, hashes)| {
                (
                    wad_path.as_str().to_string(),
                    compute_wad_fingerprint_from_meta(hashes, all_meta),
                )
            })
            .collect();

        let mut wads_to_build: Vec<Utf8PathBuf> = Vec::new();
        let mut wads_to_reuse: Vec<Utf8PathBuf> = Vec::new();

        for (wad_path_str, &new_fp) in &new_wad_fingerprints {
            let wad_path = Utf8PathBuf::from(wad_path_str);
            let overlay_wad = self.overlay_root.join(&wad_path);

            // A WAD left dirty by an interrupted rewrite may be torn, so it is
            // rebuilt even when its overrides did not change - which happens
            // when the edit that triggered the killed build is reverted.
            if can_incremental
                && let Some(state) = prev_state
                && !state.dirty_wads.contains(wad_path_str)
                && let Some(old_fp) = state.wad_fingerprint(wad_path_str)
                && old_fp == new_fp
                && overlay_wad.as_std_path().exists()
            {
                tracing::debug!("Reusing WAD: {}", wad_path);
                wads_to_reuse.push(wad_path);
                continue;
            }

            tracing::debug!("Need to rebuild WAD: {}", wad_path);
            wads_to_build.push(wad_path);
        }

        (wads_to_build, wads_to_reuse, new_wad_fingerprints)
    }

    /// Resolve the bytes each rebuilding WAD needs (pass 2).
    ///
    /// Every needed chunk is resolved once, from exactly one of two sources
    /// (see [`ChunkSources`]), into a [`ResolvedChunk`], then the flat result is
    /// spread across the per-WAD maps the patch step consumes:
    ///
    /// - **Mod overrides** - read from each mod's content provider.
    /// - **String patches** - the game's stringtable rebuilt with merged overrides.
    ///
    /// `reused` holds overrides whose compressed bytes a
    /// [tail rewrite](super::incremental) already recovered from an overlay's
    /// existing tail. Those are neither re-read nor recompressed: they seed the
    /// memo, so a WAD building the same content fresh in this build emits the
    /// bytes the reusing WAD is keeping.
    ///
    /// The rest is compressed by the [`OverrideCompressor`], once per distinct
    /// content, and the results are spread across the WADs.
    pub(crate) fn resolve_overrides_for_wads(
        &mut self,
        wads_to_build: &[Utf8PathBuf],
        wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<WadHash>>,
        all_meta: &HashMap<WadHash, OverrideMeta>,
        string_plans: &HashMap<WadHash, StringPatchPlan>,
        reused: HashMap<WadHash, PreparedOverride>,
    ) -> Result<BTreeMap<Utf8PathBuf, HashMap<WadHash, PreparedOverride>>> {
        // Every unique path hash needed across the WADs being rebuilt, minus
        // the ones a tail rewrite supplies out of the file it is rewriting.
        let needed_hashes: HashSet<WadHash> = wads_to_build
            .iter()
            .filter_map(|wad_path| wad_hash_sets.get(wad_path))
            .flat_map(|hashes| hashes.iter().copied())
            .filter(|hash| !reused.contains_key(hash))
            .collect();

        if needed_hashes.is_empty() && reused.is_empty() {
            return Ok(BTreeMap::new());
        }

        let sources = ChunkSources::classify(&needed_hashes, all_meta);

        let mut preparer = OverrideCompressor::new(all_meta, reused, BATCH_BUDGET_BYTES);
        let mismatches =
            self.resolve_provider_overrides(&sources.by_mod, all_meta, &mut preparer)?;
        self.resolve_string_patches(&sources.string_patches, string_plans, &mut preparer)?;
        self.last_checksum_mismatches.extend(mismatches);

        let prepared = preparer.finish()?;

        Ok(distribute_prepared_to_wads(
            wads_to_build,
            wad_hash_sets,
            &prepared,
        ))
    }

    /// Resolve overrides read from mod content providers into the preparer,
    /// reading each distinct content once no matter how many path hashes or
    /// WADs carry it.
    ///
    /// An override whose provider already holds it in a WAD's stored form is
    /// *passed through*: its bytes go to the preparer already compressed and are
    /// never decoded (see [`try_pass_through`]). Everything else is read
    /// decompressed and compressed by the preparer as before.
    ///
    /// Returns the chunks whose container claimed a checksum its bytes do not
    /// have, which the caller reports and does not fail on.
    fn resolve_provider_overrides(
        &mut self,
        by_mod: &HashMap<&str, Vec<WadHash>>,
        all_meta: &HashMap<WadHash, OverrideMeta>,
        preparer: &mut OverrideCompressor<'_>,
    ) -> Result<Vec<ChecksumMismatch>> {
        let mod_id_to_index: HashMap<String, usize> = self
            .enabled_mods
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id.clone(), i))
            .collect();

        let mut mismatches = Vec::new();

        for (mod_id, hashes) in by_mod {
            let Some(&idx) = mod_id_to_index.get(*mod_id) else {
                return Err(Error::Bug(Invariant::OverrideNamesUnenabledMod));
            };
            let provider = &mut self.enabled_mods[idx].content;

            for &path_hash in hashes {
                if !preparer.needs(path_hash) {
                    continue;
                }
                let bytes = match &all_meta[&path_hash].source {
                    OverrideSource::LayerWad {
                        layer,
                        wad_name,
                        rel_path,
                        ..
                    } => {
                        if let Some(passed) = try_pass_through(
                            provider.as_mut(),
                            mod_id,
                            path_hash,
                            layer,
                            wad_name,
                            rel_path,
                            &mut mismatches,
                        )? {
                            preparer.supply_prepared(path_hash, passed);
                            continue;
                        }
                        provider.read_wad_override_file(layer, wad_name, rel_path)?
                    }
                    OverrideSource::Raw { rel_path, .. } => {
                        provider.read_raw_override_file(rel_path)?
                    }
                    OverrideSource::StringPatch { .. } => {
                        return Err(Error::Bug(Invariant::StringPatchGroupedByMod));
                    }
                };
                preparer.supply(path_hash, Arc::from(bytes))?;
            }
        }

        Ok(mismatches)
    }

    /// Resolve synthetic stringtable patches into uncompressed bytes: the game's
    /// own stringtable chunk rebuilt with the locale's merged key overrides.
    ///
    /// A read/patch failure is logged and that locale left unpatched - the WAD is
    /// still written, just without the string patch - instead of failing the build.
    /// A missing plan is an internal invariant violation and is a hard error.
    fn resolve_string_patches(
        &self,
        string_patch_hashes: &[WadHash],
        string_plans: &HashMap<WadHash, StringPatchPlan>,
        preparer: &mut OverrideCompressor<'_>,
    ) -> Result<()> {
        for &path_hash in string_patch_hashes {
            if !preparer.needs(path_hash) {
                continue;
            }
            let plan = string_plans
                .get(&path_hash)
                .ok_or(Error::Bug(Invariant::StringPatchWithoutPlan))?;
            let patched =
                strings::read_game_chunk(&self.game_dir, &plan.wad_rel_path, plan.chunk_hash)
                    .and_then(|base_bytes| plan.apply(&base_bytes));
            match patched {
                Ok(bytes) => {
                    preparer.supply(path_hash, Arc::from(bytes))?;
                }
                Err(e) => tracing::error!(
                    "Failed to apply string overrides for locale '{}': {}; the game \
                     stringtable is left unpatched",
                    plan.locale,
                    e
                ),
            }
        }

        Ok(())
    }

    /// Patch WADs in parallel, emitting progress after each one completes.
    ///
    /// A WAD with a [`TailRewrite`] plan keeps its file and rewrites only the
    /// tail and the TOC; every other WAD is rebuilt in full through
    /// [`build_patched_wad`]. Consumes `wad_overrides` and `rewrites` so each
    /// parallel task owns its data, enabling progressive deallocation as each
    /// WAD finishes patching.
    pub(crate) fn patch_wads_parallel(
        &self,
        wads_to_build: Vec<Utf8PathBuf>,
        mut wad_overrides: BTreeMap<Utf8PathBuf, HashMap<WadHash, PreparedOverride>>,
        mut rewrites: BTreeMap<Utf8PathBuf, TailRewrite>,
    ) -> Result<Vec<PatchedWad>> {
        let total_wads = wads_to_build.len() as u32;
        let completed = AtomicU32::new(0);
        let reported = AtomicU32::new(0);
        let progress_callback = &self.progress_callback;

        let emit = |progress: OverlayProgress| {
            if let Some(callback) = progress_callback {
                callback(progress);
            }
        };

        emit(OverlayProgress {
            stage: OverlayStage::PatchingWad,
            current_file: None,
            current: 0,
            total: total_wads,
        });

        // Extract per-WAD work so each parallel task owns its data.
        let per_wad_work: Vec<WadWork> = wads_to_build
            .into_iter()
            .map(|relative_path| WadWork {
                overrides: wad_overrides.remove(&relative_path).unwrap_or_default(),
                rewrite: rewrites.remove(&relative_path),
                relative_path,
            })
            .collect();
        drop(wad_overrides);
        drop(rewrites);

        per_wad_work
            .into_par_iter()
            .map(|work| {
                let patched = self.patch_one_wad(work)?;

                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                let current = reported.fetch_max(done, Ordering::Relaxed).max(done);
                emit(OverlayProgress {
                    stage: OverlayStage::PatchingWad,
                    current_file: Some(
                        patched
                            .relative_path
                            .file_name()
                            .unwrap_or("unknown")
                            .to_string(),
                    ),
                    current,
                    total: total_wads,
                });

                Ok(patched)
            })
            .collect()
    }

    /// Write one overlay WAD, by tail rewrite when planned or full rebuild.
    ///
    /// A tail rewrite that fails falls back to the full rebuild rather than
    /// failing the build. Its file may be torn by then, but the full rebuild
    /// writes a fresh one through a temp file and renames it over the top, so
    /// the WAD comes out correct either way.
    fn patch_one_wad(&self, work: WadWork) -> Result<PatchedWad> {
        let WadWork {
            relative_path,
            overrides,
            rewrite,
        } = work;
        let dst_wad_path = self.overlay_root.join(&relative_path);

        if let Some(rewrite) = rewrite {
            match self.rewrite_one_wad_tail(&dst_wad_path, rewrite, &overrides) {
                Ok(patched) => return Ok(patched.into_wad(relative_path, dst_wad_path)),
                Err(e) => tracing::warn!(
                    "Tail rewrite of {} failed ({}); rebuilding it in full",
                    dst_wad_path,
                    e
                ),
            }
        }

        let patched = self.rebuild_one_wad(&relative_path, &dst_wad_path, overrides)?;
        Ok(patched.into_wad(relative_path, dst_wad_path))
    }

    /// Rewrite an overlay WAD's tail, keeping its copied source data region.
    ///
    /// The override map is read rather than drained, so a failure here leaves
    /// the caller able to run the full rebuild with the same data. The plan
    /// itself is consumed: its base entries become the file's new TOC, and the
    /// fallback rebuild needs none of it.
    fn rewrite_one_wad_tail(
        &self,
        dst_wad_path: &Utf8Path,
        rewrite: TailRewrite,
        overrides: &HashMap<WadHash, PreparedOverride>,
    ) -> Result<WrittenWad> {
        let start = std::time::Instant::now();
        let TailRewrite {
            record,
            base_entries,
            tail_hashes,
            ..
        } = rewrite;

        // An override whose bytes never resolved - a stringtable patch that
        // failed to apply, say - is dropped rather than fatal, and its chunk
        // falls back to the entry the source region holds. That is what the
        // full-rebuild path does with it too.
        let tail: Vec<(WadHash, PreparedOverride)> = tail_hashes
            .into_iter()
            .filter_map(|hash| overrides.get(&hash).cloned().map(|over| (hash, over)))
            .collect();

        tracing::info!(
            "Rewriting WAD tail dst={} overrides={}",
            dst_wad_path,
            tail.len()
        );

        // The planner proved this source identity before choosing the in-place
        // path; the rewrite itself never opens the game WAD.
        let base_entry_count = base_entries.len();
        let plan = WadTailPlan::new(&record.layout, base_entries, &tail)?;

        // Every check that can be made without the file has now been made, so
        // the file can be committed to. Truncating is what makes room for a
        // tail shorter than the one the file holds, and there is no `.tmp` to
        // discard if the write then fails - which is exactly what the caller's
        // fallback rebuild is for.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dst_wad_path.as_std_path())
            .map_err(|source| Error::write(dst_wad_path, source))?;
        file.set_len(record.layout.tail_offset)
            .map_err(|source| Error::write(dst_wad_path, source))?;

        let mut file = file;
        let report = plan.write(&mut file, 0)?;

        let elapsed_ms = start.elapsed().as_millis();
        tracing::info!(
            "Rewrote WAD tail dst={} chunks={} overrides={} tail_bytes={} elapsed_ms={}",
            dst_wad_path,
            report.entry_count,
            tail.len(),
            report.tail_len,
            elapsed_ms
        );

        Ok(WrittenWad {
            stats: PatchedWadStats {
                chunks_written: report.entry_count,
                overrides_applied: tail.len(),
                new_entries_added: report.entry_count.saturating_sub(base_entry_count),
                chunks_transient: report.entry_count - tail.len(),
                elapsed_ms,
                layout: record.layout,
                source: record.source,
            },
            written_overrides: tail.into_iter().map(|(hash, _)| hash).collect(),
        })
    }

    /// Rebuild an overlay WAD from its game WAD, through a temp file.
    fn rebuild_one_wad(
        &self,
        relative_path: &Utf8Path,
        dst_wad_path: &Utf8Path,
        mut overrides: HashMap<WadHash, PreparedOverride>,
    ) -> Result<WrittenWad> {
        let src_wad_path = self.game_dir.join(relative_path);
        let override_hashes: HashSet<WadHash> = overrides.keys().copied().collect();

        tracing::info!(
            "Patching WAD src={} dst={} overrides={}",
            src_wad_path,
            dst_wad_path,
            override_hashes.len()
        );

        let stats = build_patched_wad(&src_wad_path, dst_wad_path, &override_hashes, |hash| {
            take_override(&mut overrides, hash)
        })?;

        Ok(WrittenWad {
            stats,
            written_overrides: override_hashes.into_iter().collect(),
        })
    }
}

/// Copy one override's stored bytes through, when its provider holds them that
/// way and this crate can write them.
///
/// `None` sends the override down the ordinary read-and-compress path, for
/// either reason it can fail to qualify: the provider stores it loose, or it is
/// stored under a codec the overlay writer does not emit - in practice a `.tex`
/// inside a packed WAD, whose subchunk table lives in the WAD it came from.
/// Refusing the second kind costs one copy of its compressed bytes, which is
/// what buys the codec decision staying with the writer instead of being
/// duplicated into every provider.
///
/// A claim that disagrees with the bytes is warned about and pushed onto
/// `mismatches`; the overlay carries the checksum computed over the bytes
/// either way, so the build carries on (ADR-0001).
///
/// # Errors
///
/// Fails when the provider offers the chunk but cannot read it, or when its
/// sizes overflow the WAD format's `u32` fields.
fn try_pass_through(
    provider: &mut dyn ModContentProvider,
    mod_id: &str,
    path_hash: WadHash,
    layer: &str,
    wad_name: &str,
    rel_path: &Utf8Path,
    mismatches: &mut Vec<ChecksumMismatch>,
) -> Result<Option<PreparedOverride>> {
    let Some(chunk) = provider.read_wad_override_compressed(layer, wad_name, rel_path)? else {
        return Ok(None);
    };

    let claimed = chunk.claimed_checksum;
    let Some(prepared) = PreparedOverride::pass_through(path_hash, chunk)? else {
        return Ok(None);
    };

    let computed = prepared.checksum();
    if computed != claimed {
        tracing::warn!(
            "Mod '{}' claims checksum {:016x} for chunk {:016x} of '{}', but those bytes hash \
             to {:016x}; the overlay records the computed value",
            mod_id,
            claimed,
            path_hash,
            wad_name,
            computed,
        );
        mismatches.push(ChecksumMismatch {
            mod_id: mod_id.to_string(),
            wad_name: wad_name.to_string(),
            path_hash,
            claimed,
            computed,
        });
    }

    Ok(Some(prepared))
}

/// Hand one prepared override to a writer, releasing it from the WAD's map.
///
/// Its bytes are then freed as soon as the last WAD holding them has written it.
fn take_override(
    overrides: &mut HashMap<WadHash, PreparedOverride>,
    path_hash: WadHash,
) -> Result<PreparedOverride> {
    overrides.remove(&path_hash).ok_or_else(missing_override)
}

/// A writer asked for override bytes this build never resolved.
///
/// Both write paths plan their hashes from the same map they then read, so this
/// is a broken invariant rather than anything a mod can provoke.
fn missing_override() -> Error {
    Error::Bug(Invariant::OverrideNeverPrepared)
}

/// One WAD's share of a parallel patch pass.
struct WadWork {
    relative_path: Utf8PathBuf,
    overrides: HashMap<WadHash, PreparedOverride>,
    /// `Some` when this WAD passed the [tail-rewrite](super::incremental)
    /// preconditions, in which case its file is kept and only its tail rewritten.
    rewrite: Option<TailRewrite>,
}

/// The result of one write, before it is paired with the WAD's paths.
struct WrittenWad {
    stats: PatchedWadStats,
    written_overrides: Vec<WadHash>,
}

impl WrittenWad {
    fn into_wad(self, relative_path: Utf8PathBuf, path: Utf8PathBuf) -> PatchedWad {
        PatchedWad {
            relative_path,
            path,
            stats: self.stats,
            written_overrides: self.written_overrides,
        }
    }
}

/// One overlay WAD this build wrote.
pub(crate) struct PatchedWad {
    /// Game-relative path, the key both `wad_fingerprints` and `wad_layouts`
    /// are stored under.
    pub(crate) relative_path: Utf8PathBuf,
    /// Absolute path of the file that was written.
    pub(crate) path: Utf8PathBuf,
    /// Metrics, layout and source identity of the write.
    pub(crate) stats: PatchedWadStats,
    /// The overrides that actually reached the file.
    ///
    /// Not always the set that was routed to this WAD: an override whose bytes
    /// could not be resolved is skipped. The layout record has to describe what
    /// the file holds, not what the build intended it to hold.
    pub(crate) written_overrides: Vec<WadHash>,
}

#[cfg(test)]
mod tests;
