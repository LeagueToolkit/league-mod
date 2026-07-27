//! Pass 1: Override metadata collection and caching.
//!
//! Collects lightweight metadata (hashes, sizes, source locations) from all
//! enabled mods. Uses a persistent metadata cache to skip unchanged mods entirely.

use super::*;
use crate::meta_cache::{CachedModMeta, OverrideMetaCache};
use crate::utils::resolve_chunk_hash;
use rayon::prelude::*;
use xxhash_rust::xxh3::xxh3_64;

/// Collect override metadata from a single mod (pass 1).
///
/// Reads all override files, computes their hashes and sizes, records source
/// locations for pass 2 re-reading, then drops the bytes. Returns lightweight
/// `OverrideMeta` entries instead of raw bytes.
///
/// The result is already filtered via [`filter_override_metadata`]: overrides
/// that can never reach the overlay (SubChunkTOC entries, mod-shipped
/// stringtable chunks, lazy copies byte-identical to game originals that are
/// not cross-WAD imports) are stripped. Filtering runs last so the WAD-routing
/// overlap heuristics still see the mod's full chunk set, and the filtered
/// result is what gets cached —
/// both filter inputs (mod content, game content) are covered by the cache's
/// invalidation keys.
pub(crate) fn collect_single_mod_metadata(
    enabled_mod: &mut EnabledMod,
    game_index: &GameIndex,
    game_dir: &Utf8Path,
) -> Result<HashMap<u64, OverrideMeta>> {
    tracing::info!("Processing mod id={}", enabled_mod.id);

    let project = enabled_mod.content.mod_project()?;
    let mut layers: Vec<_> = project.layers.iter().collect();
    layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

    let mut mod_meta: HashMap<u64, OverrideMeta> = HashMap::new();

    for layer in &layers {
        collect_layer_metadata(
            enabled_mod,
            &layer.name,
            game_index,
            game_dir,
            &mut mod_meta,
        )?;
    }

    collect_raw_metadata(enabled_mod, &mut mod_meta)?;

    route_unroutable_to_dominant_wad(enabled_mod, game_index, &mut mod_meta);

    filter_override_metadata(&mut mod_meta, game_index, game_dir);

    Ok(mod_meta)
}

/// Hash one override file into its chunk path hash and [`OverrideMeta`].
///
/// `fallback_wad` starts out `None`; callers backfill it once the WAD-level
/// routing target is known (the same post-hoc pattern as
/// [`route_unroutable_to_dominant_wad`]).
fn build_override_meta(source: OverrideSource, bytes: &[u8]) -> Result<(u64, OverrideMeta)> {
    let path_hash = resolve_chunk_hash(source.rel_path(), bytes)?;
    Ok((
        path_hash,
        OverrideMeta {
            content_hash: xxh3_64(bytes),
            uncompressed_size: bytes.len(),
            source,
            fallback_wad: None,
            linked_bins: crate::linked_bins::parse_linked_bins(bytes).unwrap_or_default(),
        },
    ))
}

/// Collect metadata from every WAD directory of one layer into `mod_meta`.
///
/// Skips the layer when it is not active for this mod or contains no WAD
/// directories.
fn collect_layer_metadata(
    enabled_mod: &mut EnabledMod,
    layer_name: &str,
    game_index: &GameIndex,
    game_dir: &Utf8Path,
    mod_meta: &mut HashMap<u64, OverrideMeta>,
) -> Result<()> {
    if !enabled_mod.is_layer_active(layer_name) {
        tracing::debug!(
            "Mod={} layer='{}' skipped (not in enabled_layers)",
            enabled_mod.id,
            layer_name,
        );
        return Ok(());
    }

    let wad_names = enabled_mod.content.list_layer_wads(layer_name)?;
    if wad_names.is_empty() {
        tracing::debug!(
            "Mod={} layer='{}' no WADs found, skipping",
            enabled_mod.id,
            layer_name,
        );
        return Ok(());
    }

    tracing::info!("Mod={} layer='{}'", enabled_mod.id, layer_name);

    for wad_name in &wad_names {
        collect_wad_dir_metadata(
            enabled_mod,
            layer_name,
            wad_name,
            game_index,
            game_dir,
            mod_meta,
        )?;
    }

    Ok(())
}

