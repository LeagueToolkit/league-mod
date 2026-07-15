//! Bin-entry resolution over linked property-bins.
//!
//! Resolves a bin entry the way the game does: start at a root bin (e.g.
//! `data/characters/{champ}/skins/skin0.bin`), and if it does not define the
//! entry, follow its `linked` bins breadth-first within the same chunk
//! source until one does.
//!
//! Unlike a pure diagnostic walk, corrupt bins encountered along the way are
//! **recorded** in the [`ResolveOutcome`] rather than only logged: a strict
//! consumer (the in-game verifier) treats any corruption as fatal, while a
//! reporting consumer (a mod manager) attaches them to its diagnostics.

use std::collections::{HashSet, VecDeque};
use std::io::Cursor;

use ltk_hash::{BinHash, Hash as _, WadHash};
use ltk_meta::{Bin, BinObject};
use thiserror::Error;

use crate::source::ChunkSource;

/// Upper bound on the number of bins visited while following `linked` bins,
/// guarding against absurd or cyclic dependency graphs.
pub const MAX_LINKED_BINS: usize = 64;

/// Why a bin entry could not be resolved from a root bin and its linked bins.
///
/// Distinct outcomes so callers can report precisely: a missing root bin is
/// "nothing to verify here" (non-champion WAD), while a champion WAD whose
/// skin entry cannot be found is itself diagnostic-worthy.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResolveError {
    #[error("bin '{bin_path}' is not in the WAD")]
    RootBinMissing { bin_path: String },

    #[error("entry {entry:08x} not found in '{root}' or its linked bins")]
    EntryNotFound { root: String, entry: BinHash },

    #[error("entry {entry:08x} in '{bin_path}' has class {class:08x}, expected {expected:08x}")]
    WrongClass {
        bin_path: String,
        entry: BinHash,
        class: BinHash,
        expected: BinHash,
    },

    #[error("gave up resolving entry from '{root}': more than {limit} linked bins")]
    TooManyLinkedBins { root: String, limit: usize },
}

/// A bin that is present in the chunk source but could not be read or parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorruptBin {
    /// The bin path as referenced (root path or a `linked` entry).
    pub bin_path: String,
    /// xxh64 chunk hash of `bin_path`.
    pub name_hash: u64,
    /// Human-readable load/parse failure.
    pub reason: String,
}

/// A bin entry resolved by walking a root bin and its linked bins.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBinObject {
    /// The bin file that defines the entry.
    pub bin_path: String,
    /// xxh64 chunk hash of `bin_path`.
    pub bin_name_hash: u64,
    /// The entry object.
    pub object: BinObject,
}

/// The result of a resolve walk: the entry (or why it could not be found),
/// plus every corrupt bin encountered along the way. Corruption never aborts
/// the walk — the entry may still be defined by a later linked bin — but it
/// is always surfaced so callers can decide how much it matters.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolveOutcome {
    pub entry: Result<ResolvedBinObject, ResolveError>,
    pub corrupt: Vec<CorruptBin>,
}

