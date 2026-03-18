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
pub(crate) fn collect_single_mod_metadata(
    enabled_mod: &mut EnabledMod,
    game_index: &GameIndex,
    game_dir: &Utf8Path,
) -> Result<HashMap<u64, OverrideMeta>> {
    tracing::info!("Processing mod id={}", enabled_mod.id);

    let project = enabled_mod.content.mod_project()?;
    let mut layers = project.layers.clone();
    layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

    let mut mod_meta: HashMap<u64, OverrideMeta> = HashMap::new();

    for layer in &layers {
        if !enabled_mod.is_layer_active(&layer.name) {
            tracing::debug!(
                "Mod={} layer='{}' skipped (not in enabled_layers)",
                enabled_mod.id,
                layer.name,
            );
            continue;
        }

        let wad_names = enabled_mod.content.list_layer_wads(&layer.name)?;
        if wad_names.is_empty() {
            tracing::debug!(
                "Mod={} layer='{}' no WADs found, skipping",
                enabled_mod.id,
                layer.name,
            );
            continue;
        }

        tracing::info!("Mod={} layer='{}'", enabled_mod.id, layer.name);

        for wad_name in &wad_names {
            let fallback_wad = match game_index.find_wad(wad_name) {
                Ok(original_wad_path) => {
                    let relative_game_path = original_wad_path
                        .strip_prefix(game_dir)
                        .map_err(|_| format!("WAD path is not under Game/: {}", original_wad_path))?
                        .to_path_buf();

                    tracing::info!(
                        "WAD='{}' resolved original={} relative={}",
                        wad_name,
                        original_wad_path,
                        relative_game_path
                    );
                    Some(relative_game_path)
                }
                Err(Error::WadNotFound(_)) => {
                    tracing::warn!(
                        "Mod='{}' references unknown WAD '{}'; \
                         overrides will be routed by hash matching only",
                        enabled_mod.id,
                        wad_name
                    );
                    None
                }
                Err(other) => return Err(other),
            };

            let before = mod_meta.len();
            let override_files = enabled_mod
                .content
                .read_wad_overrides(&layer.name, wad_name)?;
            for (rel_path, bytes) in override_files {
                let path_hash = resolve_chunk_hash(&rel_path, &bytes)?;
                let content_hash = xxh3_64(&bytes);
                let uncompressed_size = bytes.len();
                // Drop bytes — only metadata is kept
                mod_meta.insert(
                    path_hash,
                    OverrideMeta {
                        content_hash,
                        uncompressed_size,
                        source: OverrideSource::LayerWad {
                            mod_id: enabled_mod.id.clone(),
                            layer: layer.name.clone(),
                            wad_name: wad_name.clone(),
                            rel_path,
                        },
                        fallback_wad: fallback_wad.clone(),
                    },
                );
            }
            let after = mod_meta.len();
            tracing::info!(
                "WAD='{}' overrides added={} total_mod_overrides={}",
                wad_name,
                after.saturating_sub(before),
                after
            );
        }
    }

    // Process RAW overrides — files identified by game asset path
    // that get routed to correct WADs via hash matching in distribute_override_hashes()
    let raw_overrides = enabled_mod.content.read_raw_overrides()?;
    if !raw_overrides.is_empty() {
        let before = mod_meta.len();
        for (rel_path, bytes) in raw_overrides {
            let path_hash = resolve_chunk_hash(&rel_path, &bytes)?;
            let content_hash = xxh3_64(&bytes);
            let uncompressed_size = bytes.len();
            mod_meta.insert(
                path_hash,
                OverrideMeta {
                    content_hash,
                    uncompressed_size,
                    source: OverrideSource::Raw {
                        mod_id: enabled_mod.id.clone(),
                        rel_path,
                    },
                    fallback_wad: None,
                },
            );
        }
        tracing::info!(
            "Mod={} RAW overrides added={}",
            enabled_mod.id,
            mod_meta.len().saturating_sub(before)
        );
    }

    Ok(mod_meta)
}