/// Collect metadata from one WAD target of a layer into `mod_meta`.
fn collect_wad_dir_metadata(
    enabled_mod: &mut EnabledMod,
    layer_name: &str,
    wad_name: &str,
    game_index: &GameIndex,
    game_dir: &Utf8Path,
    mod_meta: &mut HashMap<u64, OverrideMeta>,
) -> Result<()> {
    let before = mod_meta.len();
    let override_files = enabled_mod
        .content
        .read_wad_overrides(layer_name, wad_name)?;

    // Capture only the id: the provider box inside `EnabledMod` isn't `Sync`.
    let mod_id = &enabled_mod.id;
    let entries: Vec<(u64, OverrideMeta)> = override_files
        .into_par_iter()
        .map(|(rel_path, bytes)| {
            let source = OverrideSource::LayerWad {
                mod_id: mod_id.clone(),
                layer: layer_name.to_string(),
                wad_name: wad_name.to_string(),
                rel_path,
            };
            build_override_meta(source, &bytes)
        })
        .collect::<Result<_>>()?;

    let path_hashes: Vec<u64> = entries.iter().map(|(path_hash, _)| *path_hash).collect();
    let fallback_wad = resolve_fallback_wad(
        &enabled_mod.id,
        wad_name,
        &path_hashes,
        game_index,
        game_dir,
    )?;

    for (path_hash, mut meta) in entries {
        meta.fallback_wad = fallback_wad.clone();
        mod_meta.insert(path_hash, meta);
    }

    tracing::info!(
        "WAD='{}' overrides added={} total_mod_overrides={}",
        wad_name,
        mod_meta.len().saturating_sub(before),
        mod_meta.len()
    );

    Ok(())
}

/// Resolve the game WAD that a mod WAD target's overrides fall back to when
/// a chunk hash matches no game WAD.
///
/// A WAD name known to the game maps directly to its game-relative path. An
/// unknown name (e.g. "Spirit-Blossom-Rift.wad.client") is matched by chunk-hash
/// overlap against the game WADs. With no overlap either, returns `None` and
/// the overrides are routed by hash matching only.
fn resolve_fallback_wad(
    mod_id: &str,
    wad_name: &str,
    path_hashes: &[u64],
    game_index: &GameIndex,
    game_dir: &Utf8Path,
) -> Result<Option<Utf8PathBuf>> {
    match game_index.find_wad(wad_name) {
        Ok(original_wad_path) => {
            let relative_game_path = original_wad_path
                .strip_prefix(game_dir)
                .map_err(|_| {
                    Error::Other(format!("WAD path is not under Game/: {original_wad_path}"))
                })?
                .to_path_buf();

            tracing::info!(
                "WAD='{}' resolved original={} relative={}",
                wad_name,
                original_wad_path,
                relative_game_path
            );

            Ok(Some(relative_game_path))
        }
        Err(Error::WadNotFound(_)) => match game_index.find_best_matching_wad(path_hashes) {
            Some(best_wad) => {
                tracing::info!(
                    "Mod='{}' WAD '{}' not found in game; \
                         overlap detection matched to '{}'",
                    mod_id,
                    wad_name,
                    best_wad
                );

                Ok(Some(best_wad))
            }
            None => {
                tracing::warn!(
                    "Mod='{}' references unknown WAD '{}' with no overlapping \
                         game WAD; overrides will be routed by hash matching only",
                    mod_id,
                    wad_name
                );

                Ok(None)
            }
        },
        Err(other) => Err(other),
    }
}

/// Collect RAW overrides into `mod_meta` — files identified by game asset path
/// that get routed to the correct WADs via hash matching in
/// [`OverlayBuilder::distribute_override_hashes`].
fn collect_raw_metadata(
    enabled_mod: &mut EnabledMod,
    mod_meta: &mut HashMap<u64, OverrideMeta>,
) -> Result<()> {
    let raw_overrides = enabled_mod.content.read_raw_overrides()?;
    if raw_overrides.is_empty() {
        return Ok(());
    }

    let before = mod_meta.len();
    for (rel_path, bytes) in raw_overrides {
        let (path_hash, meta) = build_override_meta(
            OverrideSource::Raw {
                mod_id: enabled_mod.id.clone(),
                rel_path,
            },
            &bytes,
        )?;

        mod_meta.insert(path_hash, meta);
    }

    tracing::info!(
        "Mod={} RAW overrides added={}",
        enabled_mod.id,
        mod_meta.len().saturating_sub(before)
    );

    Ok(())
}

/// Route any overrides that still have no fallback target — e.g. RAW files
/// introducing brand-new assets, or WAD-layer overrides whose own chunks didn't
/// overlap any game WAD — to the game WAD that the majority of THIS mod's
/// chunks map to.
///
/// Without this they would be dropped at distribution time; placing them
/// alongside the bulk of the mod's content is the same overlap heuristic
/// already used to resolve unknown WAD names.
fn route_unroutable_to_dominant_wad(
    enabled_mod: &EnabledMod,
    game_index: &GameIndex,
    mod_meta: &mut HashMap<u64, OverrideMeta>,
) {
    let unroutable = mod_meta
        .values()
        .filter(|meta| meta.fallback_wad.is_none())
        .count();
    if unroutable == 0 {
        return;
    }

    let all_hashes: Vec<u64> = mod_meta.keys().copied().collect();
    let Some(dominant_wad) = game_index.find_best_matching_wad(&all_hashes) else {
        return;
    };

    tracing::info!(
        "Mod={} routing {} override(s) with no WAD match to dominant WAD '{}'",
        enabled_mod.id,
        unroutable,
        dominant_wad
    );

    for meta in mod_meta.values_mut() {
        if meta.fallback_wad.is_none() {
            meta.fallback_wad = Some(dominant_wad.clone());
        }
    }
}

