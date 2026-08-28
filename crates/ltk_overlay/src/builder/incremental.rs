//! Deciding which overlay WADs can be rebuilt by rewriting their tail alone.
//!
//! A WAD whose override *bytes* changed but whose chunk set did not can keep
//! its copied source data region: only the tail and the TOC need rewriting.
//! That turns a multi-gigabyte copy into a write of the mod's own bytes.
//!
//! Everything here is a trust check. The recorded [`WadLayoutRecord`] is a
//! hint about a file the builder wrote earlier, and any of it can be stale -
//! the game may have been patched, the overlay edited, a previous build killed
//! part-way. Each precondition is therefore re-verified against the files
//! themselves, cheaply, and any doubt drops the WAD onto the full-rebuild path,
//! which is the same code that wrote it the first time.

use crate::builder::{OverlayBuilder, OverrideMeta};
use crate::error::{Error, Result};
use crate::state::{OverlayState, WadLayoutRecord};
use crate::wad_builder::{PreparedOverride, SourceWadIdentity, WadTailLayout, merged_entry_count};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_wad::{Wad, WadChunk, WadChunks, WadHash};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;

/// Why a WAD cannot keep its file and rewrite only its tail.
///
/// None of these is an [`Error`]: they are what a healthy install produces
/// when the game is patched, an overlay is edited, or a mod gains a file. The
/// planner hands one back, the caller logs it and rebuilds that WAD in full.
#[derive(Debug)]
enum RebuildReason {
    /// A file or record the plan depends on could not be read, mounted, or used.
    Unusable(Error),
    /// The recorded layout's own numbers do not hang together.
    IncoherentLayout(Error),
    /// The game WAD is no longer the one this overlay was built from.
    GameWadChanged {
        found: SourceWadIdentity,
        recorded: SourceWadIdentity,
    },
    /// The overlay WAD carries a different signature than the game WAD.
    SignatureMismatch,
    /// The overlay WAD is shorter than the recorded tail offset.
    OverlayTooShort { len: u64, tail_offset: u64 },
    /// The overlay holds a different number of chunks than the record implies.
    ChunkCountMismatch { found: usize, expected: usize },
    /// A transient entry is not the source entry shifted into the region.
    TransientDiverged { path_hash: WadHash },
    /// The new override set does not fit the TOC capacity the file reserved.
    CapacityMismatch { needed: usize, reserved: u32 },
    /// An override the record calls reusable points outside the tail.
    OverrideOutsideTail { path_hash: WadHash },
}

impl From<Error> for RebuildReason {
    fn from(error: Error) -> Self {
        Self::Unusable(error)
    }
}

impl From<ltk_wad::WadError> for RebuildReason {
    fn from(error: ltk_wad::WadError) -> Self {
        Self::Unusable(error.into())
    }
}

impl std::fmt::Display for RebuildReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unusable(error) => write!(f, "{error}"),
            Self::IncoherentLayout(error) => write!(f, "the recorded layout is unusable: {error}"),
            Self::GameWadChanged { found, recorded } => write!(
                f,
                "the game WAD changed since the overlay was built \
                 ({found:?} against the recorded {recorded:?})"
            ),
            Self::SignatureMismatch => write!(
                f,
                "the overlay WAD carries a different signature than the game WAD"
            ),
            Self::OverlayTooShort { len, tail_offset } => write!(
                f,
                "the overlay WAD is {len} bytes, short of the recorded tail \
                 offset {tail_offset}"
            ),
            Self::ChunkCountMismatch { found, expected } => write!(
                f,
                "the overlay WAD holds {found} chunks, not the {expected} the \
                 record implies"
            ),
            Self::TransientDiverged { path_hash } => write!(
                f,
                "the overlay WAD's entry for chunk {path_hash:016x} is not the \
                 source entry shifted into the copied region"
            ),
            Self::CapacityMismatch { needed, reserved } => write!(
                f,
                "the new override set needs {needed} TOC entries, not the \
                 {reserved} this file reserved"
            ),
            Self::OverrideOutsideTail { path_hash } => write!(
                f,
                "the overlay WAD's override {path_hash:016x} points into the \
                 copied region rather than the tail"
            ),
        }
    }
}

/// Whether the overlay files an earlier build wrote are still on disk.
///
/// A tail rewrite keeps the file it finds there, so this is the precondition
/// for planning one at all. The full-rebuild path empties the overlay directory
/// before any planning happens, which leaves every record pointing at a file
/// that is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviousOverlay {
    /// Untouched since the last build, so a rewrite has a file to keep.
    OnDisk,
    /// Already deleted by the full-rebuild path.
    Wiped,
}

