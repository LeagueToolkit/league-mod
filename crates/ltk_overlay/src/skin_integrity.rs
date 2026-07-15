//! Base-skin correctness diagnostics over the built overlay WADs.
//!
//! Runs [`ltk_sanitize`]'s closed-world check on every champion WAD whose
//! `skin0.bin` was overridden by a mod, after the overlay WADs are written:
//! the base skin's mesh references must resolve inside the overlay WAD the
//! game will load, exactly as the in-game verifier asserts. Violations are
//! attributed to the mod that owns the winning `skin0.bin` override so the
//! manager can prompt the user about that specific broken mod instead of the
//! whole overlay failing at injection.
//!
//! [Baseline anomalies](ltk_sanitize::BaselineAnomaly) — the *original* game
//! WAD violating the check's assumptions — are never a mod diagnostic; they
//! are logged (`tracing::error`, stable `base-skin baseline anomaly` prefix)
//! and dropped, since they point at a corrupt game install or at an
//! assumption a game patch has invalidated.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;

use camino::{Utf8Path, Utf8PathBuf};
pub use ltk_sanitize::SkinPolicy;
use ltk_sanitize::{
    ChunkSource, SkinCheckOutcome, VirtualMerge, WadChunkSource, champion_from_wad_path,
    check_base_skin, skin0_bin_name_hash,
};
use serde::{Deserialize, Serialize};

use crate::builder::{EnabledMod, OverrideMeta, OverrideSource, metadata};
use crate::content::ModContentProvider;
use crate::game_index::GameIndex;

/// One champion WAD whose overridden base skin violates the closed-world
/// assertion the in-game verifier enforces (see [`ltk_sanitize`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinIntegrityOffender {
    /// Mod that owns the winning `skin0.bin` override in this WAD.
    pub mod_id: String,
    /// WAD filename (e.g. `Aatrox.wad.client`).
    pub wad: String,
    /// Lowercase champion directory name.
    pub champion: String,
    /// Human-readable violation lines (see
    /// [`SkinIntegrity::violations`](ltk_sanitize::SkinIntegrity::violations)).
    pub violations: Vec<String>,
}

/// Check every champion WAD with an overridden `skin0.bin` against its
/// original and collect per-mod violations.
///
/// Must run after the overlay WADs exist on disk (built and reused alike):
/// it mounts the actual files the game will load, so it also covers whatever
/// the write path did to the chunks. A WAD that cannot be read is logged and
/// skipped — this is a diagnostic and must never fail the build.
pub(crate) fn collect_skin_integrity_offenders(
    game_dir: &Utf8Path,
    overlay_root: &Utf8Path,
    all_meta: &HashMap<u64, OverrideMeta>,
    wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<u64>>,
    game_index: &GameIndex,
) -> Vec<SkinIntegrityOffender> {
    // Builds always judge under the blessed shared default so the offender
    // list predicts exactly what the in-game verifier will do.
    let policy = SkinPolicy::default();
    let mut offenders = Vec::new();

    for (wad_path, override_hashes) in wad_hash_sets {
        let Some(champion) = champion_from_wad_path(wad_path.as_str()) else {
            continue;
        };
        let root_hash = skin0_bin_name_hash(&champion);
        // A WAD whose skin0.bin is not overridden keeps the original chunk —
        // the check would skip anyway, so don't even mount it.
        if !override_hashes.contains(&root_hash) {
            continue;
        }
        let Some(mod_id) = all_meta.get(&root_hash).map(|m| m.source.mod_id()) else {
            continue;
        };

        let original_path = game_dir.join(wad_path);
        let overlay_path = overlay_root.join(wad_path);
        let (mut original, mut merged) = match (mount(&original_path), mount(&overlay_path)) {
            (Ok(original), Ok(merged)) => (original, merged),
            (Err(err), _) | (_, Err(err)) => {
                tracing::error!("Base-skin check could not read '{wad_path}': {err}; skipping");
                continue;
            }
        };

        // Which other WADs — original game WADs or overlay override sets —
        // contain a hash, to tell "shipped to the wrong WAD" apart from
        // "missing everywhere".
        let world = |hash: u64| -> Vec<String> {
            let mut found: Vec<String> = game_index
                .find_wads_with_hash(hash)
                .unwrap_or_default()
                .iter()
                .filter(|path| *path != wad_path)
                .filter_map(|path| path.file_name().map(str::to_string))
                .collect();
            for (other_path, hashes) in wad_hash_sets {
                if other_path == wad_path || !hashes.contains(&hash) {
                    continue;
                }
                if let Some(name) = other_path.file_name() {
                    let name = name.to_string();
                    if !found.contains(&name) {
                        found.push(name);
                    }
                }
            }
            found
        };

        match check_base_skin(
            &mut WadChunkSource(&mut original),
            &mut WadChunkSource(&mut merged),
            &champion,
            Some(&world),
            policy,
        ) {
            SkinCheckOutcome::SkippedUnmodified => {}
            SkinCheckOutcome::BaselineAnomaly(anomaly) => {
                // Already logged by ltk_sanitize with the stable prefix; add
                // the overlay context so logs pin down which file to look at.
                tracing::error!("base-skin baseline anomaly in '{wad_path}': {anomaly}");
            }
            SkinCheckOutcome::Report(report) => {
                if report.is_broken(policy) {
                    offenders.push(SkinIntegrityOffender {
                        mod_id: mod_id.to_string(),
                        wad: wad_path
                            .file_name()
                            .unwrap_or(wad_path.as_str())
                            .to_string(),
                        champion,
                        violations: report.violations(policy),
                    });
                }
            }
        }
    }

    offenders
}

