//! Between-passes distribution, Pass 2 resolve, and parallel WAD patching.
//!
//! Routes override hashes to affected WADs, partitions into rebuild/reuse sets,
//! re-reads bytes for WADs that need rebuilding, and patches WADs in parallel.

use super::*;
use crate::utils::compute_wad_fingerprint_from_meta;
use crate::wad_builder::build_patched_wad;
use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

impl OverlayBuilder {
    /// Distribute override path hashes to all affected WADs (lightweight).
    ///
    /// Returns a map of `relative_wad_path -> set of path_hashes`. No byte data
    /// is involved — only hash routing via the game index.
    pub(crate) fn distribute_override_hashes(
        &self,
        all_meta: &HashMap<u64, OverrideMeta>,
        game_index: &GameIndex,
    ) -> BTreeMap<Utf8PathBuf, HashSet<u64>> {
        let mut wad_hash_sets: BTreeMap<Utf8PathBuf, HashSet<u64>> = BTreeMap::new();
        let mut new_entry_count = 0usize;

        for (&path_hash, meta) in all_meta {
            if let Some(wad_paths) = game_index.find_wads_with_hash(path_hash) {
                for wad_path in wad_paths {
                    wad_hash_sets
                        .entry(wad_path.clone())
                        .or_default()
                        .insert(path_hash);
                }
            } else if let Some(fallback) = &meta.fallback_wad {
                wad_hash_sets
                    .entry(fallback.clone())
                    .or_default()
                    .insert(path_hash);
                new_entry_count += 1;
            }
        }

        if new_entry_count > 0 {
            tracing::info!(
                "Routed {} new entries (not in any game WAD) via mod directory structure",
                new_entry_count
            );
        }
        tracing::info!(
            "Distributed override hashes to {} affected WAD files",
            wad_hash_sets.len()
        );

        wad_hash_sets
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

            if can_incremental {
                if let Some(ref state) = prev_state {
                    if let Some(old_fp) = state.wad_fingerprint(wad_path_str) {
                        if old_fp == new_fp && overlay_wad.as_std_path().exists() {
                            tracing::debug!("Reusing WAD: {}", wad_path);
                            wads_to_reuse.push(wad_path);
                            continue;
                        }
                    }
                }
            }

            tracing::debug!("Need to rebuild WAD: {}", wad_path);
            wads_to_build.push(wad_path);
        }