/// A WAD this build can rebuild by rewriting only its override tail.
pub(crate) struct TailRewrite {
    /// The verified record this rewrite starts from. Its `overrides` map is the
    /// *previous* set; the caller replaces it when recording the result.
    pub(crate) record: WadLayoutRecord,

    /// The TOC every source chunk would have with no override applied, keyed by
    /// path hash. The rewrite overwrites the entries it puts in the tail.
    pub(crate) base_entries: BTreeMap<WadHash, WadChunk>,

    /// Path hashes this build intends to put in the tail, ascending. A hash
    /// whose data turns out to be unresolvable is dropped at write time and
    /// keeps its [`base_entries`](Self::base_entries) entry.
    pub(crate) tail_hashes: Vec<WadHash>,

    /// Overrides whose compressed bytes were lifted out of the old tail because
    /// their content is unchanged. The rest of [`tail_hashes`](Self::tail_hashes)
    /// is resolved and compressed by the normal pass-2 path.
    pub(crate) reused: HashMap<WadHash, PreparedOverride>,
}

impl OverlayBuilder {
    /// Work out which of `wads_to_build` can take the tail-rewrite path.
    ///
    /// WADs that cannot are simply absent from the result and fall through to a
    /// full rebuild; the reason is logged at debug level. Nothing here writes.
    pub(crate) fn plan_tail_rewrites(
        &self,
        wads_to_build: &[Utf8PathBuf],
        wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<WadHash>>,
        all_meta: &HashMap<WadHash, OverrideMeta>,
        prev_state: Option<&OverlayState>,
        previous_overlay: PreviousOverlay,
    ) -> BTreeMap<Utf8PathBuf, TailRewrite> {
        // Verifying a record costs a mount and a TOC hash of the game WAD, so
        // checking one against a file that is already gone is pure loss.
        if previous_overlay == PreviousOverlay::Wiped {
            return BTreeMap::new();
        }
        let Some(state) = prev_state else {
            return BTreeMap::new();
        };
        // Records written by another schema version describe files this build
        // has no reason to believe in, whatever the rest of the checks find.
        if !state.is_current_version() {
            return BTreeMap::new();
        }

        let mut rewrites = BTreeMap::new();
        for relative_path in wads_to_build {
            let Some(record) = state.wad_layout(relative_path.as_str()) else {
                continue;
            };
            let Some(new_overrides) = wad_hash_sets.get(relative_path) else {
                continue;
            };

            match self.plan_one_tail_rewrite(relative_path, record, new_overrides, all_meta) {
                Ok(rewrite) => {
                    rewrites.insert(relative_path.clone(), rewrite);
                }
                Err(reason) => tracing::debug!("Full rebuild for {}: {}", relative_path, reason),
            }
        }

        if !rewrites.is_empty() {
            tracing::info!(
                "Rewriting the tail of {} of {} WAD(s) instead of rebuilding them",
                rewrites.len(),
                wads_to_build.len()
            );
        }

        rewrites
    }