/// Resolve a bin entry the way the game does: walk the root bin and its
/// `linked` bins (breadth-first, within this chunk source) and return as
/// soon as a bin defines the entry.
///
/// Linked bins absent from the source (e.g. references into Global) are
/// skipped with a debug log; bins that fail to load or parse are recorded in
/// [`ResolveOutcome::corrupt`] and skipped. When `expected_class` is given,
/// a found entry with a different class is a definitive
/// [`ResolveError::WrongClass`] (entry path hashes are unique across the
/// merged bin graph, so there is nothing further to search).
pub fn resolve_bin_entry_with(
    source: &mut dyn ChunkSource,
    root_bin_path: &str,
    entry_hash: BinHash,
    expected_class: Option<BinHash>,
) -> ResolveOutcome {
    let mut corrupt = Vec::new();

    if !source.contains(*WadHash::hash_str(root_bin_path)) {
        return ResolveOutcome {
            entry: Err(ResolveError::RootBinMissing {
                bin_path: root_bin_path.to_owned(),
            }),
            corrupt,
        };
    }

    let mut queue = VecDeque::from([root_bin_path.to_owned()]);
    let mut visited: HashSet<u64> = HashSet::new();

    while let Some(bin_path) = queue.pop_front() {
        let bin_name_hash = *WadHash::hash_str(&bin_path);
        if !visited.insert(bin_name_hash) {
            continue;
        }
        if visited.len() > MAX_LINKED_BINS {
            return ResolveOutcome {
                entry: Err(ResolveError::TooManyLinkedBins {
                    root: root_bin_path.to_owned(),
                    limit: MAX_LINKED_BINS,
                }),
                corrupt,
            };
        }

        if !source.contains(bin_name_hash) {
            tracing::debug!("Linked bin '{bin_path}' is not in this WAD, skipping");
            continue;
        }
        let data = match source.load(bin_name_hash) {
            Ok(data) => data,
            Err(reason) => {
                tracing::warn!("Failed to load bin '{bin_path}': {reason}");
                corrupt.push(CorruptBin {
                    bin_path,
                    name_hash: bin_name_hash,
                    reason,
                });
                continue;
            }
        };
        let bin = match Bin::from_reader(&mut Cursor::new(&data[..])) {
            Ok(bin) => bin,
            Err(err) => {
                tracing::warn!("Failed to parse bin '{bin_path}': {err}");
                corrupt.push(CorruptBin {
                    bin_path,
                    name_hash: bin_name_hash,
                    reason: format!("parse: {err}"),
                });
                continue;
            }
        };

        if let Some(object) = bin.get_object(entry_hash) {
            if let Some(expected) = expected_class
                && object.class_hash != expected
            {
                return ResolveOutcome {
                    entry: Err(ResolveError::WrongClass {
                        bin_path,
                        entry: entry_hash,
                        class: object.class_hash,
                        expected,
                    }),
                    corrupt,
                };
            }
            return ResolveOutcome {
                entry: Ok(ResolvedBinObject {
                    bin_path,
                    bin_name_hash,
                    object: object.clone(),
                }),
                corrupt,
            };
        }

        queue.extend(bin.dependencies.iter().cloned());
    }

    ResolveOutcome {
        entry: Err(ResolveError::EntryNotFound {
            root: root_bin_path.to_owned(),
            entry: entry_hash,
        }),
        corrupt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ltk_meta::property::NoMeta;
    use std::collections::HashMap;

    /// A [`ChunkSource`] over plain maps: `bins` are readable chunks,
    /// `broken` are present but fail to load.
    #[derive(Default)]
    struct MapSource {
        bins: HashMap<u64, Vec<u8>>,
        broken: HashMap<u64, String>,
    }

    impl MapSource {
        fn bin(mut self, path: &str, bin: &Bin) -> Self {
            let mut cursor = Cursor::new(Vec::new());
            bin.to_writer(&mut cursor).unwrap();
            self.bins
                .insert(*WadHash::hash_str(path), cursor.into_inner());
            self
        }

        fn garbage(mut self, path: &str, bytes: &[u8]) -> Self {
            self.bins.insert(*WadHash::hash_str(path), bytes.to_vec());
            self
        }

        fn unreadable(mut self, path: &str, reason: &str) -> Self {
            self.broken
                .insert(*WadHash::hash_str(path), reason.to_owned());
            self
        }
    }

    impl ChunkSource for MapSource {
        fn checksum(&mut self, name_hash: u64) -> Option<u64> {
            (self.bins.contains_key(&name_hash) || self.broken.contains_key(&name_hash))
                .then_some(0)
        }
        fn load(&mut self, name_hash: u64) -> Result<Vec<u8>, String> {
            if let Some(reason) = self.broken.get(&name_hash) {
                return Err(reason.clone());
            }
            self.bins
                .get(&name_hash)
                .cloned()
                .ok_or_else(|| "not present".to_string())
        }
    }

    const ROOT: &str = "data/characters/testchamp/skins/skin0.bin";
    const CONCAT: &str = "data/testchamp_skin0_concat.bin";
    const ENTRY: &str = "Characters/Testchamp/Skins/Skin0";

    fn h(name: &str) -> BinHash {
        BinHash::hash_str(name)
    }

    fn entry_object(class: &str) -> BinObject {
        BinObject::<NoMeta>::builder(h(ENTRY), h(class)).build()
    }

    #[test]
    fn resolves_entry_in_root() {
        let root = Bin::builder()
            .object(entry_object("SkinCharacterDataProperties"))
            .build();
        let mut source = MapSource::default().bin(ROOT, &root);

        let outcome = resolve_bin_entry_with(&mut source, ROOT, h(ENTRY), None);
        let resolved = outcome.entry.unwrap();
        assert_eq!(resolved.bin_path, ROOT);
        assert!(outcome.corrupt.is_empty());
    }

    #[test]
    fn missing_root_is_root_bin_missing() {
        let mut source = MapSource::default();
        let outcome = resolve_bin_entry_with(&mut source, ROOT, h(ENTRY), None);
        assert!(matches!(
            outcome.entry,
            Err(ResolveError::RootBinMissing { .. })
        ));
    }

    #[test]
    fn follows_linked_bins_and_skips_absent_ones() {
        let root = Bin::builder()
            .dependency("DATA/Characters/Testchamp/Testchamp.bin") // not in source
            .dependency(CONCAT)
            .build();
        let concat = Bin::builder()
            .object(entry_object("SkinCharacterDataProperties"))
            .build();
        let mut source = MapSource::default().bin(ROOT, &root).bin(CONCAT, &concat);

        let outcome = resolve_bin_entry_with(&mut source, ROOT, h(ENTRY), None);
        assert_eq!(outcome.entry.unwrap().bin_path, CONCAT);
        assert!(outcome.corrupt.is_empty());
    }

    #[test]
    fn cycles_terminate_with_entry_not_found() {
        let root = Bin::builder().dependency(CONCAT).build();
        let concat = Bin::builder().dependency(ROOT).build();
        let mut source = MapSource::default().bin(ROOT, &root).bin(CONCAT, &concat);

        let outcome = resolve_bin_entry_with(&mut source, ROOT, h(ENTRY), None);
        assert!(matches!(
            outcome.entry,
            Err(ResolveError::EntryNotFound { .. })
        ));
    }

    #[test]
    fn corrupt_bins_are_recorded_and_walk_continues() {
        // Root parses but links a garbage bin and an unreadable bin; the
        // entry lives in a healthy third bin. All corruption is surfaced,
        // and resolution still succeeds.
        let good = "data/good.bin";
        let root = Bin::builder()
            .dependency(CONCAT)
            .dependency("data/unreadable.bin")
            .dependency(good)
            .build();
        let target = Bin::builder()
            .object(entry_object("SkinCharacterDataProperties"))
            .build();
        let mut source = MapSource::default()
            .bin(ROOT, &root)
            .garbage(CONCAT, b"not a property bin")
            .unreadable("data/unreadable.bin", "decompress: boom")
            .bin(good, &target);

        let outcome = resolve_bin_entry_with(&mut source, ROOT, h(ENTRY), None);
        assert_eq!(outcome.entry.unwrap().bin_path, good);
        assert_eq!(outcome.corrupt.len(), 2);
        assert!(outcome.corrupt.iter().any(|c| c.bin_path == CONCAT));
        assert!(
            outcome
                .corrupt
                .iter()
                .any(|c| c.reason == "decompress: boom")
        );
    }

    #[test]
    fn corrupt_root_yields_entry_not_found_plus_corruption() {
        let mut source = MapSource::default().garbage(ROOT, b"garbage");
        let outcome = resolve_bin_entry_with(&mut source, ROOT, h(ENTRY), None);
        assert!(matches!(
            outcome.entry,
            Err(ResolveError::EntryNotFound { .. })
        ));
        assert_eq!(outcome.corrupt.len(), 1);
    }

    #[test]
    fn wrong_class_is_definitive() {
        let root = Bin::builder()
            .object(entry_object("ResourceResolver"))
            .build();
        let mut source = MapSource::default().bin(ROOT, &root);

        let outcome = resolve_bin_entry_with(
            &mut source,
            ROOT,
            h(ENTRY),
            Some(h("SkinCharacterDataProperties")),
        );
        assert!(matches!(
            outcome.entry,
            Err(ResolveError::WrongClass { class, expected, .. })
                if class == h("ResourceResolver")
                    && expected == h("SkinCharacterDataProperties")
        ));
    }
}
