//! Main overlay builder implementation (two-pass architecture).
//!
//! The [`OverlayBuilder`] orchestrates the full overlay build pipeline:
//! game indexing, override metadata collection, cross-WAD distribution, and WAD patching.
//!
//! # Two-Pass Build Algorithm
//!
//! 1. Validate that `game_dir/DATA/FINAL` exists.
//! 2. Build (or load from cache) a [`GameIndex`] from all `.wad.client` files.
//! 3. Load the saved [`OverlayState`] and choose a build strategy:
//!    - **Skip**: mod list, per-mod content fingerprints, and game fingerprint
//!      all match, no WAD is marked dirty, and every overlay WAD file still
//!      exists on disk.
//!    - **Incremental**: game fingerprint and state version match but the mod
//!      list or some mod's content differs. Compute per-WAD override
//!      fingerprints and only rebuild WADs whose fingerprint changed. Remove
//!      stale WADs no longer needed.
//!    - **Full rebuild**: state version or game fingerprint mismatch. Wipe all
//!      overlay WAD files and rebuild everything from scratch.
//! 4. **Pass 1**: Collect lightweight override metadata (hashes, sizes, source
//!    locations) from all mods. Uses a persistent metadata cache to skip
//!    unchanged mods entirely. Bytes are hashed and then dropped.
//! 5. Distribute override hashes to WADs, partition into rebuild/reuse sets,
//!    then work out which of the rebuilds can keep their file and rewrite only
//!    its tail (see the `incremental` module). Those are marked dirty in one atomic state
//!    write before any byte is touched.
//! 6. **Pass 2**: Re-read override bytes only for WADs being rebuilt, and only
//!    for the overrides a tail rewrite cannot lift out of the file it is about
//!    to rewrite. Compress each distinct content once, then write every WAD in
//!    parallel via [`build_patched_wad`](crate::wad_builder::build_patched_wad)
//!    or a [`WadRebasePlan`](ltk_wad::WadRebasePlan).
//! 7. Persist the new [`OverlayState`]: per-WAD fingerprints, the layout record
//!    of every WAD now on disk, and no dirty flags.

mod incremental;
mod metadata;
mod resolve;

use crate::builder::incremental::PreviousOverlay;
use crate::content::ModContentProvider;
use crate::error::{Error, GameDirError, Result};
use crate::game_index::GameIndex;
use crate::linked_bins::{LinkedBinOffender, collect_linked_bin_offenders};
use crate::state::{OverlayState, WadLayoutRecord};
use crate::strings::{self, StringOverrideMode, StringPatchPlan};
use crate::utils::ContentHash;
use camino::{Utf8Path, Utf8PathBuf};
use ltk_wad::WadHash;
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// Shared byte buffer for override data distributed across multiple WADs.
///
/// A single mod override may be distributed to many WADs (cross-WAD matching),
/// so we share the bytes via `Arc` instead of cloning.
pub(crate) type SharedBytes = Arc<[u8]>;

/// Where an override can be re-read from in pass 2.
#[derive(Clone, Debug)]
pub(crate) enum OverrideSource {
    /// Override from a WAD directory inside a layer.
    LayerWad {
        mod_id: String,
        layer: String,
        wad_name: String,
        rel_path: Utf8PathBuf,
    },
    /// Override from the raw content directory.
    Raw {
        mod_id: String,
        rel_path: Utf8PathBuf,
    },
    /// Synthetic override produced by the string-override engine: the patched
    /// `lol.stringtable` for one locale (encoded in the chunk path). Its bytes
    /// are generated at resolve time (base chunk + key-level overrides) rather
    /// than re-read from a provider, so this variant never enters the per-mod
    /// metadata cache.
    StringPatch { chunk_path: Utf8PathBuf },
}

/// Pseudo mod id reported for [`OverrideSource::StringPatch`] entries, which
/// aggregate overrides from several mods.
pub(crate) const STRING_PATCH_MOD_ID: &str = "string-overrides";

impl OverrideSource {
    /// The id of the mod this override came from.
    pub(crate) fn mod_id(&self) -> &str {
        match self {
            OverrideSource::LayerWad { mod_id, .. } => mod_id,
            OverrideSource::Raw { mod_id, .. } => mod_id,
            OverrideSource::StringPatch { .. } => STRING_PATCH_MOD_ID,
        }
    }

    /// The relative path this override was read from (for diagnostics).
    pub(crate) fn rel_path(&self) -> &Utf8Path {
        match self {
            OverrideSource::LayerWad { rel_path, .. } => rel_path.as_path(),
            OverrideSource::Raw { rel_path, .. } => rel_path.as_path(),
            OverrideSource::StringPatch { chunk_path, .. } => chunk_path.as_path(),
        }
    }
}

/// Lightweight metadata collected in pass 1 (no byte data).
#[derive(Clone, Debug)]
pub struct OverrideMeta {
    pub content_hash: ContentHash,
    pub uncompressed_size: usize,
    pub(crate) source: OverrideSource,
    /// WAD path to route this override to when the game index has no match
    pub(crate) fallback_wad: Option<Utf8PathBuf>,
    /// The unlocalized sibling of [`Self::fallback_wad`]
    pub(crate) unlocalized_wad: Option<Utf8PathBuf>,
    /// Linked-file dependency paths declared by this override when it is a League
    /// property-bin (`PROP`/`PTCH`); empty otherwise. Parsed once in pass 1 and
    /// cached so the linked-bin pre-flight needs no re-decompression.
    pub(crate) linked_bins: Vec<String>,
}

impl OverrideMeta {
    /// Whether this override is a cross-WAD import
    pub(crate) fn is_cross_wad_import(&self, path_hash: WadHash, game_index: &GameIndex) -> bool {
        if !matches!(self.source, OverrideSource::LayerWad { .. }) {
            return false;
        }
        let Some(fallback) = self.fallback_wad.as_deref() else {
            return false;
        };
        game_index
            .find_wads_with_hash(path_hash)
            .unwrap_or_default()
            .iter()
            .all(|wad| wad != fallback)
    }

    /// The game-relative WAD paths this override routes to.
    ///
    /// - Every game WAD containing `path_hash` - a shared chunk is fanned out
    ///   to all of its holders so every loaded copy stays checksum-consistent.
    /// - Additionally `fallback_wad` when no WAD hash-matched, or when the
    ///   override is a [cross-WAD import](Self::is_cross_wad_import) whose
    ///   declared WAD is missing from the matches.
    /// - Additionally [`unlocalized_wad`](Self::unlocalized_wad) whenever
    ///   `fallback_wad` is taken, so a chunk declared into a localized WAD also
    ///   reaches players on other locales and the integrity scan can resolve it.
    ///
    /// An empty result means the override cannot be routed anywhere and is
    /// dropped. This is the single source of truth shared by
    /// [`OverlayBuilder::distribute_override_hashes`] and
    /// [`ModWadReport::from_meta`] so build routing and mod reports agree.
    pub(crate) fn route_targets<'a>(
        &'a self,
        path_hash: WadHash,
        game_index: &'a GameIndex,
    ) -> Vec<&'a Utf8Path> {
        let matched = game_index
            .find_wads_with_hash(path_hash)
            .unwrap_or_default();

        let mut targets: Vec<&Utf8Path> = matched.iter().map(Utf8PathBuf::as_path).collect();

        if let Some(fallback) = self.fallback_wad.as_deref()
            && (targets.is_empty() || self.is_cross_wad_import(path_hash, game_index))
        {
            targets.push(fallback);
            if let Some(unlocalized) = self.unlocalized_wad.as_deref()
                && !targets.contains(&unlocalized)
            {
                targets.push(unlocalized);
            }
        }

        targets
    }
}

