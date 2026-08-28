//! Between-passes distribution, Pass 2 resolve, and parallel WAD patching.
//!
//! Routes override hashes to affected WADs, partitions into rebuild/reuse sets,
//! re-reads bytes for WADs that need rebuilding, and patches WADs in parallel.

use super::*;
use crate::builder::incremental::TailRewrite;
use crate::utils::compute_wad_fingerprint_from_meta;
use crate::wad_builder::{PatchedWadStats, PreparedOverride, build_patched_wad, rewrite_wad_tail};
use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

/// Uncompressed override bytes as read in pass 2, before [`prepare_overrides`]
/// turns them into the compressed [`PreparedOverride`]s the writer consumes.
pub(crate) type ResolvedChunk = SharedBytes;

struct ChunkSources<'a> {
    /// Overrides read from mod content providers, grouped by mod id. Their bytes
    /// are compressed by the writer.
    by_mod: HashMap<&'a str, Vec<u64>>,

    /// Synthetic stringtable patches, rebuilt from the game chunk + merged plan.
    string_patches: Vec<u64>,
}

impl<'a> ChunkSources<'a> {
    fn classify(needed_hashes: &HashSet<u64>, all_meta: &'a HashMap<u64, OverrideMeta>) -> Self {
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
    wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<u64>>,
    prepared: &HashMap<u64, PreparedOverride>,
) -> BTreeMap<Utf8PathBuf, HashMap<u64, PreparedOverride>> {
    let mut wad_overrides: BTreeMap<Utf8PathBuf, HashMap<u64, PreparedOverride>> = BTreeMap::new();
    for wad_path in wads_to_build {
        let Some(hashes) = wad_hash_sets.get(wad_path) else {
            continue;
        };
        let mut per_wad: HashMap<u64, PreparedOverride> = HashMap::with_capacity(hashes.len());
        for &hash in hashes {
            if let Some(chunk) = prepared.get(&hash) {
                per_wad.insert(hash, chunk.clone());
            }
        }
        wad_overrides.insert(wad_path.clone(), per_wad);
    }
    wad_overrides
}

/// Compress every resolved override, once per distinct content, in parallel.
///
/// Overrides are memoized on their pass-1 `content_hash`, so a chunk routed to
/// several WADs - and any two overrides that happen to carry identical bytes -
/// are compressed a single time and then share one [`PreparedOverride`]. That
/// structural sharing is what keeps every copy of a cross-WAD chunk on the same
/// compressed checksum, which the game validates (see the [`wad_builder`] module
/// docs).
///
/// Consumes `resolved`: each uncompressed buffer is dropped as soon as its
/// compressed form exists, and nothing downstream of the writer reads
/// uncompressed override bytes.
///
/// `reused` seeds the memo with bytes already compressed in a previous build,
/// recovered from an overlay's tail. Anything whose content they already cover
/// is never compressed again, and every WAD needing that content emits those
/// exact bytes.
///
/// [`wad_builder`]: crate::wad_builder
fn prepare_overrides(
    resolved: HashMap<u64, ResolvedChunk>,
    all_meta: &HashMap<u64, OverrideMeta>,
    reused: HashMap<u64, PreparedOverride>,
) -> Result<HashMap<u64, PreparedOverride>> {
    // A chunk with no metadata entry cannot be deduplicated against anything,
    // so it keys on its own path hash - distinct by construction.
    let content_hash_of = |path_hash: u64| {
        all_meta
            .get(&path_hash)
            .map_or(path_hash, |meta| meta.content_hash)
    };

    let mut memo: HashMap<u64, PreparedOverride> = reused
        .iter()
        .map(|(&path_hash, prepared)| (content_hash_of(path_hash), prepared.clone()))
        .collect();

    // One representative (path hash, bytes) per distinct content still needing
    // compression. Duplicate buffers are released here, before any starts.
    let mut content_of: HashMap<u64, u64> = HashMap::with_capacity(resolved.len());
    let mut distinct: HashMap<u64, (u64, ResolvedChunk)> = HashMap::new();
    for (path_hash, bytes) in resolved {
        let content_hash = content_hash_of(path_hash);
        content_of.insert(path_hash, content_hash);
        if !memo.contains_key(&content_hash) {
            distinct.entry(content_hash).or_insert((path_hash, bytes));
        }
    }

    let compressed = distinct.len();
    memo.par_extend(
        distinct
            .into_par_iter()
            .map(|(content_hash, (path_hash, bytes))| {
                PreparedOverride::compress(path_hash, &bytes)
                    .map(|prepared| (content_hash, prepared))
            })
            .collect::<Result<Vec<_>>>()?,
    );

    tracing::info!(
        "Compressed {} unique override chunk(s); reused {} from the previous build",
        compressed,
        reused.len()
    );

    let mut prepared = reused;
    prepared.reserve(content_of.len());
    for (path_hash, content_hash) in content_of {
        if let Some(chunk) = memo.get(&content_hash) {
            prepared.insert(path_hash, chunk.clone());
        }
    }

    Ok(prepared)
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
        all_meta: &HashMap<u64, OverrideMeta>,
        game_index: &GameIndex,
    ) -> BTreeMap<Utf8PathBuf, HashSet<u64>> {
        let mut wad_hash_sets: BTreeMap<&Utf8Path, HashSet<u64>> = BTreeMap::new();
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
        wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<u64>>,
        all_meta: &HashMap<u64, OverrideMeta>,
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
    /// The rest is compressed by [`prepare_overrides`], once per distinct
    /// content, and the results are spread across the WADs.
    pub(crate) fn resolve_overrides_for_wads(
        &mut self,
        wads_to_build: &[Utf8PathBuf],
        wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<u64>>,
        all_meta: &HashMap<u64, OverrideMeta>,
        string_plans: &HashMap<u64, StringPatchPlan>,
        reused: HashMap<u64, PreparedOverride>,
    ) -> Result<BTreeMap<Utf8PathBuf, HashMap<u64, PreparedOverride>>> {
        // Every unique path hash needed across the WADs being rebuilt, minus
        // the ones a tail rewrite supplies out of the file it is rewriting.
        let needed_hashes: HashSet<u64> = wads_to_build
            .iter()
            .filter_map(|wad_path| wad_hash_sets.get(wad_path))
            .flat_map(|hashes| hashes.iter().copied())
            .filter(|hash| !reused.contains_key(hash))
            .collect();

        if needed_hashes.is_empty() && reused.is_empty() {
            return Ok(BTreeMap::new());
        }

        let sources = ChunkSources::classify(&needed_hashes, all_meta);

        let mut resolved: HashMap<u64, ResolvedChunk> = HashMap::with_capacity(needed_hashes.len());
        self.resolve_provider_overrides(&sources.by_mod, all_meta, &mut resolved)?;
        self.resolve_string_patches(&sources.string_patches, string_plans, &mut resolved)?;

        let prepared = prepare_overrides(resolved, all_meta, reused)?;

        Ok(distribute_prepared_to_wads(
            wads_to_build,
            wad_hash_sets,
            &prepared,
        ))
    }

    /// Resolve overrides read from mod content providers into uncompressed bytes,
    /// reading each file once and cloning the bytes for every WAD that needs it.
    fn resolve_provider_overrides(
        &mut self,
        by_mod: &HashMap<&str, Vec<u64>>,
        all_meta: &HashMap<u64, OverrideMeta>,
        resolved: &mut HashMap<u64, ResolvedChunk>,
    ) -> Result<()> {
        let mod_id_to_index: HashMap<String, usize> = self
            .enabled_mods
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id.clone(), i))
            .collect();