/// Filter out override metadata that should not be included in the overlay.
///
/// This performs three filtering passes:
/// 1. SubChunkTOC entries — always stripped to prevent game corruption.
/// 2. Stringtable chunks — mods must not ship `lol.stringtable` overrides;
///    layer `string_overrides` are the only supported way to modify game
///    strings, and the game's own stringtable is always the patch base.
/// 3. Lazy overrides — mod files identical to game originals, detected by
///    comparing pre-computed content hashes against game originals. Identical
///    files that are cross-WAD imports (shipped under a WAD directory whose
///    game WAD lacks the chunk) survive this pass and route like any other
///    override: to the declared WAD and every game WAD holding the chunk.
///
/// Runs per-mod at the end of [`collect_single_mod_metadata`], before caching
/// and before [`super::ModWadReport`]s are computed, so reports, cache, and
/// the overlay build all see the same effective override set
pub(crate) fn filter_override_metadata(
    all_meta: &mut HashMap<u64, OverrideMeta>,
    game_index: &GameIndex,
    game_dir: &Utf8Path,
) {
    if all_meta.is_empty() {
        return;
    }

    let subchunktoc_blocked = game_index.subchunktoc_blocked();
    let stringtable_blocked = crate::strings::blocked_stringtable_hashes(game_index);

    let mut subchunktoc_count = 0usize;
    all_meta.retain(|path_hash, meta| {
        if subchunktoc_blocked.contains(path_hash) {
            tracing::debug!("Filtered SubChunkTOC override: {:016x}", path_hash);
            subchunktoc_count += 1;

            return false;
        }
        if stringtable_blocked.contains(path_hash) {
            tracing::warn!(
                "Mod '{}' ships a stringtable chunk ('{}'); rejecting it - use layer \
                 string_overrides to modify game strings",
                meta.source.mod_id(),
                meta.source.rel_path(),
            );

            return false;
        }
        true
    });
    if subchunktoc_count > 0 {
        tracing::info!(
            "Filtered {} SubChunkTOC override(s) from mod overrides",
            subchunktoc_count
        );
    }

    // Filter out lazy overrides — mod files identical to game originals.
    // Use pre-computed content_hash from metadata instead of re-reading bytes.
    //
    // Exception: a byte-identical file shipped under a WAD directory whose game
    // WAD does NOT contain the chunk is a cross-WAD import (the mod bundles an
    // asset from another WAD so it is loadable from its own target WAD). Those
    // are kept and routed like modified overrides.
    let override_hashes: HashSet<u64> = all_meta.keys().copied().collect();
    let content_hashes = game_index.compute_content_hashes_batch(game_dir, &override_hashes);

    let mut lazy_count = 0usize;
    let mut import_count = 0usize;
    all_meta.retain(|&path_hash, meta| {
        let Some(&original_hash) = content_hashes.get(&path_hash) else {
            return true;
        };
        if meta.content_hash != original_hash {
            return true;
        }

        if meta.is_cross_wad_import(path_hash, game_index) {
            tracing::debug!(
                "Keeping byte-identical override {:016x} as cross-WAD import into '{}'",
                path_hash,
                meta.fallback_wad.as_deref().unwrap_or(Utf8Path::new("?")),
            );

            import_count += 1;

            return true;
        }

        tracing::debug!("Filtered lazy override: {:016x}", path_hash);
        lazy_count += 1;
        false
    });
    if lazy_count > 0 {
        tracing::info!(
            "Filtered {} lazy override(s) (identical to game originals)",
            lazy_count
        );
    }
    if import_count > 0 {
        tracing::info!(
            "Kept {} byte-identical cross-WAD import(s), fanned out to their declared \
             and source WADs",
            import_count
        );
    }
}

/// Try the metadata cache for a single mod; on miss, collect fresh metadata and
/// update the cache.
fn collect_or_cache_mod_metadata(
    enabled_mod: &mut EnabledMod,
    fingerprint: Option<u64>,
    meta_cache: &mut OverrideMetaCache,
    game_index: &GameIndex,
    game_dir: &Utf8Path,
) -> Result<HashMap<u64, OverrideMeta>> {
    // Cache hit — reconstruct from cached data without reading any files.
    if let Some(fp) = fingerprint
        && let Some(cached) = meta_cache.get_mod_meta(&enabled_mod.id, fp)
    {
        tracing::info!(
            "Mod={} cache hit (fingerprint {:016x}), {} overrides",
            enabled_mod.id,
            fp,
            cached.overrides.len()
        );

        return Ok(cached.reconstruct(&enabled_mod.id));
    }

    // Cache miss — collect fresh metadata from mod content.
    tracing::info!("Mod={} cache miss, reading files", enabled_mod.id);
    let mod_meta = collect_single_mod_metadata(enabled_mod, game_index, game_dir)?;

    // Persist to cache for next build.
    if let Some(fp) = fingerprint {
        meta_cache.set_mod_meta(
            enabled_mod.id.clone(),
            CachedModMeta::from_override_meta(fp, &mod_meta),
        );
    }

    Ok(mod_meta)
}