fn mount(path: &Utf8Path) -> Result<ltk_wad::Wad<File>, String> {
    let file = File::open(path.as_std_path()).map_err(|err| format!("open: {err}"))?;
    ltk_wad::Wad::mount(file).map_err(|err| format!("mount: {err}"))
}

/// Check a single mod's base-skin integrity against the game **without
/// building an overlay or extracting the mod to disk**.
///
/// The merged view is a [`VirtualMerge`] of the mod's override chunks (read
/// in-memory through its [`ModContentProvider`]) over the original game WAD,
/// routed exactly like an overlay build would route them. Untrusted archives
/// are therefore never written to the filesystem to be checked.
///
/// `index_cache_path` is where the [`GameIndex`] cache lives (e.g.
/// `<state>/game_index.bin`); the index is built from `game_dir` when the
/// cache is stale or absent.
///
/// Returns one offender per champion WAD whose overridden `skin0.bin` leaves
/// the base skin violating the closed-world assertion. Baseline anomalies
/// are logged, never returned (see the module docs).
pub fn check_single_mod(
    game_dir: &Utf8Path,
    index_cache_path: &Utf8Path,
    enabled_mod: &mut EnabledMod,
    policy: SkinPolicy,
) -> crate::error::Result<Vec<SkinIntegrityOffender>> {
    let game_index = GameIndex::load_or_build(game_dir, index_cache_path)?;
    let mod_meta = metadata::collect_single_mod_metadata(enabled_mod, &game_index, game_dir)?;

    let mut wad_hash_sets: BTreeMap<Utf8PathBuf, HashSet<u64>> = BTreeMap::new();
    for (&hash, meta) in &mod_meta {
        for target in meta.route_targets(hash, &game_index) {
            wad_hash_sets
                .entry(target.to_owned())
                .or_default()
                .insert(hash);
        }
    }

    let mod_id = enabled_mod.id.clone();
    let mut mod_source = ModChunkSource::new(enabled_mod.content.as_mut(), &mod_meta);
    let mut offenders = Vec::new();

    for (wad_path, routed) in &wad_hash_sets {
        let Some(champion) = champion_from_wad_path(wad_path.as_str()) else {
            continue;
        };
        if !routed.contains(&skin0_bin_name_hash(&champion)) {
            continue;
        }

        let original_path = game_dir.join(wad_path);
        // Two mounts: one is the pristine comparison side, the other is the
        // base layer of the virtual merge.
        let (mut original, mut base) = match (mount(&original_path), mount(&original_path)) {
            (Ok(original), Ok(base)) => (original, base),
            (Err(err), _) | (_, Err(err)) => {
                tracing::error!("Base-skin check could not read '{wad_path}': {err}; skipping");
                continue;
            }
        };

        let world = |hash: u64| -> Vec<String> {
            let mut found: Vec<String> = game_index
                .find_wads_with_hash(hash)
                .unwrap_or_default()
                .iter()
                .filter(|path| *path != wad_path)
                .filter_map(|path| path.file_name().map(str::to_string))
                .collect();
            for (other_path, hashes) in &wad_hash_sets {
                if other_path == wad_path || !hashes.contains(&hash) {
                    continue;
                }
                if let Some(name) = other_path.file_name() {
                    let name = name.to_string();
                    if !found.contains(&name) {
                        found.push(name);
                    }
                }
            }
            found
        };

        mod_source.routed = routed.clone();
        let mut base_source = WadChunkSource(&mut base);
        let mut merged = VirtualMerge {
            overlay: &mut mod_source,
            base: &mut base_source,
        };

        match check_base_skin(
            &mut WadChunkSource(&mut original),
            &mut merged,
            &champion,
            Some(&world),
            policy,
        ) {
            SkinCheckOutcome::SkippedUnmodified => {}
            SkinCheckOutcome::BaselineAnomaly(anomaly) => {
                tracing::error!("base-skin baseline anomaly in '{wad_path}': {anomaly}");
            }
            SkinCheckOutcome::Report(report) => {
                if report.is_broken(policy) {
                    offenders.push(SkinIntegrityOffender {
                        mod_id: mod_id.clone(),
                        wad: wad_path
                            .file_name()
                            .unwrap_or(wad_path.as_str())
                            .to_string(),
                        champion,
                        violations: report.violations(policy),
                    });
                }
            }
        }
    }

    Ok(offenders)
}