/// A mod to be included in the overlay build.
///
/// Each enabled mod contributes override files through its [`ModContentProvider`].
/// Mods are processed in the order they appear in the `enabled_mods` list passed to
/// [`OverlayBuilder::set_enabled_mods`]. Position 0 (first in the list) has the
/// **highest** priority - when two mods override the same path hash, the mod
/// closer to the front of the list wins.
pub struct EnabledMod {
    /// Unique identifier for the mod (used in state tracking and logging).
    pub id: String,
    /// Content provider for accessing mod metadata and override files.
    ///
    /// This can be backed by a filesystem directory, a `.modpkg` archive, a
    /// `.fantome` ZIP, or any other source that implements [`ModContentProvider`].
    pub content: Box<dyn ModContentProvider>,
    /// Optional set of layer names to include. When `Some`, only layers whose
    /// names are in this set will be processed during overlay building. When
    /// `None`, all layers are included (backward-compatible default).
    pub enabled_layers: Option<HashSet<String>>,
}

/// The name of the base layer that is always included regardless of
/// `enabled_layers` configuration. Every mod has a base layer at priority 0.
pub const BASE_LAYER_NAME: &str = "base";

impl EnabledMod {
    /// Compute a cache fingerprint that accounts for both content changes and
    /// the current `enabled_layers` selection.
    ///
    /// Returns `None` if the underlying content provider cannot compute a
    /// fingerprint (the metadata cache will be skipped for this mod).
    pub fn cache_fingerprint(&self) -> Option<u64> {
        use xxhash_rust::xxh3::xxh3_64;

        let base_fp = self.content.content_fingerprint().unwrap_or_else(|e| {
            tracing::warn!(
                "Failed to compute content fingerprint for mod '{}': {}",
                self.id,
                e
            );
            None
        })?;

        Some(match &self.enabled_layers {
            Some(layers) => {
                // Exclude BASE_LAYER_NAME before hashing - it's always implicitly
                // active via is_layer_active, so {"extras"} and {"base","extras"}
                // should produce the same fingerprint.
                let mut sorted: Vec<&str> = layers
                    .iter()
                    .map(|s| s.as_str())
                    .filter(|&s| s != BASE_LAYER_NAME)
                    .collect();
                sorted.sort_unstable();

                // Encode as "fp\0layer1\0layer2\0..." and hash the whole buffer.
                let mut buf = format!("{base_fp}");
                for layer in &sorted {
                    buf.push('\0');
                    buf.push_str(layer);
                }

                xxh3_64(buf.as_bytes())
            }
            None => base_fp,
        })
    }

    /// Returns whether the given layer name should be processed for this mod.
    ///
    /// A layer is active when:
    /// - `enabled_layers` is `None` (all layers enabled), OR
    /// - The layer is the base layer ([`BASE_LAYER_NAME`]), OR
    /// - The layer is explicitly listed in `enabled_layers`.
    pub fn is_layer_active(&self, layer_name: &str) -> bool {
        match &self.enabled_layers {
            None => true,
            Some(allowed) => layer_name == BASE_LAYER_NAME || allowed.contains(layer_name),
        }
    }
}

/// Progress information emitted during overlay building.
///
/// Serialized as JSON and sent to the frontend via Tauri events so the UI can
/// display a progress bar and stage label. The `current`/`total` fields are only
/// meaningful during the [`PatchingWad`](OverlayStage::PatchingWad) stage.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayProgress {
    /// Current stage of the build process.
    pub stage: OverlayStage,
    /// Filename of the WAD currently being patched (set during `PatchingWad`).
    pub current_file: Option<String>,
    /// 1-based index of the WAD currently being patched.
    pub current: u32,
    /// Total number of WADs that need patching.
    pub total: u32,
}

impl OverlayProgress {
    /// Create a stage-only progress event with no file or counter information.
    fn stage(stage: OverlayStage) -> Self {
        Self {
            stage,
            current_file: None,
            current: 0,
            total: 0,
        }
    }
}

/// Stages of the overlay build pipeline.
///
/// Emitted in order: `Indexing` -> `CollectingOverrides` -> `PatchingWad` (repeated) -> `Complete`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayStage {
    /// Scanning the game directory and building the [`GameIndex`].
    Indexing,
    /// Reading override files from all enabled mods.
    CollectingOverrides,
    /// Building a patched WAD file in the overlay directory.
    PatchingWad,
    /// Merging string overrides from all enabled mods and planning stringtable
    /// patches for the target locales.
    ApplyingStringOverrides,
    /// Build finished successfully.
    Complete,
}

/// Summary returned after an overlay build completes.
#[derive(Debug)]
pub struct OverlayBuildResult {
    /// Root directory of the overlay (mirrors the game's `DATA/FINAL` structure).
    pub overlay_root: Utf8PathBuf,
    /// WAD files that were freshly built during this run.
    pub wads_built: Vec<Utf8PathBuf>,
    /// WAD files reused from a previous build (unchanged fingerprint).
    pub wads_reused: Vec<Utf8PathBuf>,
    /// Detected conflicts between mods (not yet implemented).
    pub conflicts: Vec<Conflict>,
    /// Chunks whose container claimed a checksum its own bytes do not have.
    ///
    /// Empty on a build that passed no chunk through, and on one where every
    /// container told the truth.
    pub checksum_mismatches: Vec<ChecksumMismatch>,
    /// Wall-clock time for the entire build.
    pub build_time: Duration,
}

/// A chunk whose container claimed a checksum its own bytes do not have.
///
/// Reported, never fatal. Mod tools in the wild ship wrong metadata over
/// perfectly good bytes - the same tools already ship wrong ZIP CRCs, which this
/// crate also tolerates - and the overlay carries the checksum recomputed while
/// the bytes were copied, so the mod's content still reaches the game intact.
/// What the report is for is the container: a mod that produces these is worth
/// re-exporting. See `docs/adr/0001-pass-through-recomputes-checksums-and-warns.md`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumMismatch {
    /// Mod identifier (matches [`EnabledMod::id`]).
    pub mod_id: String,
    /// The mod's WAD target the chunk was read from, e.g. `Aatrox.wad.client`.
    pub wad_name: String,
    /// Path hash of the chunk that disagreed.
    pub path_hash: WadHash,
    /// What the container's own TOC claimed for the chunk's stored bytes.
    pub claimed: u64,
    /// What those bytes actually hash to - the value the overlay TOC carries.
    pub computed: u64,
}

/// A conflict where multiple mods override the same chunk (not yet implemented).
#[derive(Debug, Clone)]
pub struct Conflict {
    /// xxHash3 path hash of the conflicting chunk.
    pub path_hash: WadHash,
    /// Human-readable path (if available from hash tables).
    pub path: String,
    /// All mods that contributed an override for this chunk.
    pub contributing_mods: Vec<ModContribution>,
    /// The mod whose override was used (last-writer-wins).
    pub winner: String,
}

