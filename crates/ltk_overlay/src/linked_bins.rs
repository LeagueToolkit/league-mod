//! Property-bin "linked file" dependency validation, run as part of the overlay build.
//!
//! League property-bins (`PROP`/`PTCH`) declare a list of *linked* bin paths they
//! depend on. One is reported missing when its chunk-path hash is absent from
//! every archive the build produces and the game mounts.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_wad::WadHash;
use serde::{Deserialize, Serialize};

use crate::builder::{OverrideMeta, is_wad_blocked};
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

/// Every chunk path the game will find once the overlay is in place.
///
/// The union, across every archive the game mounts, of that archive's original
/// chunks and the overrides the build routes into it. A declared dependency is
/// missing when it is in none of them, rather than when it is absent from the
/// archive the declaring bin came from.
struct PresentSet<'a> {
    /// Every override hash the build routes into any archive.
    routed: HashSet<WadHash>,
    /// The installed game, for the chunks each archive already had.
    game_index: &'a GameIndex,
    /// Lower-cased file names of the archives the user blocked, which offer
    /// nothing.
    blocked: &'a HashSet<String>,
}

impl<'a> PresentSet<'a> {
    /// Read the set off the build's routing table and the installed game.
    fn of(
        wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<WadHash>>,
        game_index: &'a GameIndex,
        blocked: &'a HashSet<String>,
    ) -> Self {
        Self {
            routed: wad_hash_sets.values().flatten().copied().collect(),
            game_index,
            blocked,
        }
    }

    /// Whether any archive the game mounts will offer `path_hash`.
    fn holds(&self, path_hash: WadHash) -> bool {
        self.routed.contains(&path_hash)
            || self
                .game_index
                .find_wads_with_hash(path_hash)
                .is_some_and(|wads| wads.iter().any(|wad| !is_wad_blocked(wad, self.blocked)))
    }
}

/// Scan every enabled mod's property-bin overrides for linked dependencies no
/// archive the game mounts can answer.
///
/// `wad_hash_sets` must already have blocked WADs removed, and `blocked_wads`
/// is that same blocklist as lower-cased file names.
pub(crate) fn collect_linked_bin_offenders(
    all_meta: &HashMap<WadHash, OverrideMeta>,
    wad_hash_sets: &BTreeMap<Utf8PathBuf, HashSet<WadHash>>,
    game_index: &GameIndex,
    blocked_wads: &HashSet<String>,
) -> Vec<LinkedBinOffender> {
    let present = PresentSet::of(wad_hash_sets, game_index, blocked_wads);

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
                if present.holds(link_hash) {
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
    use byteorder::{LE, ReadBytesExt};
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
    use byteorder::{LE, WriteBytesExt};
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

    fn hash(path: &str) -> WadHash {
        resolve_chunk_hash(Utf8Path::new(path), b"").unwrap()
    }

    fn layer_wad_meta(mod_id: &str, linked: &[&str]) -> OverrideMeta {
        OverrideMeta {
            content_hash: crate::utils::ContentHash(0),
            uncompressed_size: 0,
            source: OverrideSource::LayerWad {
                mod_id: mod_id.to_string(),
                layer: "base".to_string(),
                wad_name: "Test.wad.client".to_string(),
                rel_path: Utf8PathBuf::from("data/test.bin"),
            },
            fallback_wad: None,
            unlocalized_wad: None,
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
        let offenders =
            collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index, &HashSet::new());
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
        let offenders =
            collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index, &HashSet::new());

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

        let offenders =
            collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index, &HashSet::new());
        assert!(offenders.is_empty());
    }

    /// A dependency an archive other than the declaring bin's holds resolves.
    #[test]
    fn dependency_in_another_mounted_archive_resolves() {
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

        let offenders =
            collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index, &HashSet::new());
        assert!(offenders.is_empty(), "{offenders:?}");
    }

    /// A mod shipping its content in a localized archive routes the override
    /// into both, where the dependency is an original chunk of the base alone.
    #[test]
    fn dependency_in_the_base_archive_resolves_on_the_localized_pass() {
        let base = Utf8PathBuf::from("DATA/FINAL/Champions/Sett.wad.client");
        let localized = Utf8PathBuf::from("DATA/FINAL/Champions/Sett.en_us.wad.client");
        let bin_hash = hash("data/characters/sett/skins/skin0.bin");
        let dep_hash = hash("data/characters/sett/sett.bin");

        let mut all_meta = HashMap::new();
        all_meta.insert(
            bin_hash,
            layer_wad_meta("mod-a", &["data/characters/sett/sett.bin"]),
        );

        let mut wad_hash_sets = BTreeMap::new();
        wad_hash_sets.insert(base.clone(), HashSet::from([bin_hash]));
        wad_hash_sets.insert(localized, HashSet::from([bin_hash]));

        let mut game_index = GameIndex::new();
        game_index.hash_index.insert(dep_hash, vec![base]);

        let offenders =
            collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index, &HashSet::new());
        assert!(offenders.is_empty(), "{offenders:?}");
    }

    /// A dependency another mod ships into a different archive resolves too.
    #[test]
    fn dependency_shipped_by_another_mod_elsewhere_resolves() {
        let wad = Utf8PathBuf::from("DATA/FINAL/Champions/Test.wad.client");
        let other_wad = Utf8PathBuf::from("DATA/FINAL/Champions/Other.wad.client");
        let bin_hash = hash("data/characters/test/skins/skin0.bin");
        let dep_hash = hash("data/characters/other/other.bin");

        let mut all_meta = HashMap::new();
        all_meta.insert(
            bin_hash,
            layer_wad_meta("mod-a", &["data/characters/other/other.bin"]),
        );
        all_meta.insert(dep_hash, layer_wad_meta("mod-b", &[]));

        let mut wad_hash_sets = BTreeMap::new();
        wad_hash_sets.insert(wad, HashSet::from([bin_hash]));
        wad_hash_sets.insert(other_wad, HashSet::from([dep_hash]));

        let game_index = GameIndex::new();
        let offenders =
            collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index, &HashSet::new());
        assert!(offenders.is_empty(), "{offenders:?}");
    }

    /// A dependency that resolves only inside an archive the user blocked is
    /// still reported.
    #[test]
    fn dependency_only_in_a_blocked_archive_is_flagged() {
        let wad = Utf8PathBuf::from("DATA/FINAL/Champions/Test.wad.client");
        let blocked_wad = Utf8PathBuf::from("DATA/FINAL/Global.wad.client");
        let bin_hash = hash("data/characters/test/skins/skin0.bin");
        let dep_hash = hash("data/shared/global.bin");

        let mut all_meta = HashMap::new();
        all_meta.insert(
            bin_hash,
            layer_wad_meta("mod-a", &["data/shared/global.bin"]),
        );

        // Blocked archives are already gone from the routing table.
        let mut wad_hash_sets = BTreeMap::new();
        wad_hash_sets.insert(wad, HashSet::from([bin_hash]));

        let mut game_index = GameIndex::new();
        game_index.hash_index.insert(dep_hash, vec![blocked_wad]);

        let blocked = HashSet::from(["global.wad.client".to_string()]);
        let offenders =
            collect_linked_bin_offenders(&all_meta, &wad_hash_sets, &game_index, &blocked);

        assert_eq!(offenders.len(), 1, "{offenders:?}");
        assert_eq!(offenders[0].missing_links, vec!["data/shared/global.bin"]);
    }
}