impl OverlayBuilder {
    /// Collect override metadata from all mods (pass 1).
    ///
    /// Uses the persistent metadata cache to skip re-reading unchanged mods.
    /// For cache misses, reads files, computes hashes, records source locations,
    /// and drops the bytes immediately.
    ///
    /// Returns `path_hash -> OverrideMeta` for all overrides across all mods.
    pub(crate) fn collect_all_override_metadata(
        &mut self,
        game_index: &GameIndex,
        fingerprints: &[Option<u64>],
    ) -> Result<(HashMap<u64, OverrideMeta>, Vec<ModWadReport>)> {
        debug_assert_eq!(fingerprints.len(), self.enabled_mods.len());

        let game_dir = &self.game_dir;
        let meta_cache_path = self.state_dir.join("override_meta.bin");
        let game_fp = game_index.game_fingerprint();

        // Load persistent metadata cache (invalidated when game is patched)
        let mut meta_cache = OverrideMetaCache::load(&meta_cache_path, game_fp)
            .unwrap_or_else(|| OverrideMetaCache::new(game_fp));

        // For each mod: either use cache or collect fresh metadata.
        let mut per_mod_results: Vec<HashMap<u64, OverrideMeta>> =
            Vec::with_capacity(self.enabled_mods.len());

        for (idx, enabled_mod) in self.enabled_mods.iter_mut().enumerate() {
            per_mod_results.push(collect_or_cache_mod_metadata(
                enabled_mod,
                fingerprints[idx],
                &mut meta_cache,
                game_index,
                game_dir,
            )?);
        }

        let mod_wad_reports =
            self.build_mod_wad_reports(&per_mod_results, fingerprints, game_index);

        // Merge in reverse order (last mod first → first mod wins via last-writer-wins)
        let mut all_meta: HashMap<u64, OverrideMeta> = HashMap::new();

        for mod_meta in per_mod_results.into_iter().rev() {
            for (hash, meta) in mod_meta {
                all_meta.insert(hash, meta);
            }
        }

        tracing::info!(
            "Collected {} unique override metadata entries from all mods",
            all_meta.len()
        );

        // Prune cache to only keep enabled mods
        let enabled_ids: Vec<String> = self.enabled_mods.iter().map(|m| m.id.clone()).collect();
        meta_cache.retain_mods(&enabled_ids);

        if let Err(e) = meta_cache.save(&meta_cache_path) {
            tracing::warn!("Failed to save override meta cache: {}", e);
        }

        Ok((all_meta, mod_wad_reports))
    }

    /// Pair each enabled mod with its un-merged metadata and turn it into a
    /// [`ModWadReport`].
    ///
    /// `per_mod_results` and `fingerprints` MUST be parallel to `self.enabled_mods`.
    fn build_mod_wad_reports(
        &self,
        per_mod_results: &[HashMap<u64, OverrideMeta>],
        fingerprints: &[Option<u64>],
        game_index: &GameIndex,
    ) -> Vec<ModWadReport> {
        self.enabled_mods
            .iter()
            .zip(per_mod_results.iter())
            .zip(fingerprints.iter())
            .map(|((enabled_mod, mod_meta), fp)| {
                ModWadReport::from_meta(enabled_mod.id.clone(), mod_meta, *fp, game_index)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_cache::CachedOverride;
    use indexmap::IndexMap;
    use ltk_mod_project::{ModProject, ModProjectLayer};
    use std::sync::{Arc, Mutex};

    /// Mock content provider that tracks which layers are queried.
    struct MockModContent {
        layers: Vec<ModProjectLayer>,
        queried_layers: Arc<Mutex<Vec<String>>>,
    }

    impl ModContentProvider for MockModContent {
        fn mod_project(&mut self) -> Result<ModProject> {
            Ok(ModProject {
                name: "test-mod".to_string(),
                display_name: "Test Mod".to_string(),
                version: "1.0.0".to_string(),
                description: "test".to_string(),
                authors: vec![],
                license: None,
                tags: vec![],
                champions: vec![],
                maps: vec![],
                transformers: vec![],
                layers: self.layers.clone(),
                thumbnail: None,
            })
        }

        fn list_layer_wads(&mut self, layer: &str) -> Result<Vec<String>> {
            self.queried_layers.lock().unwrap().push(layer.to_string());
            // Return empty so we don't need a real GameIndex
            Ok(vec![])
        }

        fn read_wad_overrides(
            &mut self,
            _layer: &str,
            _wad_name: &str,
        ) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
            Ok(vec![])
        }

        fn read_wad_override_file(
            &mut self,
            _layer: &str,
            _wad_name: &str,
            _rel_path: &Utf8Path,
        ) -> Result<Vec<u8>> {
            Ok(vec![])
        }

        fn read_raw_override_file(&mut self, _rel_path: &Utf8Path) -> Result<Vec<u8>> {
            Ok(vec![])
        }
    }

    fn make_layers(names: &[&str]) -> Vec<ModProjectLayer> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| ModProjectLayer {
                name: name.to_string(),
                display_name: None,
                priority: i as i32,
                description: None,
                string_overrides: IndexMap::new(),
            })
            .collect()
    }