/// One game WAD a mod's overrides land in, paired with how many land there.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AffectedWad {
    /// Game-relative WAD path, e.g. `DATA/FINAL/Champions/Aatrox.wad.client`.
    pub path: Box<str>,
    /// Number of the mod's distinct overrides that land in this WAD.
    ///
    /// A single override whose chunk hash exists in several game WADs - e.g. a
    /// champion *base* chunk that the game physically duplicates into every map
    /// WAD - contributes to each WAD's count. Summed across a report's WADs this
    /// therefore exceeds [`ModWadReport::override_count`] when a mod's content
    /// spills across WADs. The asymmetry (a large count in the mod's real target
    /// WAD, small equal counts in the WADs it bleeds into) is what distinguishes
    /// a genuine edit from incidental spillover.
    pub override_count: u32,
}

/// Per-mod summary of which game WAD files a mod's overrides affect.
///
/// Computed independently for each mod (i.e. before cross-mod merging), so
/// the result is **load-order independent**: it represents the mod's full
/// potential WAD footprint regardless of which other mods are enabled
/// alongside it.
///
/// Overrides that can never reach the overlay - SubChunkTOC entries,
/// mod-shipped stringtable chunks, and lazy overrides byte-identical to the
/// game originals whose declared WAD already holds the chunk - are excluded
/// before the report is computed, so the reported footprint matches the WADs
/// an overlay build of this mod alone would actually write.
///
/// Reports are produced in two ways:
///
/// 1. As a side effect of [`OverlayBuilder::build`], which captures one report
///    per enabled mod and exposes them via
///    [`take_mod_wad_reports`](OverlayBuilder::take_mod_wad_reports).
/// 2. On demand via [`OverlayBuilder::analyze_single_mod`], which runs the
///    same per-mod analysis without writing any overlay files.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModWadReport {
    /// Mod identifier (matches [`EnabledMod::id`]).
    pub mod_id: String,
    /// The game WADs the mod's overrides land in, each with its per-WAD override
    /// count. Sorted by [`AffectedWad::path`] and deduplicated. Use
    /// [`wad_paths`](Self::wad_paths) for the paths alone.
    pub affected_wads: Vec<AffectedWad>,
    /// Total number of distinct override entries the mod contributes across all
    /// layers. Less than the sum of the per-WAD counts when overrides spill
    /// across WADs (see [`AffectedWad::override_count`]).
    pub override_count: u32,
    /// Content fingerprint of the mod at the time the report was computed,
    /// from [`ModContentProvider::content_fingerprint`].
    pub content_fingerprint: Option<u64>,
    /// Game index fingerprint at the time the report was computed.
    pub game_index_fingerprint: u64,
}

impl ModWadReport {
    /// The affected WAD path strings in report order, without the per-WAD counts.
    pub fn wad_paths(&self) -> impl Iterator<Item = &str> {
        self.affected_wads.iter().map(|w| w.path.as_ref())
    }

    /// Build a report from one mod's collected override metadata.
    ///
    /// Each override contributes to every WAD returned by
    /// [`OverrideMeta::route_targets`] - the same routing the build itself
    /// uses: hash-matched game WADs, plus the mod's declared WAD for new
    /// entries and cross-WAD imports.
    pub(crate) fn from_meta(
        mod_id: String,
        mod_meta: &HashMap<WadHash, OverrideMeta>,
        content_fingerprint: Option<u64>,
        game_index: &GameIndex,
    ) -> Self {
        let mut counts: BTreeMap<&Utf8Path, u32> = BTreeMap::new();
        for (path_hash, meta) in mod_meta {
            for wad_path in meta.route_targets(*path_hash, game_index) {
                *counts.entry(wad_path).or_insert(0) += 1;
            }
        }

        Self {
            mod_id,
            affected_wads: counts
                .into_iter()
                .map(|(path, override_count)| AffectedWad {
                    path: path.as_str().into(),
                    override_count,
                })
                .collect(),
            override_count: mod_meta.len() as u32,
            content_fingerprint,
            game_index_fingerprint: game_index.game_fingerprint(),
        }
    }
}

/// Details about one mod's contribution to a conflicting chunk.
#[derive(Debug, Clone)]
pub struct ModContribution {
    /// Unique mod identifier.
    pub mod_id: String,
    /// Human-readable mod name.
    pub mod_name: String,
    /// Layer name the override came from.
    pub layer: String,
    /// Layer priority value.
    pub priority: i32,
    /// Position in the enabled mods list (0-based).
    pub install_order: usize,
}

pub(crate) type ProgressCallback = Arc<dyn Fn(OverlayProgress) + Send + Sync>;

/// The layout records to persist for every WAD the overlay now holds.
///
/// Freshly written WADs get a record built from what the writer reported;
/// reused ones carry their previous record forward unchanged, since their file
/// did not move. A WAD with no usable record is simply absent, which is what
/// puts it on the full-rebuild path next time.
fn collect_wad_layouts(
    built: &[resolve::PatchedWad],
    reused: &[Utf8PathBuf],
    all_meta: &HashMap<WadHash, OverrideMeta>,
    prev_state: Option<&OverlayState>,
) -> BTreeMap<String, WadLayoutRecord> {
    let mut layouts: BTreeMap<String, WadLayoutRecord> = built
        .iter()
        .map(|wad| {
            (
                wad.relative_path.as_str().to_string(),
                WadLayoutRecord {
                    source: wad.stats.source,
                    layout: wad.stats.layout,
                    // The overrides the file actually holds, not the ones this
                    // build routed to it: a record that overstates its tail
                    // would let a later build reuse bytes that are not there.
                    overrides: wad
                        .written_overrides
                        .iter()
                        .filter_map(|&hash| Some((hash, all_meta.get(&hash)?.content_hash)))
                        .collect(),
                },
            )
        })
        .collect();

    for relative_path in reused {
        let key = relative_path.as_str();
        if let Some(record) = prev_state.and_then(|state| state.wad_layout(key)) {
            layouts.insert(key.to_string(), record.clone());
        }
    }

    layouts
}

/// Orchestrates the overlay build pipeline.
///
/// Create a builder with [`new`](Self::new), configure it with
/// [`set_enabled_mods`](Self::set_enabled_mods) and optionally
/// [`with_progress`](Self::with_progress), then call [`build`](Self::build).
///
/// The builder owns the enabled mod list and consumes each mod's content provider
/// during the build. After building, the same builder instance can be reconfigured
/// and built again.
pub struct OverlayBuilder {
    game_dir: Utf8PathBuf,
    overlay_root: Utf8PathBuf,
    /// Directory for `overlay.json` and `game_index.bin`
    /// (typically the parent profile directory, e.g. `profiles/default/`).
    state_dir: Utf8PathBuf,
    enabled_mods: Vec<EnabledMod>,
    blocked_wads: HashSet<String>,
    string_override_mode: StringOverrideMode,
    progress_callback: Option<ProgressCallback>,
    /// Per-mod WAD reports captured during the most recent successful
    /// [`build`](Self::build), drained via [`take_mod_wad_reports`](Self::take_mod_wad_reports).
    last_mod_wad_reports: Vec<ModWadReport>,
    /// Mods with unresolved property-bin linked dependencies from the most recent
    /// [`build`](Self::build), drained via
    /// [`take_linked_bin_offenders`](Self::take_linked_bin_offenders).
    last_linked_bin_offenders: Vec<LinkedBinOffender>,
    /// Chunks the most recent [`build`](Self::build) passed through whose
    /// container claimed the wrong checksum for them. Moved into that build's
    /// [`OverlayBuildResult`].
    last_checksum_mismatches: Vec<ChecksumMismatch>,
}