/// Filter out override metadata that should not be included in the overlay.
///
/// This performs two filtering passes:
/// 1. SubChunkTOC entries — always stripped to prevent game corruption.
/// 2. Lazy overrides — mod files identical to game originals, detected by
///    comparing pre-computed content hashes against game originals.
pub(crate) fn filter_override_metadata(
    all_meta: &mut HashMap<u64, OverrideMeta>,
    game_index: &GameIndex,
    game_dir: &Utf8Path,
) {
    // Filter out SubChunkTOC entries
    let blocked = game_index.subchunktoc_blocked();
    let before_filter = all_meta.len();
    all_meta.retain(|path_hash, _| {
        let dominated = blocked.contains(path_hash);
        if dominated {
            tracing::debug!("Filtered SubChunkTOC override: {:016x}", path_hash);
        }
        !dominated
    });
    let filtered_count = before_filter - all_meta.len();
    if filtered_count > 0 {
        tracing::info!(
            "Filtered {} SubChunkTOC override(s) from mod overrides",
            filtered_count
        );
    }

    // Filter out lazy overrides — mod files identical to game originals.
    // Use pre-computed content_hash from metadata instead of re-reading bytes.
    let override_hashes: HashSet<u64> = all_meta.keys().copied().collect();
    let content_hashes = game_index.compute_content_hashes_batch(game_dir, &override_hashes);

    let before_lazy = all_meta.len();
    all_meta.retain(|&path_hash, meta| {
        if let Some(&original_hash) = content_hashes.get(&path_hash) {
            if meta.content_hash == original_hash {
                tracing::debug!("Filtered lazy override: {:016x}", path_hash);
                return false;
            }
        }
        true
    });
    let lazy_count = before_lazy - all_meta.len();
    if lazy_count > 0 {
        tracing::info!(
            "Filtered {} lazy override(s) (identical to game originals)",
            lazy_count
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
    if let Some(fp) = fingerprint {
        if let Some(cached) = meta_cache.get_mod_meta(&enabled_mod.id, fp) {
            tracing::info!(
                "Mod={} cache hit (fingerprint {:016x}), {} overrides",
                enabled_mod.id,
                fp,
                cached.overrides.len()
            );
            return Ok(cached.reconstruct(&enabled_mod.id));
        }
    }

    // Cache miss — collect fresh metadata from mod content.
    tracing::info!("Mod={} cache miss, reading files", enabled_mod.id);
    let mod_meta = collect_single_mod_metadata(enabled_mod, game_index, game_dir)?;

    // Persist to cache for next build.
    if let Some(fp) = fingerprint {
        let cache_entry = CachedModMeta::from_override_meta(fp, &mod_meta);
        meta_cache.set_mod_meta(enabled_mod.id.clone(), cache_entry);
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
    ) -> Result<HashMap<u64, OverrideMeta>> {
        let game_dir = &self.game_dir;
        let meta_cache_path = self.state_dir.join("override_meta.bin");
        let game_fp = game_index.game_fingerprint();

        // Load persistent metadata cache (invalidated when game is patched)
        let mut meta_cache = OverrideMetaCache::load(&meta_cache_path, game_fp)
            .unwrap_or_else(|| OverrideMetaCache::new(game_fp));

        // Compute content fingerprints in parallel — each mod's fingerprint is
        // independent (filesystem stat calls or archive metadata).
        let fingerprints: Vec<Option<u64>> = self
            .enabled_mods
            .par_iter()
            .map(|m| m.cache_fingerprint())
            .collect();

        // For each mod: either use cache or collect fresh metadata.
        let mut per_mod_results: Vec<HashMap<u64, OverrideMeta>> =
            Vec::with_capacity(self.enabled_mods.len());

        for (idx, enabled_mod) in self.enabled_mods.iter_mut().enumerate() {
            let mod_meta = collect_or_cache_mod_metadata(
                enabled_mod,
                fingerprints[idx],
                &mut meta_cache,
                game_index,
                game_dir,
            )?;
            per_mod_results.push(mod_meta);
        }

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

        // Filter on metadata (SubChunkTOC + lazy)
        filter_override_metadata(&mut all_meta, game_index, game_dir);

        // Prune cache to only keep enabled mods
        let enabled_ids: Vec<String> = self.enabled_mods.iter().map(|m| m.id.clone()).collect();
        meta_cache.retain_mods(&enabled_ids);

        // Save updated cache (best-effort)
        if let Err(e) = meta_cache.save(&meta_cache_path) {
            tracing::warn!("Failed to save override meta cache: {}", e);
        }

        Ok(all_meta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_cache::CachedOverride;
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
                priority: i as i32,
                description: None,
                string_overrides: HashMap::new(),
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
                },
                CachedOverride {
                    path_hash: 0xABCD,
                    content_hash: 0xEF01,
                    uncompressed_size: 200,
                    target_wad: None,
                    source_layer: None,
                    source_wad_name: None,
                    source_rel_path: "assets/raw/file.bin".to_string(),
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
    }
}