    #[test]
    fn test_enabled_layers_filters_correctly() {
        let queried = Arc::new(Mutex::new(Vec::new()));

        // Build an empty GameIndex from a temp directory with DATA/FINAL
        let tmp = tempfile::tempdir().unwrap();
        let game_dir_std = tmp.path().join("Game");
        std::fs::create_dir_all(game_dir_std.join("DATA").join("FINAL")).unwrap();
        let game_dir = Utf8Path::from_path(&game_dir_std).unwrap();
        let game_index = GameIndex::build(game_dir).unwrap();

        // With enabled_layers = None, all layers should be queried
        let mut mod_all = EnabledMod {
            id: "mod1".to_string(),
            content: Box::new(MockModContent {
                layers: make_layers(&["base", "high_res", "extras"]),
                queried_layers: Arc::clone(&queried),
            }),
            enabled_layers: None,
        };
        let _ = collect_single_mod_metadata(&mut mod_all, &game_index, game_dir);
        let all_queried: Vec<String> = queried.lock().unwrap().drain(..).collect();
        assert_eq!(all_queried, vec!["base", "high_res", "extras"]); // sorted by priority then name

        // With enabled_layers = Some({"extras"}), base + extras should be queried
        // (base is always included even if not in the set)
        let mut mod_filtered = EnabledMod {
            id: "mod2".to_string(),
            content: Box::new(MockModContent {
                layers: make_layers(&["base", "high_res", "extras"]),
                queried_layers: Arc::clone(&queried),
            }),
            enabled_layers: Some(HashSet::from(["extras".to_string()])),
        };
        let _ = collect_single_mod_metadata(&mut mod_filtered, &game_index, game_dir);
        let filtered_queried: Vec<String> = queried.lock().unwrap().drain(..).collect();
        assert_eq!(filtered_queried, vec!["base", "extras"]);
        // "high_res" should NOT appear, but "base" is always included
    }

    struct OverrideMockContent {
        layers: Vec<ModProjectLayer>,
        /// WAD name -> list of (rel_path, bytes) overrides to return.
        wad_overrides: HashMap<String, Vec<(Utf8PathBuf, Vec<u8>)>>,
    }

    impl ModContentProvider for OverrideMockContent {
        fn mod_project(&mut self) -> Result<ModProject> {
            Ok(ModProject {
                name: "test-mod".to_string(),
                display_name: "Test Mod".to_string(),
                version: "1.0.0".to_string(),
                description: "test".to_string(),
                authors: vec![],
                license: None,
                tags: vec![],
                champions: vec![],
                maps: vec![],
                transformers: vec![],
                layers: self.layers.clone(),
                thumbnail: None,
            })
        }

        fn list_layer_wads(&mut self, _layer: &str) -> Result<Vec<String>> {
            Ok(self.wad_overrides.keys().cloned().collect())
        }

        fn read_wad_overrides(
            &mut self,
            _layer: &str,
            wad_name: &str,
        ) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
            Ok(self
                .wad_overrides
                .get(wad_name)
                .cloned()
                .unwrap_or_default())
        }

        fn read_wad_override_file(
            &mut self,
            _layer: &str,
            _wad_name: &str,
            _rel_path: &Utf8Path,
        ) -> Result<Vec<u8>> {
            Ok(vec![])
        }