impl OverlayBuilder {
    /// Create a new overlay builder.
    ///
    /// # Arguments
    ///
    /// * `game_dir` - Path to the League of Legends `Game/` directory. Must contain
    ///   a `DATA/FINAL` subdirectory with `.wad.client` files.
    /// * `overlay_root` - Directory where patched WAD files will be written
    ///   (e.g. `profiles/default/overlay`).
    /// * `state_dir` - Directory for `overlay.json` and `game_index.bin`
    ///   (e.g. the profile folder `profiles/default/`).
    pub fn new(game_dir: Utf8PathBuf, overlay_root: Utf8PathBuf, state_dir: Utf8PathBuf) -> Self {
        Self {
            game_dir,
            overlay_root,
            state_dir,
            enabled_mods: Vec::new(),
            blocked_wads: HashSet::new(),
            string_override_mode: StringOverrideMode::Disabled,
            progress_callback: None,
            last_mod_wad_reports: Vec::new(),
            last_linked_bin_offenders: Vec::new(),
            last_checksum_mismatches: Vec::new(),
        }
    }

    /// Drain the per-mod WAD reports captured during the most recent successful
    /// [`build`](Self::build).
    ///
    /// Each report describes one mod's full potential WAD footprint, computed
    /// independently of load order. Returns an empty vector if no build has
    /// run yet, or if the previous build short-circuited (e.g. exact-match skip).
    pub fn take_mod_wad_reports(&mut self) -> Vec<ModWadReport> {
        std::mem::take(&mut self.last_mod_wad_reports)
    }

    /// Drain the linked-bin offenders detected during the most recent
    /// [`build`](Self::build).
    ///
    /// Each entry is a mod whose property-bins reference linked dependencies that
    /// are absent from the overlay WAD they land in. Returns an empty vector when
    /// the last build found none. On an exact-match skip the offenders from the
    /// previous build are restored from the persisted overlay state.
    pub fn take_linked_bin_offenders(&mut self) -> Vec<LinkedBinOffender> {
        std::mem::take(&mut self.last_linked_bin_offenders)
    }

    /// Analyze a single mod's WAD footprint without building or modifying any
    /// overlay artifacts.
    ///
    /// Loads (or builds) the [`GameIndex`] from `game_dir` using the cache at
    /// `state_dir/game_index.bin`, then runs the same per-mod metadata
    /// collection used during a full build and resolves it into a
    /// [`ModWadReport`]. Safe to call concurrently with [`build`](Self::build)
    /// because it neither writes overlay state nor takes any locks held by
    /// the build pipeline.
    pub fn analyze_single_mod(
        game_dir: &Utf8Path,
        state_dir: &Utf8Path,
        enabled_mod: &mut EnabledMod,
    ) -> Result<ModWadReport> {
        let data_final_dir = game_dir.join("DATA").join("FINAL");
        if !data_final_dir.as_std_path().exists() {
            return Err(GameDirError::MissingDataFinal {
                path: game_dir.to_path_buf(),
            }
            .into());
        }

        std::fs::create_dir_all(state_dir.as_std_path())
            .map_err(|source| Error::write(state_dir, source))?;
        let cache_path = state_dir.join("game_index.bin");
        let game_index = GameIndex::load_or_build(game_dir, &cache_path)?;

        let fingerprint = enabled_mod.cache_fingerprint();
        let mod_meta = metadata::collect_single_mod_metadata(enabled_mod, &game_index, game_dir)?;

        Ok(ModWadReport::from_meta(
            enabled_mod.id.clone(),
            &mod_meta,
            fingerprint,
            &game_index,
        ))
    }

    /// Register a progress callback.
    ///
    /// The callback receives [`OverlayProgress`] updates at each stage of the build.
    /// This is typically used to forward progress to a UI via Tauri events.
    pub fn with_progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(OverlayProgress) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    /// Set WAD filenames to block from patching.
    ///
    /// Filenames are automatically lowercased for case-insensitive matching.
    pub fn with_blocked_wads(mut self, wads: Vec<String>) -> Self {
        self.blocked_wads = wads.into_iter().map(|w| w.to_ascii_lowercase()).collect();
        self
    }

    /// Configure which locales mods' string overrides are applied to.
    ///
    /// Defaults to [`StringOverrideMode::Disabled`], in which case
    /// `ModProjectLayer::string_overrides` metadata is carried through but never
    /// applied to the game's stringtables.
    pub fn with_string_overrides(mut self, mode: StringOverrideMode) -> Self {
        self.string_override_mode = mode;
        self
    }

    /// Set the ordered list of mods to include in the overlay.
    ///
    /// Order matters: the first mod in the list (index 0) has the highest priority.
    /// When two mods override the same chunk, the mod closer to the front wins.
    pub fn set_enabled_mods(&mut self, mods: Vec<EnabledMod>) {
        self.enabled_mods = mods;
    }

