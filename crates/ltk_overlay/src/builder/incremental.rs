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

use super::*;
use crate::state::WadLayoutRecord;
use crate::wad_builder::{PreparedOverride, SourceWadIdentity, merged_entry_count};
use ltk_wad::{Wad, WadChunk, WadHash};

/// A WAD this build can rebuild by rewriting only its override tail.
pub(crate) struct TailRewrite {
    /// The verified record this rewrite starts from. Its `overrides` map is the
    /// *previous* set; the caller replaces it when recording the result.
    pub(crate) record: WadLayoutRecord,

    /// The TOC every source chunk would have with no override applied, keyed by
    /// path hash. The rewrite overwrites the entries it puts in the tail.
    pub(crate) base_entries: BTreeMap<u64, WadChunk>,

    /// Path hashes this build intends to put in the tail, ascending. A hash
    /// whose data turns out to be unresolvable is dropped at write time and
    /// keeps its [`base_entries`](Self::base_entries) entry.
    pub(crate) tail_hashes: Vec<u64>,

    /// Overrides whose compressed bytes were lifted out of the old tail because
    /// their content is unchanged. The rest of [`tail_hashes`](Self::tail_hashes)
    /// is resolved and compressed by the normal pass-2 path.
    pub(crate) reused: HashMap<u64, PreparedOverride>,
}

impl OverlayBuilder {
    /// Work out which of `wads_to_build` can take the tail-rewrite path.
    ///
    /// WADs that cannot are simply absent from the result and fall through to a
    /// full rebuild; the reason is logged at debug level. Nothing here writes.
    pub(crate) fn plan_tail_rewrites(
        &self,
        wads_to_build: &[Utf8PathBuf],
        wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<u64>>,
        all_meta: &HashMap<u64, OverrideMeta>,
        prev_state: Option<&OverlayState>,
    ) -> BTreeMap<Utf8PathBuf, TailRewrite> {
        let Some(state) = prev_state else {
            return BTreeMap::new();
        };

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
    /// Returns the reason the WAD must be rebuilt in full. Every one of them is
    /// expected traffic, not a failure: the caller logs it and moves on.
    fn plan_one_tail_rewrite(
        &self,
        relative_path: &Utf8Path,
        record: &WadLayoutRecord,
        new_overrides: &HashSet<u64>,
        all_meta: &HashMap<u64, OverrideMeta>,
    ) -> Result<TailRewrite> {
        let overlay_path = self.overlay_root.join(relative_path);
        let source_path = self.game_dir.join(relative_path);

        // The record came out of a JSON file that anything could have written,
        // and its offsets become seek targets below.
        record.layout.validate()?;

        // The game WAD must be the one this overlay was built from.
        let source_file = File::open(source_path.as_std_path())
            .map_err(|source| Error::read(&source_path, source))?;
        let source_metadata = source_file
            .metadata()
            .map_err(|source| Error::read(&source_path, source))?;
        let source = Wad::mount(BufReader::new(&source_file))?;
        let identity = SourceWadIdentity::new(&source_metadata, source.chunks())?;
        if identity != record.source {
            return Err(Error::Other(format!(
                "the game WAD changed since the overlay was built ({identity:?} \
                 against the recorded {:?})",
                record.source
            )));
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
            return Err(Error::Other(
                "the overlay WAD carries a different signature than the game WAD".to_string(),
            ));
        }
        if overlay_len < record.layout.tail_offset {
            return Err(Error::Other(format!(
                "the overlay WAD is {overlay_len} bytes, short of the recorded tail \
                 offset {}",
                record.layout.tail_offset
            )));
        }
        verify_passthrough_toc(&record.layout, source.chunks(), overlay.chunks(), record)?;

        // The new override set must fit the TOC the file already reserved.
        let base_entries = base_entries(&record.layout, source.chunks())?;
        let entry_count = merged_entry_count(&base_entries, new_overrides.iter().copied());
        if !record.layout.admits_entry_count(entry_count) {
            return Err(Error::Other(format!(
                "the new override set needs {entry_count} TOC entries, not the \
                 {} this file reserved",
                record.layout.toc_capacity
            )));
        }

        let mut tail_hashes: Vec<u64> = new_overrides.iter().copied().collect();
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
fn verify_passthrough_toc(
    layout: &WadTailLayout,
    source: &WadChunks,
    overlay: &WadChunks,
    record: &WadLayoutRecord,
) -> Result<()> {
    let added = record
        .overrides
        .keys()
        .filter(|hash| !source.contains(WadHash(**hash)))
        .count();
    let expected = source.len() + added;
    if overlay.len() != expected {
        return Err(Error::Other(format!(
            "the overlay WAD holds {} chunks, not the {expected} the record implies",
            overlay.len()
        )));
    }

    for chunk in source {
        if record.overrides.contains_key(&chunk.path_hash.0) {
            continue;
        }
        let expected = layout.shift(chunk)?;
        match overlay.get(chunk.path_hash) {
            Some(found) if *found == expected => {}
            _ => {
                return Err(Error::Other(format!(
                    "the overlay WAD's entry for chunk {:016x} is not the source entry \
                     shifted into the copied region",
                    chunk.path_hash
                )));
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
/// to apply, say) stays a passthrough instead of failing the rebuild - the same
/// outcome the full-rebuild path gives it.
fn base_entries(layout: &WadTailLayout, source: &WadChunks) -> Result<BTreeMap<u64, WadChunk>> {
    source
        .iter()
        .map(|chunk| Ok((chunk.path_hash.0, layout.shift(chunk)?)))
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
    tail_hashes: &[u64],
    all_meta: &HashMap<u64, OverrideMeta>,
) -> Result<HashMap<u64, PreparedOverride>> {
    let mut reused = HashMap::new();
    for &path_hash in tail_hashes {
        let Some(meta) = all_meta.get(&path_hash) else {
            continue;
        };
        if record.overrides.get(&path_hash) != Some(&meta.content_hash) {
            continue;
        }
        let Some(chunk) = overlay.chunks().get(WadHash(path_hash)).copied() else {
            continue;
        };
        if (chunk.data_offset as u64) < record.layout.tail_offset {
            return Err(Error::Other(format!(
                "the overlay WAD's override {path_hash:016x} points into the copied \
                 region rather than the tail"
            )));
        }

        let compressed = overlay.load_chunk_raw(&chunk)?;
        reused.insert(
            path_hash,
            PreparedOverride::from_wad_bytes(&chunk, compressed)?,
        );
    }

    Ok(reused)
}