        fn read_raw_override_file(&mut self, _rel_path: &Utf8Path) -> Result<Vec<u8>> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_unknown_wad_uses_overlap_fallback() {
        let mut hash_index = HashMap::new();
        for h in [0xAAAA_u64, 0xBBBB] {
            hash_index
                .entry(h)
                .or_insert_with(Vec::new)
                .push(Utf8PathBuf::from("DATA/FINAL/Maps/MapA.wad.client"));
        }

        hash_index
            .entry(0xAAAA)
            .or_insert_with(Vec::new)
            .push(Utf8PathBuf::from("DATA/FINAL/Maps/MapB.wad.client"));

        let game_index = GameIndex {
            wad_index: HashMap::new(),
            hash_index,
            game_fingerprint: 0,
            subchunktoc_blocked: HashSet::new(),
        };

        let mut wad_overrides = HashMap::new();
        wad_overrides.insert(
            "Unknown.wad.client".to_string(),
            vec![
                (
                    Utf8PathBuf::from("000000000000aaaa.bin"),
                    b"data_a".to_vec(),
                ),
                (
                    Utf8PathBuf::from("000000000000bbbb.bin"),
                    b"data_b".to_vec(),
                ),
            ],
        );

        let tmp = tempfile::tempdir().unwrap();
        let game_dir_std = tmp.path().join("Game");
        std::fs::create_dir_all(game_dir_std.join("DATA").join("FINAL")).unwrap();
        let game_dir = Utf8Path::from_path(&game_dir_std).unwrap();

        let mut enabled_mod = EnabledMod {
            id: "overlap-mod".to_string(),
            content: Box::new(OverrideMockContent {
                layers: make_layers(&["base"]),
                wad_overrides,
            }),
            enabled_layers: None,
        };

        let meta = collect_single_mod_metadata(&mut enabled_mod, &game_index, game_dir).unwrap();
        assert_eq!(meta.len(), 2);

        let entry = &meta[&0xAAAA];
        assert_eq!(
            entry.fallback_wad.as_deref(),
            Some(Utf8Path::new("DATA/FINAL/Maps/MapA.wad.client")),
            "Overlap detection should select the WAD with the most matching hashes"
        );

        let entry_b = &meta[&0xBBBB];
        assert_eq!(
            entry_b.fallback_wad.as_deref(),
            Some(Utf8Path::new("DATA/FINAL/Maps/MapA.wad.client")),
        );
    }

    #[test]
    fn test_unknown_wad_no_overlap_sets_fallback_none() {
        let game_index = GameIndex {
            wad_index: HashMap::new(),
            hash_index: HashMap::new(),
            game_fingerprint: 0,
            subchunktoc_blocked: HashSet::new(),
        };

        let mut wad_overrides = HashMap::new();
        wad_overrides.insert(
            "Nonexistent.wad.client".to_string(),
            vec![(Utf8PathBuf::from("000000000000cccc.bin"), b"data".to_vec())],
        );

        let tmp = tempfile::tempdir().unwrap();
        let game_dir_std = tmp.path().join("Game");
        std::fs::create_dir_all(game_dir_std.join("DATA").join("FINAL")).unwrap();
        let game_dir = Utf8Path::from_path(&game_dir_std).unwrap();

        let mut enabled_mod = EnabledMod {
            id: "no-overlap-mod".to_string(),
            content: Box::new(OverrideMockContent {
                layers: make_layers(&["base"]),
                wad_overrides,
            }),
            enabled_layers: None,
        };

        let meta = collect_single_mod_metadata(&mut enabled_mod, &game_index, game_dir).unwrap();

        assert_eq!(meta.len(), 1);
        assert!(
            meta[&0xCCCC].fallback_wad.is_none(),
            "When no overlapping WAD is found, fallback_wad should be None"
        );
    }

    #[test]
    fn test_unroutable_override_routed_to_dominant_wad() {
        // Game has one chunk (0xAAAA) living in Ahri.wad.
        let mut hash_index = HashMap::new();
        hash_index.insert(
            0xAAAA_u64,
            vec![Utf8PathBuf::from("DATA/FINAL/Champions/Ahri.wad.client")],
        );
        let game_index = GameIndex {
            wad_index: HashMap::new(),
            hash_index,
            game_fingerprint: 0,
            subchunktoc_blocked: HashSet::new(),
        };

        // The mod overrides a known chunk (0xAAAA, maps to Ahri.wad) and ships a brand-new
        // asset (0xCCCC) under an unknown WAD that overlaps nothing on its own.
        let mut wad_overrides = HashMap::new();
        wad_overrides.insert(
            "Ahri.wad.client".to_string(),
            vec![(Utf8PathBuf::from("000000000000aaaa.bin"), b"a".to_vec())],
        );
        wad_overrides.insert(
            "BrandNew.wad.client".to_string(),
            vec![(Utf8PathBuf::from("000000000000cccc.bin"), b"c".to_vec())],
        );

        let tmp = tempfile::tempdir().unwrap();
        let game_dir_std = tmp.path().join("Game");
        std::fs::create_dir_all(game_dir_std.join("DATA").join("FINAL")).unwrap();
        let game_dir = Utf8Path::from_path(&game_dir_std).unwrap();

        let mut enabled_mod = EnabledMod {
            id: "dominant-mod".to_string(),
            content: Box::new(OverrideMockContent {
                layers: make_layers(&["base"]),
                wad_overrides,
            }),
            enabled_layers: None,
        };

        let meta = collect_single_mod_metadata(&mut enabled_mod, &game_index, game_dir).unwrap();

        let ahri = Utf8Path::new("DATA/FINAL/Champions/Ahri.wad.client");
        assert_eq!(meta[&0xAAAA].fallback_wad.as_deref(), Some(ahri));
        assert_eq!(
            meta[&0xCCCC].fallback_wad.as_deref(),
            Some(ahri),
            "Override with no WAD match should be routed to the mod's dominant WAD"
        );
    }