    /// Build the overlay with incremental rebuild support (two-pass).
    ///
    /// 1. If the overlay state matches exactly - including every mod's content
    ///    fingerprint - and all WAD files exist → skip.
    /// 2. If the game fingerprint and state version match → incremental rebuild
    ///    (only re-patch WADs whose override fingerprint changed).
    /// 3. Otherwise → full rebuild (wipe and rebuild everything).
    pub fn build(&mut self) -> Result<OverlayBuildResult> {
        let start_time = std::time::Instant::now();

        // Reset per-build outputs; each return path sets these as appropriate.
        self.last_linked_bin_offenders = Vec::new();
        self.last_checksum_mismatches = Vec::new();

        let effective_blocked = self.effective_blocked_wads();

        tracing::info!("Building overlay...");
        tracing::debug!("Game dir: {}", self.game_dir);
        tracing::debug!("Overlay root: {}", self.overlay_root);
        tracing::debug!("Enabled mods: {}", self.enabled_mods.len());
        tracing::debug!("Blocked WADs: {:?}", effective_blocked);

        self.emit_progress(OverlayProgress::stage(OverlayStage::Indexing));

        let data_final_dir = self.game_dir.join("DATA").join("FINAL");
        if !data_final_dir.as_std_path().exists() {
            return Err(GameDirError::MissingDataFinal {
                path: self.game_dir.clone(),
            }
            .into());
        }

        std::fs::create_dir_all(self.overlay_root.as_std_path())
            .map_err(|source| Error::write(&self.overlay_root, source))?;
        std::fs::create_dir_all(self.state_dir.as_std_path())
            .map_err(|source| Error::write(&self.state_dir, source))?;

        let cache_path = self.state_dir.join("game_index.bin");
        let game_index = GameIndex::load_or_build(&self.game_dir, &cache_path)?;

        // Resolved locale targets are part of the build configuration: they enter
        // the state comparison so toggling the mode invalidates the exact-match skip.
        let target_locales = self.string_override_mode.resolve_locales(&game_index);

        // Load previous state
        let state_path = self.state_dir.join("overlay.json");
        let enabled_ids: Vec<String> = self.enabled_mods.iter().map(|m| m.id.clone()).collect();

        // A state file that will not parse is treated as no state at all, which
        // costs one full rebuild. Everything it holds describes files this
        // builder wrote and can write again; refusing to build because a cache
        // is unreadable would strand the user with no overlay.
        let prev_state = OverlayState::load(&state_path).unwrap_or_else(|e| {
            tracing::warn!(
                "Overlay state at {} could not be read ({}); rebuilding from scratch",
                state_path,
                e
            );
            None
        });

        // --- Handle empty mod list ---
        if self.enabled_mods.is_empty() {
            tracing::info!("Overlay: no enabled mods, cleaning overlay");

            self.clean_overlay_wads()?;

            let state = OverlayState::new(
                Vec::new(),
                BTreeMap::new(),
                game_index.game_fingerprint(),
                effective_blocked.clone(),
                target_locales.clone(),
                BTreeMap::new(),
            );
            state.save(&state_path)?;

            self.emit_progress(OverlayProgress::stage(OverlayStage::Complete));

            return Ok(OverlayBuildResult {
                overlay_root: self.overlay_root.clone(),
                wads_built: Vec::new(),
                wads_reused: Vec::new(),
                conflicts: Vec::new(),
                checksum_mismatches: Vec::new(),
                build_time: start_time.elapsed(),
            });
        }

        let (fingerprints, mod_fingerprints) = self.collect_active_mod_fingerprints();

        if let Some(result) = self.try_exact_match_skip(
            prev_state.as_ref(),
            &enabled_ids,
            mod_fingerprints.as_ref(),
            game_index.game_fingerprint(),
            &effective_blocked,
            &target_locales,
            start_time,
        ) {
            return Ok(result);
        }

        // Determine if incremental build is possible
        let can_incremental = prev_state
            .as_ref()
            .is_some_and(|s| s.supports_incremental(game_index.game_fingerprint()));

        if !can_incremental {
            tracing::info!(
                "Overlay: full rebuild required (state version or game fingerprint mismatch)"
            );
            self.clean_overlay_wads()?;
        }

        self.emit_progress(OverlayProgress::stage(OverlayStage::CollectingOverrides));

        let (mut all_meta, mod_wad_reports) =
            self.collect_all_override_metadata(&game_index, &fingerprints)?;
        self.last_mod_wad_reports = mod_wad_reports;

        let string_plans =
            self.build_string_patch_plans(&game_index, &target_locales, &mut all_meta)?;

        let mut wad_hash_sets = self.distribute_override_hashes(&all_meta, &game_index);

        wad_hash_sets.retain(|path, _| {
            let blocked = self.is_wad_blocked(path);
            if blocked {
                tracing::info!("Blocked WAD from patching: {}", path);
            }
            !blocked
        });

        // Validate property-bin linked dependencies against the overlay WADs we are
        // about to write. Runs over every enabled mod's overrides (built and reused
        // alike) since distribution precedes the rebuild/reuse partition.
        self.last_linked_bin_offenders =
            collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index);
        if !self.last_linked_bin_offenders.is_empty() {
            tracing::info!(
                "Linked-bin check: {} mod(s) reference unresolved linked bins",
                self.last_linked_bin_offenders.len()
            );
        }

        let (wads_to_build, wads_to_reuse, new_wad_fingerprints) =
            self.partition_wads_from_meta(&wad_hash_sets, &all_meta, &prev_state, can_incremental);

        // Which of them can keep their file and rewrite only their tail. Decided
        // before anything is written, from metadata and two in-memory TOCs.
        let rewrites = self.plan_tail_rewrites(
            &wads_to_build,
            &wad_hash_sets,
            &all_meta,
            prev_state.as_ref(),
            if can_incremental {
                PreviousOverlay::OnDisk
            } else {
                PreviousOverlay::Wiped
            },
        );
        self.mark_dirty(&state_path, prev_state.as_ref(), rewrites.keys())?;

        let reused = rewrites
            .values()
            .flat_map(|rewrite| rewrite.reused.iter())
            .map(|(&hash, prepared)| (hash, prepared.clone()))
            .collect();

        let wad_overrides = self.resolve_overrides_for_wads(
            &wads_to_build,
            &wad_hash_sets,
            &all_meta,
            &string_plans,
            reused,
        )?;

        let built = self.patch_wads_parallel(wads_to_build, wad_overrides, rewrites)?;

        self.sweep_unexpected_overlay_files(&new_wad_fingerprints);

        let reused_paths: Vec<Utf8PathBuf> = wads_to_reuse
            .iter()
            .map(|p| self.overlay_root.join(p))
            .collect();

        let mut state = OverlayState::new(
            enabled_ids,
            mod_fingerprints.unwrap_or_default(),
            game_index.game_fingerprint(),
            effective_blocked,
            target_locales,
            new_wad_fingerprints,
        );
        state.linked_bin_offenders = self.last_linked_bin_offenders.clone();
        state.wad_layouts =
            collect_wad_layouts(&built, &wads_to_reuse, &all_meta, prev_state.as_ref());
        state.save(&state_path)?;

        let built_paths: Vec<Utf8PathBuf> = built.into_iter().map(|wad| wad.path).collect();
        let total_wads = built_paths.len() as u32;
        self.emit_progress(OverlayProgress {
            stage: OverlayStage::Complete,
            current_file: None,
            current: total_wads,
            total: total_wads,
        });

        tracing::info!(
            "Overlay build complete: {} built, {} reused in {:?}",
            built_paths.len(),
            reused_paths.len(),
            start_time.elapsed()
        );

