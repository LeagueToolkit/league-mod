//! Property-bin "linked file" dependency validation, run as part of the overlay build.
//!
//! League property-bins (`PROP`/`PTCH`) declare a list of *linked* bin paths they
//! depend on. At load time the game resolves each linked path against the WAD it is
//! mounted from; a missing dependency yields `STATUS_NOT_FOUND` (`c0000225`). The
//! cslol patcher used to treat this as fatal but now only logs it and keeps patching,
//! so a broken mod can silently destabilize the game.
//!
//! We replicate the check against the overlay we are about to write: a linked bin is
//! considered missing when its chunk-path hash is absent from the overlay WAD that
//! contains the bin declaring it. Because the overlay WAD is the original game WAD
//! with the mod's overrides layered on top, this set is exactly
//! `original_chunks(wad) ∪ overrides_routed_to(wad)`. New/custom bins the mod ships
//! resolve (they exist in the overlay WAD); references to bins that were removed from
//! the game in a past patch do not.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::builder::OverrideMeta;
use crate::game_index::GameIndex;
use crate::utils::resolve_chunk_hash;

/// Upper bound on a bin's declared linked-file count, guarding `Vec` pre-allocation
/// against corrupt/garbage input. Real bins declare at most a handful.
const MAX_LINKED_FILES: u32 = 100_000;

/// A mod that ships one or more property-bins whose linked dependencies cannot be
/// resolved against the overlay WAD they land in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedBinOffender {
    /// Mod identifier (matches [`crate::EnabledMod::id`]).
    pub mod_id: String,
    /// WAD filenames (e.g. `Ahri.wad.client`) containing the unresolved bins,
    /// deduped and sorted.
    pub wads: Vec<String>,
    /// The missing linked bin paths, deduped and sorted.
    pub missing_links: Vec<String>,
}

/// Scan every enabled mod's property-bin overrides for linked dependencies that
/// cannot be resolved against the overlay WAD they are routed to.
///
/// The present-set for an overlay WAD `W` is the union of:
/// - the original game chunks of `W` (from `game_index`), which are copied into the
///   overlay verbatim, and
/// - every override hash routed to `W` (from `wad_hash_sets`), across all mods.
///
/// `wad_hash_sets` must already have blocked WADs removed, so blocked WADs are
/// neither validated nor counted as present.
pub(crate) fn collect_linked_bin_offenders(
    all_meta: &HashMap<u64, OverrideMeta>,
    wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<u64>>,
    game_index: &GameIndex,
) -> Vec<LinkedBinOffender> {
    // mod_id -> (offending wad filenames, missing linked paths)
    let mut by_mod: HashMap<&str, (BTreeSet<String>, BTreeSet<String>)> = HashMap::new();

    for (wad_path, override_hashes) in wad_hash_sets {
        for &path_hash in override_hashes {
            let Some(meta) = all_meta.get(&path_hash) else {
                continue;
            };
            if meta.linked_bins.is_empty() {
                continue;
            }

            for link in &meta.linked_bins {
                let Ok(link_hash) = resolve_chunk_hash(Utf8Path::new(link), b"") else {
                    continue;
                };

                // Resolved by another override layered into this same overlay WAD.
                if override_hashes.contains(&link_hash) {
                    continue;
                }
                // Resolved by an original chunk of this WAD (copied into the overlay).
                let in_original = game_index
                    .find_wads_with_hash(link_hash)
                    .is_some_and(|wads| wads.iter().any(|w| w == wad_path));
                if in_original {
                    continue;
                }

                let entry = by_mod.entry(meta.source.mod_id()).or_default();
                if let Some(name) = wad_path.file_name() {
                    entry.0.insert(name.to_string());
                }
                entry.1.insert(link.clone());
            }
        }
    }

    let mut offenders: Vec<LinkedBinOffender> = by_mod
        .into_iter()
        .map(|(mod_id, (wads, links))| LinkedBinOffender {
            mod_id: mod_id.to_string(),
            wads: wads.into_iter().collect(),
            missing_links: links.into_iter().collect(),
        })
        .collect();
    offenders.sort_by(|a, b| a.mod_id.cmp(&b.mod_id));
    offenders
}