        (wads_to_build, wads_to_reuse, new_wad_fingerprints)
    }

    /// Re-read override bytes for WADs that need rebuilding (pass 2).
    ///
    /// Groups needed overrides by source mod, reads each file once via the
    /// targeted `read_wad_override_file` / `read_raw_override_file` methods,
    /// wraps bytes in `Arc` for cross-WAD sharing, and distributes to per-WAD maps.
    pub(crate) fn resolve_overrides_for_wads(
        &mut self,
        wads_to_build: &[Utf8PathBuf],
        wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<u64>>,
        all_meta: &HashMap<u64, OverrideMeta>,
    ) -> Result<BTreeMap<Utf8PathBuf, HashMap<u64, SharedBytes>>> {
        // Collect all unique path_hashes needed across WADs to build
        let needed_hashes: HashSet<u64> = wads_to_build
            .iter()
            .filter_map(|wad_path| wad_hash_sets.get(wad_path))
            .flat_map(|hashes| hashes.iter().copied())
            .collect();

        if needed_hashes.is_empty() {
            return Ok(BTreeMap::new());
        }

        // Group needed hashes by source mod ID
        let mut by_mod: HashMap<&str, Vec<u64>> = HashMap::new();
        for &path_hash in &needed_hashes {
            if let Some(meta) = all_meta.get(&path_hash) {
                let mod_id = match &meta.source {
                    OverrideSource::LayerWad { mod_id, .. } => mod_id.as_str(),
                    OverrideSource::Raw { mod_id, .. } => mod_id.as_str(),
                };
                by_mod.entry(mod_id).or_default().push(path_hash);
            }
        }

        // Build mod ID -> index lookup for provider access
        let mod_id_to_index: HashMap<String, usize> = self
            .enabled_mods
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id.clone(), i))
            .collect();

        // Read bytes from each mod via targeted read methods
        let mut resolved: HashMap<u64, SharedBytes> = HashMap::with_capacity(needed_hashes.len());

        for (mod_id, hashes) in &by_mod {
            let Some(&idx) = mod_id_to_index.get(*mod_id) else {
                return Err(Error::Other(format!(
                    "Override source references unknown mod '{}'",
                    mod_id
                )));
            };
            let provider = &mut self.enabled_mods[idx].content;

            for &path_hash in hashes {
                let meta = &all_meta[&path_hash];
                let bytes = match &meta.source {
                    OverrideSource::LayerWad {
                        layer,
                        wad_name,
                        rel_path,
                        ..
                    } => provider.read_wad_override_file(layer, wad_name, rel_path)?,
                    OverrideSource::Raw { rel_path, .. } => {
                        provider.read_raw_override_file(rel_path)?
                    }
                };
                resolved.insert(path_hash, Arc::from(bytes));
            }
        }

        // Distribute resolved bytes to per-WAD maps
        let mut wad_overrides: BTreeMap<Utf8PathBuf, HashMap<u64, SharedBytes>> = BTreeMap::new();
        for wad_path in wads_to_build {
            if let Some(hashes) = wad_hash_sets.get(wad_path) {
                let mut per_wad: HashMap<u64, SharedBytes> = HashMap::with_capacity(hashes.len());
                for &hash in hashes {
                    if let Some(bytes) = resolved.get(&hash) {
                        per_wad.insert(hash, Arc::clone(bytes));
                    }
                }
                wad_overrides.insert(wad_path.clone(), per_wad);
            }
        }

        Ok(wad_overrides)
    }

    /// Patch WADs in parallel, emitting progress after each one completes.
    ///
    /// Consumes `wad_overrides` so each parallel task owns its data, enabling
    /// progressive deallocation as each WAD finishes patching.
    pub(crate) fn patch_wads_parallel(
        &self,
        wads_to_build: Vec<Utf8PathBuf>,
        mut wad_overrides: BTreeMap<Utf8PathBuf, HashMap<u64, SharedBytes>>,
    ) -> Result<Vec<Utf8PathBuf>> {
        let total_wads = wads_to_build.len() as u32;
        let completed = AtomicU32::new(0);
        let game_dir = &self.game_dir;
        let overlay_root = &self.overlay_root;
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

        // Extract per-WAD overrides so each parallel task owns its data.
        let per_wad_work: Vec<(Utf8PathBuf, HashMap<u64, SharedBytes>)> = wads_to_build
            .into_iter()
            .map(|path| {
                let overrides = wad_overrides.remove(&path).unwrap_or_default();
                (path, overrides)
            })
            .collect();
        drop(wad_overrides);

        per_wad_work
            .into_par_iter()
            .map(|(relative_game_path, mut overrides)| {
                let src_wad_path = game_dir.join(&relative_game_path);
                let dst_wad_path = overlay_root.join(&relative_game_path);

                tracing::info!(
                    "Patching WAD src={} dst={} overrides={}",
                    src_wad_path,
                    dst_wad_path,
                    overrides.len()
                );

                let override_hashes: HashSet<u64> = overrides.keys().copied().collect();
                build_patched_wad(&src_wad_path, &dst_wad_path, &override_hashes, |hash| {
                    overrides.remove(&hash).ok_or_else(|| {
                        Error::Other(format!("Missing override data for hash {:016x}", hash))
                    })
                })?;

                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                emit(OverlayProgress {
                    stage: OverlayStage::PatchingWad,
                    current_file: Some(
                        relative_game_path
                            .file_name()
                            .unwrap_or("unknown")
                            .to_string(),
                    ),
                    current: done,
                    total: total_wads,
                });

                Ok(dst_wad_path)
            })
            .collect()
    }
}