    #[test]
    fn test_filter_rejects_mod_shipped_stringtable_chunks() {
        let mut wad_index = HashMap::new();
        wad_index.insert(
            "global.en_us.wad.client".to_string(),
            vec![Utf8PathBuf::from(
                "DATA/FINAL/Localized/Global.en_US.wad.client",
            )],
        );
        let game_index = GameIndex {
            wad_index,
            hash_index: HashMap::new(),
            game_fingerprint: 0,
            subchunktoc_blocked: HashSet::new(),
        };

        let raw_meta = |rel_path: &str| OverrideMeta {
            content_hash: 1,
            uncompressed_size: 1,
            source: OverrideSource::Raw {
                mod_id: "strings-shipper".to_string(),
                rel_path: Utf8PathBuf::from(rel_path),
            },
            fallback_wad: None,
            linked_bins: Vec::new(),
        };

        let stringtable_hash = crate::strings::stringtable_chunk_hash("en_us");
        let mut all_meta = HashMap::new();
        all_meta.insert(
            stringtable_hash,
            raw_meta("data/menu/en_us/lol.stringtable"),
        );
        all_meta.insert(0xAAAA, raw_meta("assets/other.bin"));

        let tmp = tempfile::tempdir().unwrap();
        let game_dir_std = tmp.path().join("Game");
        std::fs::create_dir_all(game_dir_std.join("DATA").join("FINAL")).unwrap();
        let game_dir = Utf8Path::from_path(&game_dir_std).unwrap();

        filter_override_metadata(&mut all_meta, &game_index, game_dir);

        assert!(
            !all_meta.contains_key(&stringtable_hash),
            "Mod-shipped stringtable chunks must be rejected"
        );
        assert!(all_meta.contains_key(&0xAAAA));
    }

    #[test]
    fn test_collect_single_mod_metadata_returns_filtered_overrides() {
        let mut hash_index = HashMap::new();
        for h in [0xAAAA_u64, 0xB10C] {
            hash_index
                .entry(h)
                .or_insert_with(Vec::new)
                .push(Utf8PathBuf::from("DATA/FINAL/Maps/MapA.wad.client"));
        }
        let game_index = GameIndex {
            wad_index: HashMap::new(),
            hash_index,
            game_fingerprint: 0,
            subchunktoc_blocked: HashSet::from([0xB10C_u64]),
        };

        let mut wad_overrides = HashMap::new();
        wad_overrides.insert(
            "MapA.wad.client".to_string(),
            vec![
                (Utf8PathBuf::from("000000000000aaaa.bin"), b"keep".to_vec()),
                (
                    Utf8PathBuf::from("000000000000b10c.bin"),
                    b"subchunktoc".to_vec(),
                ),
            ],
        );

        let tmp = tempfile::tempdir().unwrap();
        let game_dir_std = tmp.path().join("Game");
        std::fs::create_dir_all(game_dir_std.join("DATA").join("FINAL")).unwrap();
        let game_dir = Utf8Path::from_path(&game_dir_std).unwrap();

        let mut enabled_mod = EnabledMod {
            id: "filtered-mod".to_string(),
            content: Box::new(OverrideMockContent {
                layers: make_layers(&["base"]),
                wad_overrides,
            }),
            enabled_layers: None,
        };

        let meta = collect_single_mod_metadata(&mut enabled_mod, &game_index, game_dir).unwrap();

        assert_eq!(
            meta.keys().copied().collect::<Vec<_>>(),
            vec![0xAAAA],
            "SubChunkTOC overrides must already be stripped from collected metadata, \
             so reports, the metadata cache, and the overlay build all agree"
        );
    }