/// Parse the "linked files" list from a League property-bin.
///
/// Layout (little-endian):
/// - optional `PTCH` magic (4) + patch header `(u32, u32)`
/// - `PROP` magic (4) + `version: u32`
/// - if `version >= 2`: `count: u32`, then `count` × (`len: u16` + `len` UTF-8 bytes)
///
/// Returns `Some(links)` for a well-formed bin (empty when it declares none) and
/// `None` when the bytes are not a property-bin or are truncated.
pub(crate) fn parse_linked_bins(bytes: &[u8]) -> Option<Vec<String>> {
    use byteorder::{ReadBytesExt, LE};
    use std::io::Read;

    let mut cursor = std::io::Cursor::new(bytes);
    let mut magic = [0u8; 4];
    cursor.read_exact(&mut magic).ok()?;

    if &magic == b"PTCH" {
        // Patch header: two u32s precede the embedded PROP section.
        cursor.read_u32::<LE>().ok()?;
        cursor.read_u32::<LE>().ok()?;
        cursor.read_exact(&mut magic).ok()?;
    }

    if &magic != b"PROP" {
        return None;
    }

    let version = cursor.read_u32::<LE>().ok()?;
    if version < 2 {
        return Some(Vec::new());
    }

    let count = cursor.read_u32::<LE>().ok()?;
    if count > MAX_LINKED_FILES {
        return None;
    }

    let mut links = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let len = cursor.read_u16::<LE>().ok()? as usize;
        let mut buf = vec![0u8; len];
        cursor.read_exact(&mut buf).ok()?;
        links.push(String::from_utf8_lossy(&buf).into_owned());
    }
    Some(links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::OverrideSource;
    use byteorder::{WriteBytesExt, LE};
    use std::io::Write;

    /// Build a minimal PROP bin body with the given version and linked paths.
    fn prop_bin(version: u32, linked: &[&str]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"PROP");
        buf.write_u32::<LE>(version).unwrap();
        if version >= 2 {
            buf.write_u32::<LE>(linked.len() as u32).unwrap();
            for path in linked {
                buf.write_u16::<LE>(path.len() as u16).unwrap();
                buf.write_all(path.as_bytes()).unwrap();
            }
        }
        // Trailing object-type count (unused by the parser) to mimic a real file.
        buf.write_u32::<LE>(0).unwrap();
        buf
    }

    /// Wrap a PROP body in a PTCH patch header.
    fn ptch_bin(version: u32, linked: &[&str]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"PTCH");
        buf.write_u32::<LE>(1).unwrap();
        buf.write_u32::<LE>(0).unwrap();
        buf.extend_from_slice(&prop_bin(version, linked));
        buf
    }

    #[test]
    fn parses_v1_bin_as_no_links() {
        assert_eq!(parse_linked_bins(&prop_bin(1, &[])), Some(Vec::new()));
    }

    #[test]
    fn parses_v2_linked_files() {
        let bin = prop_bin(
            3,
            &[
                "data/characters/ahri/ahri.bin",
                "data/characters/ahri/skins/skin0.bin",
            ],
        );
        assert_eq!(
            parse_linked_bins(&bin),
            Some(vec![
                "data/characters/ahri/ahri.bin".to_string(),
                "data/characters/ahri/skins/skin0.bin".to_string(),
            ])
        );
    }

    #[test]
    fn parses_ptch_wrapped_prop() {
        let bin = ptch_bin(3, &["data/characters/ahri/ahri.bin"]);
        assert_eq!(
            parse_linked_bins(&bin),
            Some(vec!["data/characters/ahri/ahri.bin".to_string()])
        );
    }

    #[test]
    fn rejects_non_bin_bytes() {
        assert_eq!(parse_linked_bins(b"OEGM\x01\x02\x03\x04"), None);
        assert_eq!(parse_linked_bins(&[]), None);
    }

    #[test]
    fn rejects_truncated_link_section() {
        // PROP v2 claiming one link but providing no string bytes.
        let mut bin = Vec::new();
        bin.extend_from_slice(b"PROP");
        bin.write_u32::<LE>(2).unwrap();
        bin.write_u32::<LE>(1).unwrap();
        bin.write_u16::<LE>(10).unwrap(); // declares 10 bytes that aren't there
        assert_eq!(parse_linked_bins(&bin), None);
    }

    #[test]
    fn rejects_absurd_link_count() {
        let mut bin = Vec::new();
        bin.extend_from_slice(b"PROP");
        bin.write_u32::<LE>(2).unwrap();
        bin.write_u32::<LE>(u32::MAX).unwrap();
        assert_eq!(parse_linked_bins(&bin), None);
    }

    fn hash(path: &str) -> u64 {
        resolve_chunk_hash(Utf8Path::new(path), b"").unwrap()
    }

    fn layer_wad_meta(mod_id: &str, linked: &[&str]) -> OverrideMeta {
        OverrideMeta {
            content_hash: 0,
            uncompressed_size: 0,
            source: OverrideSource::LayerWad {
                mod_id: mod_id.to_string(),
                layer: "base".to_string(),
                wad_name: "Test.wad.client".to_string(),
                rel_path: Utf8PathBuf::from("data/test.bin"),
            },
            fallback_wad: None,
            linked_bins: linked.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// A bin override whose linked dependency is a brand-new bin shipped by the same
    /// mod into the same overlay WAD resolves (no offense).
    #[test]
    fn new_bin_shipped_in_same_wad_resolves() {
        let wad = Utf8PathBuf::from("DATA/FINAL/Champions/Test.wad.client");
        let bin_hash = hash("data/characters/test/skins/skin50.bin");
        let dep_hash = hash("data/characters/test/skins/skin50/companion.bin");

        let mut all_meta = HashMap::new();
        all_meta.insert(
            bin_hash,
            layer_wad_meta(
                "mod-a",
                &["data/characters/test/skins/skin50/companion.bin"],
            ),
        );
        // The companion bin is also shipped by the mod (another override in the WAD).
        all_meta.insert(dep_hash, layer_wad_meta("mod-a", &[]));

        let mut wad_hash_sets = BTreeMap::new();
        wad_hash_sets.insert(wad, HashSet::from([bin_hash, dep_hash]));

        let game_index = GameIndex::new();
        let offenders = collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index);
        assert!(offenders.is_empty());
    }

    /// A reference to a bin that exists in neither the overlay WAD's originals nor any
    /// override is flagged (e.g. a long-gone bin removed in a past game patch).
    #[test]
    fn missing_dependency_is_flagged() {
        let wad = Utf8PathBuf::from("DATA/FINAL/Champions/Test.wad.client");
        let bin_hash = hash("data/characters/test/skins/skin0.bin");

        let mut all_meta = HashMap::new();
        all_meta.insert(
            bin_hash,
            layer_wad_meta("mod-a", &["data/characters/test/removed_long_ago.bin"]),
        );

        let mut wad_hash_sets = BTreeMap::new();
        wad_hash_sets.insert(wad, HashSet::from([bin_hash]));

        let game_index = GameIndex::new();
        let offenders = collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index);

        assert_eq!(offenders.len(), 1);
        assert_eq!(offenders[0].mod_id, "mod-a");
        assert_eq!(offenders[0].wads, vec!["Test.wad.client"]);
        assert_eq!(
            offenders[0].missing_links,
            vec!["data/characters/test/removed_long_ago.bin"]
        );
    }

    /// A linked dependency satisfied by an original game chunk of the same WAD
    /// resolves.
    #[test]
    fn dependency_in_original_wad_resolves() {
        let wad = Utf8PathBuf::from("DATA/FINAL/Champions/Test.wad.client");
        let bin_hash = hash("data/characters/test/skins/skin0.bin");
        let dep_hash = hash("data/characters/test/test.bin");

        let mut all_meta = HashMap::new();
        all_meta.insert(
            bin_hash,
            layer_wad_meta("mod-a", &["data/characters/test/test.bin"]),
        );

        let mut wad_hash_sets = BTreeMap::new();
        wad_hash_sets.insert(wad.clone(), HashSet::from([bin_hash]));

        // The dependency is a vanilla chunk of this WAD.
        let mut game_index = GameIndex::new();
        game_index.hash_index.insert(dep_hash, vec![wad]);

        let offenders = collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index);
        assert!(offenders.is_empty());
    }

    /// A dependency that exists only in a *different* WAD is flagged (per-WAD scope).
    #[test]
    fn dependency_in_other_wad_is_flagged() {
        let wad = Utf8PathBuf::from("DATA/FINAL/Champions/Test.wad.client");
        let other_wad = Utf8PathBuf::from("DATA/FINAL/Champions/Other.wad.client");
        let bin_hash = hash("data/characters/test/skins/skin0.bin");
        let dep_hash = hash("data/characters/other/other.bin");

        let mut all_meta = HashMap::new();
        all_meta.insert(
            bin_hash,
            layer_wad_meta("mod-a", &["data/characters/other/other.bin"]),
        );

        let mut wad_hash_sets = BTreeMap::new();
        wad_hash_sets.insert(wad, HashSet::from([bin_hash]));

        let mut game_index = GameIndex::new();
        game_index.hash_index.insert(dep_hash, vec![other_wad]);

        let offenders = collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index);
        assert_eq!(offenders.len(), 1);
        assert_eq!(
            offenders[0].missing_links,
            vec!["data/characters/other/other.bin"]
        );
    }
}