/// [`ChunkSource`] over a single mod's override chunks, read lazily through
/// its content provider and cached per `(layer, WAD)` directory.
///
/// `checksum` reports the override's *content hash* (xxh3 of the uncompressed
/// bytes) as a pseudo-checksum: it only ever gets compared against original
/// TOC checksums, so overridden chunks read as modified — which is all the
/// correctness lane needs (its violations come from *missing* chunks).
struct ModChunkSource<'a> {
    provider: &'a mut dyn ModContentProvider,
    meta: &'a HashMap<u64, OverrideMeta>,
    /// Hashes routed to the WAD currently being checked; chunks outside it
    /// are invisible (the closed world under test).
    routed: HashSet<u64>,
    wad_cache: HashMap<(String, String), HashMap<u64, Vec<u8>>>,
    raw_cache: Option<HashMap<u64, Vec<u8>>>,
}

impl<'a> ModChunkSource<'a> {
    fn new(provider: &'a mut dyn ModContentProvider, meta: &'a HashMap<u64, OverrideMeta>) -> Self {
        Self {
            provider,
            meta,
            routed: HashSet::new(),
            wad_cache: HashMap::new(),
            raw_cache: None,
        }
    }

    fn bytes_for(&mut self, name_hash: u64) -> Result<Vec<u8>, String> {
        let meta = self
            .meta
            .get(&name_hash)
            .ok_or_else(|| "chunk is not one of this mod's overrides".to_string())?;
        let entries = match &meta.source {
            OverrideSource::LayerWad {
                layer, wad_name, ..
            } => {
                let key = (layer.clone(), wad_name.clone());
                if !self.wad_cache.contains_key(&key) {
                    let entries = self
                        .provider
                        .read_wad_overrides(layer, wad_name)
                        .map_err(|err| err.to_string())?;
                    self.wad_cache.insert(key.clone(), index_by_hash(entries));
                }
                &self.wad_cache[&key]
            }
            OverrideSource::Raw { .. } => {
                if self.raw_cache.is_none() {
                    let entries = self
                        .provider
                        .read_raw_overrides()
                        .map_err(|err| err.to_string())?;
                    self.raw_cache = Some(index_by_hash(entries));
                }
                self.raw_cache.as_ref().expect("populated above")
            }
            OverrideSource::StringPatch { .. } => {
                return Err("string-patch overrides have no source bytes".to_string());
            }
        };
        entries
            .get(&name_hash)
            .cloned()
            .ok_or_else(|| "override bytes not found in mod content".to_string())
    }
}

/// Index provider entries by their resolved chunk path hash.
fn index_by_hash(entries: Vec<(Utf8PathBuf, Vec<u8>)>) -> HashMap<u64, Vec<u8>> {
    let mut by_hash = HashMap::new();
    for (rel_path, bytes) in entries {
        match crate::utils::resolve_chunk_hash(&rel_path, &bytes) {
            Ok(hash) => {
                by_hash.insert(hash, bytes);
            }
            Err(err) => tracing::warn!("Skipping override '{rel_path}': {err}"),
        }
    }
    by_hash
}

impl ChunkSource for ModChunkSource<'_> {
    fn checksum(&mut self, name_hash: u64) -> Option<u64> {
        if !self.routed.contains(&name_hash) {
            return None;
        }
        self.meta.get(&name_hash).map(|meta| meta.content_hash)
    }

    fn load(&mut self, name_hash: u64) -> Result<Vec<u8>, String> {
        if !self.routed.contains(&name_hash) {
            return Err("chunk is not routed to this WAD".to_string());
        }
        self.bytes_for(name_hash)
    }
}