    /// Verify one WAD against its record and plan its rewrite.
    ///
    /// # Errors
    ///
    /// Returns the [reason](RebuildReason) the WAD must be rebuilt in full.
    fn plan_one_tail_rewrite(
        &self,
        relative_path: &Utf8Path,
        record: &WadLayoutRecord,
        new_overrides: &HashSet<WadHash>,
        all_meta: &HashMap<WadHash, OverrideMeta>,
    ) -> std::result::Result<TailRewrite, RebuildReason> {
        let overlay_path = self.overlay_root.join(relative_path);
        let source_path = self.game_dir.join(relative_path);

        // The record came out of a JSON file that anything could have written,
        // and its offsets become seek targets below.
        record
            .layout
            .validate()
            .map_err(RebuildReason::IncoherentLayout)?;

        // The game WAD must be the one this overlay was built from.
        let source_file = File::open(source_path.as_std_path())
            .map_err(|source| Error::read(&source_path, source))?;
        let source_metadata = source_file
            .metadata()
            .map_err(|source| Error::read(&source_path, source))?;
        let source = Wad::mount(BufReader::new(&source_file))?;
        let identity = SourceWadIdentity::new(&source_metadata, source.chunks())?;
        if identity != record.source {
            return Err(RebuildReason::GameWadChanged {
                found: identity,
                recorded: record.source,
            });
        }

        // The overlay file must be the one the record describes.
        let overlay_file = File::open(overlay_path.as_std_path())
            .map_err(|source| Error::read(&overlay_path, source))?;
        let overlay_len = overlay_file
            .metadata()
            .map_err(|source| Error::read(&overlay_path, source))?
            .len();
        let mut overlay = Wad::mount(BufReader::new(&overlay_file))?;
        if overlay.signature() != source.signature() {
            return Err(RebuildReason::SignatureMismatch);
        }
        if overlay_len < record.layout.tail_offset {
            return Err(RebuildReason::OverlayTooShort {
                len: overlay_len,
                tail_offset: record.layout.tail_offset,
            });
        }
        verify_transient_toc(source.chunks(), overlay.chunks(), record)?;

        // The new override set must fit the TOC the file already reserved.
        let base_entries = base_entries(&record.layout, source.chunks())?;
        let entry_count = merged_entry_count(&base_entries, new_overrides.iter().copied());
        if !record.layout.admits_entry_count(entry_count) {
            return Err(RebuildReason::CapacityMismatch {
                needed: entry_count,
                reserved: record.layout.toc_capacity,
            });
        }

        let mut tail_hashes: Vec<WadHash> = new_overrides.iter().copied().collect();
        tail_hashes.sort_unstable();
        let reused = reuse_unchanged_overrides(&mut overlay, record, &tail_hashes, all_meta)?;

        tracing::debug!(
            "Tail rewrite for {}: {} override(s), {} reused from the old tail",
            relative_path,
            tail_hashes.len(),
            reused.len()
        );

        Ok(TailRewrite {
            record: record.clone(),
            base_entries,
            tail_hashes,
            reused,
        })
    }
}

/// Check that every chunk the overlay passed through still sits where the
/// recorded layout says it does.
///
/// This is the check that makes the copied region trustworthy without reading a
/// byte of it: two TOCs compared in memory, milliseconds even for the largest
/// map WAD. Chunks the previous build overrode are skipped - they live in the
/// tail, which is about to be thrown away.
fn verify_transient_toc(
    source: &WadChunks,
    overlay: &WadChunks,
    record: &WadLayoutRecord,
) -> std::result::Result<(), RebuildReason> {
    let added = record
        .overrides
        .keys()
        .filter(|hash| !source.contains(**hash))
        .count();
    let expected = source.len() + added;
    if overlay.len() != expected {
        return Err(RebuildReason::ChunkCountMismatch {
            found: overlay.len(),
            expected,
        });
    }

    for chunk in source {
        if record.overrides.contains_key(&chunk.path_hash) {
            continue;
        }
        let expected = record.layout.shifted(chunk)?;
        match overlay.get(chunk.path_hash) {
            Some(found) if *found == expected => {}
            _ => {
                return Err(RebuildReason::TransientDiverged {
                    path_hash: chunk.path_hash,
                });
            }
        }
    }

    Ok(())
}

/// The TOC entry every source chunk would have with no override at all.
///
/// The copied region holds all of them, overridden ones included, so each is
/// just the source entry shifted. The rewrite then overwrites the entries of the
/// chunks it actually puts in the tail, which means two things fall out for
/// free: an override that was *removed* reverts to the game's bytes, and one
/// whose data could not be resolved this build (a stringtable patch that failed
/// to apply, say) stays transient instead of failing the rebuild - the same
/// outcome the full-rebuild path gives it.
fn base_entries(layout: &WadTailLayout, source: &WadChunks) -> Result<BTreeMap<WadHash, WadChunk>> {
    source
        .iter()
        .map(|chunk| Ok((chunk.path_hash, layout.shifted(chunk)?)))
        .collect()
}