        Ok(OverlayBuildResult {
            overlay_root: self.overlay_root.clone(),
            wads_built: built_paths,
            wads_reused: reused_paths,
            conflicts: Vec::new(),
            checksum_mismatches: std::mem::take(&mut self.last_checksum_mismatches),
            build_time: start_time.elapsed(),
        })
    }

    /// Force a full rebuild, ignoring the saved overlay state.
    ///
    /// Use this when the user explicitly requests a rebuild or when you know
    /// the overlay is out of date for reasons the state file cannot track.
    pub fn rebuild_all(&mut self) -> Result<OverlayBuildResult> {
        // Remove previous state so build() sees no match
        let state_path = self.state_dir.join("overlay.json");
        if state_path.as_std_path().exists() {
            std::fs::remove_file(state_path.as_std_path())
                .map_err(|source| Error::write(&state_path, source))?;
        }
        self.clean_overlay_wads()?;
        self.build()
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Compute each enabled mod's content fingerprint, in parallel.
    fn collect_active_mod_fingerprints(&self) -> (Vec<Option<u64>>, Option<BTreeMap<String, u64>>) {
        let fingerprints: Vec<Option<u64>> = self
            .enabled_mods
            .par_iter()
            .map(|m| m.cache_fingerprint())
            .collect();

        let by_id = self
            .enabled_mods
            .iter()
            .zip(&fingerprints)
            .map(|(m, fp)| fp.map(|f| (m.id.clone(), f)))
            .collect();

        (fingerprints, by_id)
    }

    /// Try the exact-match skip: when the previous state matches the current
    /// configuration and every recorded overlay WAD still exists on disk, the
    /// overlay is already up to date - finish the build without patching
    /// anything and return its result. Returns `None` when a real build is
    /// needed.
    #[allow(clippy::too_many_arguments)]
    fn try_exact_match_skip(
        &mut self,
        prev_state: Option<&OverlayState>,
        enabled_ids: &[String],
        mod_fingerprints: Option<&BTreeMap<String, u64>>,
        game_fingerprint: u64,
        blocked_wads: &[String],
        target_locales: &[String],
        start_time: std::time::Instant,
    ) -> Option<OverlayBuildResult> {
        let state = prev_state?;
        if !state.matches(
            enabled_ids,
            mod_fingerprints,
            game_fingerprint,
            blocked_wads,
            target_locales,
        ) {
            return None;
        }

        if !self.validate_wads_exist(state) {
            tracing::info!(
                "Overlay: state matched but some WADs missing, doing incremental repair"
            );

            return None;
        }

        tracing::info!("Overlay: exact match, skipping build");

        self.sweep_unexpected_overlay_files(&state.wad_fingerprints);
        self.last_linked_bin_offenders = state.linked_bin_offenders.clone();
        self.emit_progress(OverlayProgress::stage(OverlayStage::Complete));

        Some(OverlayBuildResult {
            overlay_root: self.overlay_root.clone(),
            wads_built: Vec::new(),
            wads_reused: state
                .wad_fingerprints
                .keys()
                .map(|k| self.overlay_root.join(k))
                .collect(),
            conflicts: Vec::new(),
            // A skipped build read no container, so it has nothing to report.
            checksum_mismatches: Vec::new(),
            build_time: start_time.elapsed(),
        })
    }

    /// Merge string overrides from all enabled mods and inject one synthetic
    /// override entry per target locale into `all_meta` (pass 1).
    ///
    /// Returns the patch plans keyed by stringtable chunk hash. Only the plan
    /// (merged override map + fingerprint) is computed here - the base table is
    /// read and patched lazily in pass 2, and only for WADs being rebuilt, so
    /// unchanged overrides never touch the game stringtable on incremental builds.
    fn build_string_patch_plans(
        &mut self,
        game_index: &GameIndex,
        target_locales: &[String],
        all_meta: &mut HashMap<WadHash, OverrideMeta>,
    ) -> Result<HashMap<WadHash, StringPatchPlan>> {
        let mut plans = HashMap::new();
        if target_locales.is_empty() {
            return Ok(plans);
        }

        let per_locale =
            strings::collect_effective_overrides(&mut self.enabled_mods, target_locales)?;
        if per_locale.is_empty() {
            return Ok(plans);
        }

        self.emit_progress(OverlayProgress::stage(
            OverlayStage::ApplyingStringOverrides,
        ));

        for (locale, overrides) in per_locale {
            let wad_name = format!("Global.{locale}.wad.client");
            let wad_abs_path = match game_index.find_wad(&wad_name) {
                Ok(path) => path.clone(),
                Err(Error::WadNotFound(_)) => {
                    tracing::warn!(
                        "String overrides target locale '{}' but the game has no '{}'; skipping",
                        locale,
                        wad_name
                    );
                    continue;
                }
                Err(other) => return Err(other),
            };
            let wad_rel_path = wad_abs_path
                .strip_prefix(&self.game_dir)
                .map_err(|_| GameDirError::WadOutsideGameDir {
                    game_dir: self.game_dir.clone(),
                    wad: wad_abs_path.clone(),
                })?
                .to_path_buf();

            let chunk_hash = strings::stringtable_chunk_hash(&locale);

            tracing::info!(
                "String overrides: {} entries for locale '{}' -> {}",
                overrides.len(),
                locale,
                wad_rel_path,
            );

            let plan = StringPatchPlan {
                locale,
                chunk_hash,
                wad_rel_path,
                overrides,
            };

            all_meta.insert(chunk_hash, plan.to_override_meta());
            plans.insert(chunk_hash, plan);
        }

        Ok(plans)
    }

    /// Mark `wads` as about to be rewritten in place, in one state write.
    ///
    /// A tail rewrite edits a file the injector will serve, so a build killed
    /// half way through one leaves a torn WAD behind. The marker is what the
    /// next build reads to know it must rebuild those WADs in full instead of
    /// trusting them. It is written once for the whole build rather than per
    /// WAD: over-invalidating costs a few extra full rebuilds - the designed
    /// fallback - and keeps serialized state writes out of the parallel loop.
    ///
    /// Does nothing when no WAD takes the in-place path.
    fn mark_dirty<'a>(
        &self,
        state_path: &Utf8Path,
        prev_state: Option<&OverlayState>,
        wads: impl Iterator<Item = &'a Utf8PathBuf>,
    ) -> Result<()> {
        let mut wads = wads.peekable();
        if wads.peek().is_none() {
            return Ok(());
        }

        let Some(state) = prev_state else {
            return Ok(());
        };

        let mut marker = state.clone();
        marker
            .dirty_wads
            .extend(wads.map(|path| path.as_str().to_string()));
        marker.save(state_path)
    }

    /// Check that all WADs listed in the state actually exist on disk.
    fn validate_wads_exist(&self, state: &OverlayState) -> bool {
        for wad_path in state.wad_fingerprints.keys() {
            let full_path = self.overlay_root.join(wad_path);
            if !full_path.as_std_path().exists() {
                tracing::warn!("Expected overlay WAD missing: {}", full_path);
                return false;
            }
        }
        true
    }

    /// Remove every file under the overlay's `DATA` tree that is not in
    /// `expected` (the current build's per-WAD set), cleaning up emptied
    /// parent directories.
    ///
    /// The injector serves the entire overlay directory, so orphans no state
    /// ever recorded - e.g. WADs from a build killed before it saved
    /// `overlay.json` - would reach the game and can fail its integrity scan.
    /// Runs on every build path, including the exact-match skip. Removal is
    /// best-effort: an undeletable file is logged and left for the next sweep.
    fn sweep_unexpected_overlay_files(&self, expected: &BTreeMap<String, u64>) {
        let data_dir = self.overlay_root.join("DATA");
        if !data_dir.as_std_path().is_dir() {
            return;
        }

        let expected: HashSet<String> = expected
            .keys()
            .map(|k| Self::normalize_overlay_rel(k))
            .collect();

        // Collect first, delete after - removing entries (and their emptied
        // parents) mid-walk would race the open directory handles.
        let mut to_remove: Vec<std::path::PathBuf> = Vec::new();
        let mut stack = vec![data_dir.into_std_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let keep = path
                    .strip_prefix(self.overlay_root.as_std_path())
                    .ok()
                    .and_then(|rel| rel.to_str())
                    .is_some_and(|rel| expected.contains(&Self::normalize_overlay_rel(rel)));
                if !keep {
                    to_remove.push(path);
                }
            }
        }

        for path in to_remove {
            tracing::info!("Removing unexpected overlay file: {}", path.display());
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    if let Ok(utf8) = Utf8PathBuf::from_path_buf(path) {
                        self.cleanup_empty_parents(&utf8);
                    }
                }
                Err(e) => tracing::warn!(
                    "Failed to remove unexpected overlay file {}: {}",
                    path.display(),
                    e
                ),
            }
        }
    }

    /// Normalize a path relative to the overlay root for comparison against
    /// [`OverlayState::wad_fingerprints`] keys: forward slashes, lowercased
    /// (overlay files were created from those same keys, but the separator
    /// depends on the platform the path was assembled on).
    fn normalize_overlay_rel(rel: &str) -> String {
        rel.replace('\\', "/").to_ascii_lowercase()
    }

    /// Remove all WAD files from the overlay directory.
    fn clean_overlay_wads(&self) -> Result<()> {
        let data_dir = self.overlay_root.join("DATA");
        if data_dir.as_std_path().exists() {
            std::fs::remove_dir_all(data_dir.as_std_path())
                .map_err(|source| Error::write(&data_dir, source))?;
        }
        Ok(())
    }

    /// Clean up empty parent directories after removing a stale WAD.
    fn cleanup_empty_parents(&self, path: &Utf8Path) {
        let mut current = path.parent();
        while let Some(dir) = current {
            if dir == self.overlay_root {
                break;
            }
            if dir.as_std_path().exists() {
                match std::fs::read_dir(dir.as_std_path()) {
                    Ok(mut entries) => {
                        if entries.next().is_some() {
                            break;
                        }
                        let _ = std::fs::remove_dir(dir.as_std_path());
                    }
                    Err(_) => break,
                }
            }
            current = dir.parent();
        }
    }

    /// Compute the effective blocklist from user-configured blocked WADs.
    fn effective_blocked_wads(&self) -> Vec<String> {
        let mut all: Vec<String> = self.blocked_wads.iter().cloned().collect();
        all.sort();
        all.dedup();
        all
    }

    /// Check if a WAD path is blocked from patching.
    fn is_wad_blocked(&self, wad_path: &Utf8Path) -> bool {
        let filename = wad_path.file_name().unwrap_or("").to_ascii_lowercase();
        self.blocked_wads.contains(&filename)
    }

    /// Emit a progress event if a callback was registered.
    fn emit_progress(&self, progress: OverlayProgress) {
        if let Some(callback) = &self.progress_callback {
            callback(progress);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::FsModContent;

    #[test]
    fn test_builder_creation() {
        let builder = OverlayBuilder::new(
            Utf8PathBuf::from("/game"),
            Utf8PathBuf::from("/profile/overlay"),
            Utf8PathBuf::from("/profile"),
        );

        assert_eq!(builder.game_dir, Utf8PathBuf::from("/game"));
        assert_eq!(builder.overlay_root, Utf8PathBuf::from("/profile/overlay"));
        assert_eq!(builder.state_dir, Utf8PathBuf::from("/profile"));
        assert_eq!(builder.enabled_mods.len(), 0);
    }

    #[test]
    fn test_set_enabled_mods() {
        let mut builder = OverlayBuilder::new(
            Utf8PathBuf::from("/game"),
            Utf8PathBuf::from("/profile/overlay"),
            Utf8PathBuf::from("/profile"),
        );

        builder.set_enabled_mods(vec![EnabledMod {
            id: "mod1".to_string(),
            content: Box::new(FsModContent::new(Utf8PathBuf::from("/mods/mod1"))),
            enabled_layers: None,
        }]);

        assert_eq!(builder.enabled_mods.len(), 1);
    }

    #[test]
    fn test_override_meta_types() {
        let meta = OverrideMeta {
            content_hash: ContentHash(0x1234),
            uncompressed_size: 100,
            source: OverrideSource::LayerWad {
                mod_id: "test-mod".to_string(),
                layer: "base".to_string(),
                wad_name: "Test.wad.client".to_string(),
                rel_path: Utf8PathBuf::from("data/file.bin"),
            },
            fallback_wad: None,
            unlocalized_wad: None,
            linked_bins: Vec::new(),
        };
        assert_eq!(meta.content_hash, ContentHash(0x1234));
        assert_eq!(meta.uncompressed_size, 100);
    }

    fn dummy_meta() -> OverrideMeta {
        OverrideMeta {
            content_hash: ContentHash(0),
            uncompressed_size: 0,
            source: OverrideSource::LayerWad {
                mod_id: "m".to_string(),
                layer: "base".to_string(),
                wad_name: "Aatrox.wad.client".to_string(),
                rel_path: Utf8PathBuf::from("data/x.bin"),
            },
            fallback_wad: None,
            unlocalized_wad: None,
            linked_bins: Vec::new(),
        }
    }

    fn game_index_with_hashes(hashes: HashMap<WadHash, Vec<Utf8PathBuf>>) -> GameIndex {
        GameIndex {
            wad_index: HashMap::new(),
            hash_index: hashes,
            game_fingerprint: 7,
            subchunktoc_blocked: HashSet::new(),
        }
    }

    #[test]
    fn from_meta_counts_overrides_per_wad() {
        let aatrox = Utf8PathBuf::from("DATA/FINAL/Champions/Aatrox.wad.client");
        let map11 = Utf8PathBuf::from("DATA/FINAL/Maps/Shipping/Map11/Base/Map11.wad.client");
        let map12 = Utf8PathBuf::from("DATA/FINAL/Maps/Shipping/Map12/Base/Map12.wad.client");

        // 0xBA5E is a champion base chunk the game duplicates into both map WADs
        // (spillover); 0x5C1 (a skin chunk) lives only in the champion WAD.
        let mut hash_index: HashMap<WadHash, Vec<Utf8PathBuf>> = HashMap::new();
        hash_index.insert(
            WadHash(0xBA5E),
            vec![aatrox.clone(), map11.clone(), map12.clone()],
        );
        hash_index.insert(WadHash(0x5C1), vec![aatrox.clone()]);
        let game_index = game_index_with_hashes(hash_index);

        let mut mod_meta: HashMap<WadHash, OverrideMeta> = HashMap::new();
        mod_meta.insert(WadHash(0xBA5E), dummy_meta());
        mod_meta.insert(WadHash(0x5C1), dummy_meta());

        let report =
            ModWadReport::from_meta("aatrox-skin".to_string(), &mod_meta, None, &game_index);

        // Champion WAD holds both overrides; each map holds only the spilled base
        // chunk - the asymmetry that lets a consumer pick the champion as primary.
        // Entries are sorted by path and deduplicated.
        assert_eq!(
            report.affected_wads,
            vec![
                AffectedWad {
                    path: aatrox.as_str().into(),
                    override_count: 2,
                },
                AffectedWad {
                    path: map11.as_str().into(),
                    override_count: 1,
                },
                AffectedWad {
                    path: map12.as_str().into(),
                    override_count: 1,
                },
            ]
        );
        // wad_paths() drops the counts.
        assert_eq!(
            report.wad_paths().collect::<Vec<_>>(),
            vec![aatrox.as_str(), map11.as_str(), map12.as_str()]
        );
        // override_count is distinct overrides, not the per-WAD sum (which is 4).
        assert_eq!(report.override_count, 2);
        assert_eq!(report.game_index_fingerprint, 7);
    }

    #[test]
    fn route_targets_adds_declared_wad_for_cross_wad_import() {
        let ahri = Utf8PathBuf::from("DATA/FINAL/Champions/Ahri.wad.client");
        let aatrox = Utf8PathBuf::from("DATA/FINAL/Champions/Aatrox.wad.client");
        let mut hash_index = HashMap::new();
        hash_index.insert(WadHash(0xA881), vec![ahri.clone()]);
        let game_index = game_index_with_hashes(hash_index);

        // A copy of an Ahri chunk shipped under the Aatrox WAD dir (cross-WAD
        // import, identical or modified alike): route to Ahri (hash match) AND
        // import into Aatrox (declared WAD). Both WADs receive the same bytes
        // so their compressed checksums stay consistent in-game.
        let mut meta = dummy_meta();
        meta.fallback_wad = Some(aatrox.clone());
        assert_eq!(
            meta.route_targets(WadHash(0xA881), &game_index),
            vec![ahri.as_path(), aatrox.as_path()]
        );

        // Declared WAD already among the hash matches: no extra target.
        let mut meta = dummy_meta();
        meta.fallback_wad = Some(ahri.clone());
        assert_eq!(
            meta.route_targets(WadHash(0xA881), &game_index),
            vec![ahri.as_path()]
        );
    }

    #[test]
    fn route_targets_carries_a_localized_declaration_to_its_sibling() {
        let graves = Utf8PathBuf::from("DATA/FINAL/Champions/Graves.wad.client");
        let graves_en = Utf8PathBuf::from("DATA/FINAL/Champions/Graves.en_US.wad.client");
        let game_index = game_index_with_hashes(HashMap::new());

        // A brand-new asset declared into the localized WAD. Only that WAD would
        // hold it, which no other locale installs and the integrity scan never
        // reads, so the sibling carries it too.
        let mut meta = dummy_meta();
        meta.fallback_wad = Some(graves_en.clone());
        meta.unlocalized_wad = Some(graves.clone());
        assert_eq!(
            meta.route_targets(WadHash(0xF00D), &game_index),
            vec![graves_en.as_path(), graves.as_path()]
        );
    }

    #[test]
    fn route_targets_leaves_localized_content_in_its_own_wad() {
        let graves = Utf8PathBuf::from("DATA/FINAL/Champions/Graves.wad.client");
        let graves_en = Utf8PathBuf::from("DATA/FINAL/Champions/Graves.en_US.wad.client");
        let mut hash_index = HashMap::new();
        hash_index.insert(WadHash(0x1_0CA1), vec![graves_en.clone()]);
        let game_index = game_index_with_hashes(hash_index);

        // Genuinely localized content hash-matches the localized WAD, so it never
        // reaches the fallback and the sibling stays out of it.
        let mut meta = dummy_meta();
        meta.fallback_wad = Some(graves_en.clone());
        meta.unlocalized_wad = Some(graves.clone());
        assert_eq!(
            meta.route_targets(WadHash(0x1_0CA1), &game_index),
            vec![graves_en.as_path()]
        );
    }

    #[test]
    fn route_targets_never_repeats_the_sibling() {
        let graves = Utf8PathBuf::from("DATA/FINAL/Champions/Graves.wad.client");
        let graves_en = Utf8PathBuf::from("DATA/FINAL/Champions/Graves.en_US.wad.client");
        let mut hash_index = HashMap::new();
        hash_index.insert(WadHash(0xBA5E), vec![graves.clone()]);
        let game_index = game_index_with_hashes(hash_index);

        // An existing asset misplaced into the localized WAD: the hash match
        // already names the sibling, so only the declared WAD is added.
        let mut meta = dummy_meta();
        meta.fallback_wad = Some(graves_en.clone());
        meta.unlocalized_wad = Some(graves.clone());
        assert_eq!(
            meta.route_targets(WadHash(0xBA5E), &game_index),
            vec![graves.as_path(), graves_en.as_path()]
        );
    }

    #[test]
    fn route_targets_ignores_heuristic_fallback_for_raw_sources() {
        let ahri = Utf8PathBuf::from("DATA/FINAL/Champions/Ahri.wad.client");
        let aatrox = Utf8PathBuf::from("DATA/FINAL/Champions/Aatrox.wad.client");
        let mut hash_index = HashMap::new();
        hash_index.insert(WadHash(0xA881), vec![ahri.clone()]);
        let game_index = game_index_with_hashes(hash_index);

        // A RAW override's fallback_wad is the dominant-WAD heuristic, not a
        // declared placement - hash matches must not be widened by it.
        let meta = OverrideMeta {
            content_hash: ContentHash(1),
            uncompressed_size: 1,
            source: OverrideSource::Raw {
                mod_id: "m".to_string(),
                rel_path: Utf8PathBuf::from("assets/x.bin"),
            },
            fallback_wad: Some(aatrox.clone()),
            unlocalized_wad: None,
            linked_bins: Vec::new(),
        };
        assert_eq!(
            meta.route_targets(WadHash(0xA881), &game_index),
            vec![ahri.as_path()]
        );
        // ...but it still routes brand-new hashes.
        assert_eq!(
            meta.route_targets(WadHash(0xF00D), &game_index),
            vec![aatrox.as_path()]
        );
    }

    #[test]
    fn from_meta_counts_cross_wad_import_in_declared_wad() {
        let ahri = Utf8PathBuf::from("DATA/FINAL/Champions/Ahri.wad.client");
        let aatrox = Utf8PathBuf::from("DATA/FINAL/Champions/Aatrox.wad.client");
        let mut hash_index = HashMap::new();
        hash_index.insert(WadHash(0xA881), vec![ahri.clone()]);
        let game_index = game_index_with_hashes(hash_index);

        let mut meta = dummy_meta();
        meta.fallback_wad = Some(aatrox.clone());
        let mut mod_meta = HashMap::new();
        mod_meta.insert(WadHash(0xA881), meta);

        let report = ModWadReport::from_meta("import".to_string(), &mod_meta, None, &game_index);
        assert_eq!(
            report.affected_wads,
            vec![
                AffectedWad {
                    path: aatrox.as_str().into(),
                    override_count: 1,
                },
                AffectedWad {
                    path: ahri.as_str().into(),
                    override_count: 1,
                },
            ],
            "a cross-WAD import must show up in the mod's declared WAD too"
        );
    }

    #[test]
    fn from_meta_uses_fallback_wad_when_no_game_match() {
        let custom = Utf8PathBuf::from("DATA/FINAL/Champions/NewChamp.wad.client");
        let game_index = game_index_with_hashes(HashMap::new());

        let mut meta = dummy_meta();
        meta.fallback_wad = Some(custom.clone());
        let mut mod_meta = HashMap::new();
        mod_meta.insert(WadHash(0xABCD), meta);

        let report = ModWadReport::from_meta("new".to_string(), &mod_meta, None, &game_index);
        assert_eq!(
            report.affected_wads,
            vec![AffectedWad {
                path: custom.as_str().into(),
                override_count: 1,
            }]
        );
    }
}