        for (mod_id, hashes) in by_mod {
            let Some(&idx) = mod_id_to_index.get(*mod_id) else {
                return Err(Error::Other(format!(
                    "Override source references unknown mod '{}'",
                    mod_id
                )));
            };
            let provider = &mut self.enabled_mods[idx].content;

            for &path_hash in hashes {
                let bytes = match &all_meta[&path_hash].source {
                    OverrideSource::LayerWad {
                        layer,
                        wad_name,
                        rel_path,
                        ..
                    } => provider.read_wad_override_file(layer, wad_name, rel_path)?,
                    OverrideSource::Raw { rel_path, .. } => {
                        provider.read_raw_override_file(rel_path)?
                    }
                    OverrideSource::StringPatch { .. } => {
                        unreachable!("StringPatch sources are not grouped by mod")
                    }
                };
                resolved.insert(path_hash, Arc::from(bytes));
            }
        }

        Ok(())
    }

    /// Resolve synthetic stringtable patches into uncompressed bytes: the game's
    /// own stringtable chunk rebuilt with the locale's merged key overrides.
    ///
    /// A read/patch failure is logged and that locale left unpatched - the WAD is
    /// still written, just without the string patch - instead of failing the build.
    /// A missing plan is an internal invariant violation and is a hard error.
    fn resolve_string_patches(
        &self,
        string_patch_hashes: &[u64],
        string_plans: &HashMap<u64, StringPatchPlan>,
        resolved: &mut HashMap<u64, ResolvedChunk>,
    ) -> Result<()> {
        for &path_hash in string_patch_hashes {
            let plan = string_plans.get(&path_hash).ok_or_else(|| {
                Error::Other(format!(
                    "Missing string patch plan for chunk {:016x}",
                    path_hash
                ))
            })?;
            let patched =
                strings::read_game_chunk(&self.game_dir, &plan.wad_rel_path, plan.chunk_hash)
                    .and_then(|base_bytes| plan.apply(&base_bytes));
            match patched {
                Ok(bytes) => {
                    resolved.insert(path_hash, Arc::from(bytes));
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
        mut wad_overrides: BTreeMap<Utf8PathBuf, HashMap<u64, PreparedOverride>>,
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

    /// Write one overlay WAD, by tail rewrite when it was planned and by full
    /// rebuild otherwise.
    fn patch_one_wad(&self, work: WadWork) -> Result<PatchedWad> {
        let WadWork {
            relative_path,
            mut overrides,
            rewrite,
        } = work;
        let dst_wad_path = self.overlay_root.join(&relative_path);
        let override_hashes: HashSet<u64> = overrides.keys().copied().collect();

        let stats = match rewrite {
            Some(rewrite) => {
                // An override whose bytes never resolved - a stringtable patch
                // that failed to apply, say - is dropped rather than fatal, and
                // its chunk falls back to the entry the source region holds.
                // That is what the full-rebuild path does with it too.
                let tail_hashes: Vec<u64> = rewrite
                    .tail_hashes
                    .iter()
                    .copied()
                    .filter(|hash| override_hashes.contains(hash))
                    .collect();

                tracing::info!(
                    "Rewriting WAD tail dst={} overrides={}",
                    dst_wad_path,
                    tail_hashes.len()
                );
                let mut stats = rewrite_wad_tail(
                    &dst_wad_path,
                    &rewrite.record.layout,
                    &rewrite.base_entries,
                    &tail_hashes,
                    |hash| take_override(&mut overrides, hash),
                )?;
                // The planner proved this source identity before choosing the
                // in-place path; the rewrite itself never opens the game WAD.
                stats.source = Some(rewrite.record.source);
                stats
            }
            None => {
                let src_wad_path = self.game_dir.join(&relative_path);
                tracing::info!(
                    "Patching WAD src={} dst={} overrides={}",
                    src_wad_path,
                    dst_wad_path,
                    override_hashes.len()
                );
                build_patched_wad(&src_wad_path, &dst_wad_path, &override_hashes, |hash| {
                    take_override(&mut overrides, hash)
                })?
            }
        };

        Ok(PatchedWad {
            relative_path,
            path: dst_wad_path,
            stats,
        })
    }
}

/// Hand one prepared override to a writer, releasing it from the WAD's map so
/// its bytes are freed as soon as the last WAD holding them has written it.
fn take_override(
    overrides: &mut HashMap<u64, PreparedOverride>,
    path_hash: u64,
) -> Result<PreparedOverride> {
    overrides
        .remove(&path_hash)
        .ok_or_else(|| Error::Other(format!("Missing override data for hash {path_hash:016x}")))
}

/// One WAD's share of a parallel patch pass.
struct WadWork {
    relative_path: Utf8PathBuf,
    overrides: HashMap<u64, PreparedOverride>,
    /// `Some` when this WAD passed the [tail-rewrite](super::incremental)
    /// preconditions, in which case its file is kept and only its tail rewritten.
    rewrite: Option<TailRewrite>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_with_content_hash(content_hash: u64) -> OverrideMeta {
        OverrideMeta {
            content_hash,
            uncompressed_size: 0,
            source: OverrideSource::Raw {
                mod_id: "m".to_string(),
                rel_path: Utf8PathBuf::from("assets/x.bin"),
            },
            fallback_wad: None,
            unlocalized_wad: None,
            linked_bins: Vec::new(),
        }
    }

    /// Two chunks with the same content must end up sharing one compressed
    /// buffer: that is what makes every copy of a cross-WAD chunk carry the
    /// same compressed checksum, which the game validates.
    #[test]
    fn identical_content_is_compressed_once() {
        let bytes: ResolvedChunk = Arc::from(b"the same asset, twice".repeat(64).as_slice());
        let resolved = HashMap::from([(0xAAAA_u64, bytes.clone()), (0xBBBB_u64, bytes)]);
        let all_meta = HashMap::from([
            (0xAAAA_u64, meta_with_content_hash(0xC0FFEE)),
            (0xBBBB_u64, meta_with_content_hash(0xC0FFEE)),
        ]);

        let prepared = prepare_overrides(resolved, &all_meta, HashMap::new()).unwrap();

        assert_eq!(prepared.len(), 2);
        let a = &prepared[&0xAAAA];
        let b = &prepared[&0xBBBB];
        assert!(
            std::ptr::eq(a.compressed(), b.compressed()),
            "equal content must share one compressed buffer, not two equal copies"
        );
        assert_eq!(a.checksum(), b.checksum());
    }

    /// Different content still gets its own compression, and the writer's TOC
    /// fields come straight off each prepared override.
    #[test]
    fn differing_content_is_compressed_separately() {
        let resolved = HashMap::from([
            (
                0xAAAA_u64,
                ResolvedChunk::from(b"first".repeat(64).as_slice()),
            ),
            (
                0xBBBB_u64,
                ResolvedChunk::from(b"second".repeat(64).as_slice()),
            ),
        ]);
        let all_meta = HashMap::from([
            (0xAAAA_u64, meta_with_content_hash(1)),
            (0xBBBB_u64, meta_with_content_hash(2)),
        ]);

        let prepared = prepare_overrides(resolved, &all_meta, HashMap::new()).unwrap();

        assert_ne!(prepared[&0xAAAA].checksum(), prepared[&0xBBBB].checksum());
        assert_eq!(prepared[&0xAAAA].uncompressed_size(), 5 * 64);
        assert_eq!(prepared[&0xBBBB].uncompressed_size(), 6 * 64);
    }

    /// A chunk the metadata pass never saw cannot be deduplicated against
    /// anything, but it must still be prepared rather than dropped.
    #[test]
    fn content_without_metadata_still_gets_prepared() {
        let resolved = HashMap::from([(0xAAAA_u64, ResolvedChunk::from(b"orphan".as_slice()))]);

        let prepared = prepare_overrides(resolved, &HashMap::new(), HashMap::new()).unwrap();

        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[&0xAAAA].uncompressed_size(), 6);
    }
}