    #[test]
    fn test_lazy_filter_keeps_cross_wad_imports() {
        use ltk_wad::{WadBuilder, WadChunkBuilder, WadChunkCompression};
        use std::io::{Cursor, Write};

        const IMPORT_PATH: &str = "assets/characters/ahri/vfx.tex";
        const LAZY_PATH: &str = "assets/characters/ahri/vfx2.tex";
        let import_hash = resolve_chunk_hash(Utf8Path::new(IMPORT_PATH), b"").unwrap();
        let lazy_hash = resolve_chunk_hash(Utf8Path::new(LAZY_PATH), b"").unwrap();

        // Real game WAD on disk (the lazy filter decompresses originals from it).
        let tmp = tempfile::tempdir().unwrap();
        let game_dir_std = tmp.path().join("Game");
        let champions_std = game_dir_std.join("DATA").join("FINAL").join("Champions");
        std::fs::create_dir_all(&champions_std).unwrap();
        let game_dir = Utf8Path::from_path(&game_dir_std).unwrap();

        let mut cursor = Cursor::new(Vec::new());
        WadBuilder::default()
            .with_chunk(
                WadChunkBuilder::default()
                    .with_path(IMPORT_PATH)
                    .with_force_compression(WadChunkCompression::None),
            )
            .with_chunk(
                WadChunkBuilder::default()
                    .with_path(LAZY_PATH)
                    .with_force_compression(WadChunkCompression::None),
            )
            .build_to_writer(&mut cursor, |hash, writer| {
                writer.write_all(if hash == import_hash {
                    b"IMPORT_ORIGINAL"
                } else {
                    b"LAZY_ORIGINALX"
                })?;
                Ok(())
            })
            .unwrap();
        std::fs::write(champions_std.join("Ahri.wad.client"), cursor.into_inner()).unwrap();

        let ahri_rel = Utf8PathBuf::from("DATA/FINAL/Champions/Ahri.wad.client");
        let aatrox_rel = Utf8PathBuf::from("DATA/FINAL/Champions/Aatrox.wad.client");
        let mut wad_index = HashMap::new();
        wad_index.insert(
            "ahri.wad.client".to_string(),
            vec![game_dir.join(&ahri_rel)],
        );
        wad_index.insert(
            "aatrox.wad.client".to_string(),
            vec![game_dir.join(&aatrox_rel)],
        );
        let mut hash_index = HashMap::new();
        hash_index.insert(import_hash, vec![ahri_rel.clone()]);
        hash_index.insert(lazy_hash, vec![ahri_rel.clone()]);
        let game_index = GameIndex {
            wad_index,
            hash_index,
            game_fingerprint: 0,
            subchunktoc_blocked: HashSet::new(),
        };

        // The mod ships byte-identical copies of both Ahri originals: one under
        // the Aatrox WAD dir (cross-WAD import), one under Ahri's own dir (lazy).
        let mut wad_overrides = HashMap::new();
        wad_overrides.insert(
            "Aatrox.wad.client".to_string(),
            vec![(Utf8PathBuf::from(IMPORT_PATH), b"IMPORT_ORIGINAL".to_vec())],
        );
        wad_overrides.insert(
            "Ahri.wad.client".to_string(),
            vec![(Utf8PathBuf::from(LAZY_PATH), b"LAZY_ORIGINALX".to_vec())],
        );

        let mut enabled_mod = EnabledMod {
            id: "import-mod".to_string(),
            content: Box::new(OverrideMockContent {
                layers: make_layers(&["base"]),
                wad_overrides,
            }),
            enabled_layers: None,
        };

        let meta = collect_single_mod_metadata(&mut enabled_mod, &game_index, game_dir).unwrap();

        assert!(
            !meta.contains_key(&lazy_hash),
            "byte-identical override under the chunk's own WAD is lazy and must be stripped"
        );
        let import = meta.get(&import_hash).expect(
            "byte-identical override under another WAD dir is a cross-WAD import and must be kept",
        );
        assert_eq!(import.fallback_wad.as_deref(), Some(aatrox_rel.as_path()));
        assert_eq!(
            import.route_targets(import_hash, &game_index),
            vec![ahri_rel.as_path(), aatrox_rel.as_path()],
            "an identical import must fan out to the declared WAD and every WAD \
             holding the chunk, so all copies share one compressed checksum"
        );
    }

    #[test]
    fn test_reconstruct_from_cache() {
        let cached = CachedModMeta {
            content_fingerprint: 0xDEAD,
            overrides: vec![
                CachedOverride {
                    path_hash: 0x1234,
                    content_hash: 0x5678,
                    uncompressed_size: 100,
                    target_wad: Some("DATA/FINAL/test.wad.client".to_string()),
                    source_layer: Some("base".to_string()),
                    source_wad_name: Some("Test.wad.client".to_string()),
                    source_rel_path: "data/file.bin".to_string(),
                    linked_bins: vec!["data/characters/test/test.bin".to_string()],
                },
                CachedOverride {
                    path_hash: 0xABCD,
                    content_hash: 0xEF01,
                    uncompressed_size: 200,
                    target_wad: None,
                    source_layer: None,
                    source_wad_name: None,
                    source_rel_path: "assets/raw/file.bin".to_string(),
                    linked_bins: Vec::new(),
                },
            ],
        };

        let meta = cached.reconstruct("test-mod");
        assert_eq!(meta.len(), 2);
        assert_eq!(meta[&0x1234].content_hash, 0x5678);
        assert_eq!(meta[&0xABCD].content_hash, 0xEF01);
        assert_eq!(
            meta[&0x1234].fallback_wad.as_deref(),
            Some(Utf8Path::new("DATA/FINAL/test.wad.client"))
        );
        assert!(meta[&0xABCD].fallback_wad.is_none());
        assert_eq!(
            meta[&0x1234].linked_bins,
            vec!["data/characters/test/test.bin".to_string()]
        );
        assert!(meta[&0xABCD].linked_bins.is_empty());
    }
}