/// Lift the compressed bytes of every unchanged override out of the old tail.
///
/// An override counts as unchanged when the record's content hash for it still
/// matches this build's, which means its mod never needs to be read and its
/// bytes never need compressing again. The bytes are checksummed as they are
/// read, so a corrupted tail costs a full rebuild instead of producing a WAD
/// the game would reject.
fn reuse_unchanged_overrides<S: std::io::Read + std::io::Seek>(
    overlay: &mut Wad<S>,
    record: &WadLayoutRecord,
    tail_hashes: &[WadHash],
    all_meta: &HashMap<WadHash, OverrideMeta>,
) -> std::result::Result<HashMap<WadHash, PreparedOverride>, RebuildReason> {
    let mut reused = HashMap::new();
    for &path_hash in tail_hashes {
        let Some(meta) = all_meta.get(&path_hash) else {
            continue;
        };
        if record.overrides.get(&path_hash) != Some(&meta.content_hash) {
            continue;
        }
        let Some(chunk) = overlay.chunks().get(path_hash).copied() else {
            continue;
        };
        if (chunk.data_offset as u64) < record.layout.tail_offset {
            return Err(RebuildReason::OverrideOutsideTail { path_hash });
        }

        let compressed = overlay.load_chunk_raw(&chunk)?;
        reused.insert(
            path_hash,
            PreparedOverride::from_wad_bytes(&chunk, compressed)?,
        );
    }

    Ok(reused)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{hash, write_game_wad};
    use crate::utils::ContentHash;
    use crate::wad_builder::build_patched_wad;

    const WAD_REL: &str = "DATA/FINAL/Champions/Test.wad.client";
    const SKIN: &str = "assets/characters/test/skins/skin0.dds";
    const VFX: &str = "assets/characters/test/particles.bin";

    /// A game WAD, an overlay built from it, and the state that vouches for it.
    ///
    /// Everything is genuine: the overlay is written by the same full-rebuild
    /// path a first build takes, and the record comes out of that build's own
    /// stats, so the planner has nothing to object to.
    struct Fixture {
        builder: OverlayBuilder,
        state: OverlayState,
        wads: Vec<Utf8PathBuf>,
        hash_sets: BTreeMap<Utf8PathBuf, HashSet<WadHash>>,
    }

    impl Fixture {
        fn new(root: &Utf8Path) -> Self {
            let game_dir = root.join("Game");
            let overlay_root = root.join("overlay");
            let source_path = game_dir.join(WAD_REL);
            write_game_wad(
                &source_path,
                &[(SKIN, b"the original skin"), (VFX, b"the original vfx")],
            );

            let override_hashes: HashSet<WadHash> = [hash(SKIN)].into_iter().collect();
            let prepared = PreparedOverride::compress(hash(SKIN), b"a modded skin")
                .expect("the override compresses");
            let stats = build_patched_wad(
                &source_path,
                &overlay_root.join(WAD_REL),
                &override_hashes,
                |_| Ok(prepared.clone()),
            )
            .expect("the overlay WAD builds");

            let mut state = OverlayState::default();
            state.wad_layouts.insert(
                WAD_REL.to_string(),
                WadLayoutRecord {
                    source: stats.source,
                    layout: stats.layout,
                    overrides: BTreeMap::from([(hash(SKIN), ContentHash(0xC0FFEE))]),
                },
            );

            Self {
                builder: OverlayBuilder::new(game_dir, overlay_root, root.join("state")),
                state,
                wads: vec![Utf8PathBuf::from(WAD_REL)],
                hash_sets: BTreeMap::from([(Utf8PathBuf::from(WAD_REL), override_hashes)]),
            }
        }

        fn plan_with(
            &self,
            state: &OverlayState,
            previous: PreviousOverlay,
        ) -> BTreeMap<Utf8PathBuf, TailRewrite> {
            self.builder.plan_tail_rewrites(
                &self.wads,
                &self.hash_sets,
                &HashMap::new(),
                Some(state),
                previous,
            )
        }
    }

    /// The fixture must sit on the fast path, or the tests below prove nothing.
    #[test]
    fn an_untouched_overlay_plans_a_tail_rewrite() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let fixture = Fixture::new(&root);

        assert_eq!(
            fixture
                .plan_with(&fixture.state, PreviousOverlay::OnDisk)
                .len(),
            1
        );
    }

    /// A state file from an older schema says nothing trustworthy about the
    /// WADs on disk, whatever its records claim.
    #[test]
    fn a_stale_state_version_plans_no_tail_rewrites() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let fixture = Fixture::new(&root);

        let mut stale = fixture.state.clone();
        stale.version -= 1;

        assert!(
            fixture
                .plan_with(&stale, PreviousOverlay::OnDisk)
                .is_empty()
        );
    }

    /// A full rebuild deletes the overlay directory before planning begins, so
    /// every record describes a file that is gone. Planning anyway costs a mount
    /// and a full TOC hash of every game WAD to reach that conclusion one file
    /// at a time.
    #[test]
    fn a_full_rebuild_plans_no_tail_rewrites() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let fixture = Fixture::new(&root);

        assert!(
            fixture
                .plan_with(&fixture.state, PreviousOverlay::Wiped)
                .is_empty()
        );
    }
}
