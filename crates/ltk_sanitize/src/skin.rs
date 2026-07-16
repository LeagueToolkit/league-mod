//! Base-skin (`skin0`) domain knowledge: entry paths, mesh reference
//! extraction, and champion WAD detection.

use std::fmt;

use ltk_hash::{BinHash, Hash as _};
use ltk_meta::{BinObject, PropertyValueEnum};

/// Which of a skin's three mesh assets a reference or diagnostic refers to.
///
/// `Skeleton` and `SimpleSkin` are required — every base-game `skin0` sets
/// them (empirically 172/172), so a skin without one is malformed — while
/// `Texture` is optional: material-override skins (Evelynn, Mel, Yuumi)
/// omit it. Note that optionality is about the *property being unset*; a
/// property that is set must still resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshSlot {
    Skeleton,
    SimpleSkin,
    Texture,
}

impl MeshSlot {
    /// Whether every base-game `skin0` sets this slot.
    pub fn is_required(self) -> bool {
        !matches!(self, MeshSlot::Texture)
    }
}

impl fmt::Display for MeshSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MeshSlot::Skeleton => "skeleton",
            MeshSlot::SimpleSkin => "simple-skin",
            MeshSlot::Texture => "texture",
        })
    }
}

/// Mesh asset references extracted from one `SkinCharacterDataProperties`
/// entry (its `SkinMeshProperties` embed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinMeshRefs {
    /// fnv1a path hash of the entry (e.g. `Characters/Nautilus/Skins/Skin0`),
    /// as a plain `u32` — report structs never expose `ltk_hash` types.
    pub entry_hash: u32,
    /// `SkinMeshProperties.Skeleton` (`.skl`).
    pub skeleton: Option<String>,
    /// `SkinMeshProperties.SimpleSkin` (`.skn`).
    pub simple_skin: Option<String>,
    /// `SkinMeshProperties.Texture` (`.tex`/`.dds`).
    pub texture: Option<String>,
}

impl SkinMeshRefs {
    /// The set slots and their referenced paths, in a fixed order.
    pub fn slots(&self) -> impl Iterator<Item = (MeshSlot, &str)> {
        [
            (MeshSlot::Skeleton, self.skeleton.as_deref()),
            (MeshSlot::SimpleSkin, self.simple_skin.as_deref()),
            (MeshSlot::Texture, self.texture.as_deref()),
        ]
        .into_iter()
        .filter_map(|(slot, path)| path.map(|p| (slot, p)))
    }

    /// Required slots (see [`MeshSlot::is_required`]) the entry does not set.
    pub fn missing_required_slots(&self) -> Vec<MeshSlot> {
        let mut missing = Vec::new();
        if self.skeleton.is_none() {
            missing.push(MeshSlot::Skeleton);
        }
        if self.simple_skin.is_none() {
            missing.push(MeshSlot::SimpleSkin);
        }
        missing
    }
}

/// Extract the mesh references from one `SkinCharacterDataProperties`
/// object (its `SkinMeshProperties` embed). Unset properties stay `None` —
/// judging that is [`SkinMeshRefs::missing_required_slots`]' job.
pub fn skin_mesh_refs(object: &BinObject) -> SkinMeshRefs {
    let mesh_prop = BinHash::hash_str("SkinMeshProperties");

    let mut refs = SkinMeshRefs {
        entry_hash: *object.path_hash,
        skeleton: None,
        simple_skin: None,
        texture: None,
    };
    let mesh_properties = match object.properties.get(&mesh_prop) {
        Some(PropertyValueEnum::Embedded(embedded)) => Some(&embedded.0.properties),
        Some(PropertyValueEnum::Struct(inner)) => Some(&inner.properties),
        _ => None,
    };
    if let Some(props) = mesh_properties {
        let string_of = |key: &str| match props.get(&BinHash::hash_str(key)) {
            Some(PropertyValueEnum::String(s)) => Some(s.value.clone()),
            _ => None,
        };
        refs.skeleton = string_of("Skeleton");
        refs.simple_skin = string_of("SimpleSkin");
        refs.texture = string_of("Texture");
    }
    refs
}

/// The root bin the game resolves a champion's base skin from.
pub fn skin0_bin_path(champion: &str) -> String {
    format!("data/characters/{champion}/skins/skin0.bin")
}

/// xxh64 chunk hash of [`skin0_bin_path`], for looking the root bin up in a
/// WAD TOC or override set.
pub fn skin0_bin_name_hash(champion: &str) -> u64 {
    use ltk_hash::{Hash as _, WadHash};
    *WadHash::hash_str(skin0_bin_path(champion))
}

/// The bin entry path hash of a champion's base skin.
pub fn skin0_entry_hash(champion: &str) -> BinHash {
    BinHash::hash_str(format!("characters/{champion}/skins/skin0"))
}

/// The class every skin entry must have.
pub fn skin_character_data_class() -> BinHash {
    BinHash::hash_str("SkinCharacterDataProperties")
}

/// The lowercase champion directory name for a champion WAD, or `None` for
/// anything that is not a champion WAD the base-skin check covers.
///
/// `wad_path` is the game-relative WAD path (any case, any separators),
/// e.g. `DATA/FINAL/Champions/Aatrox.wad.client` → `aatrox`. Localized
/// champion WADs (`Aatrox.en_US.wad.client`) and WADs outside
/// `data/final/champions/` are rejected — this is the same scope the
/// in-game verifier scans.
pub fn champion_from_wad_path(wad_path: &str) -> Option<String> {
    let normalized = wad_path.replace('\\', "/").to_ascii_lowercase();
    let (dir, file) = normalized.rsplit_once('/')?;
    if dir != "data/final/champions" && !dir.ends_with("/data/final/champions") {
        return None;
    }
    let champion = file.strip_suffix(".wad.client")?;
    if champion.is_empty() || champion.contains(['_', '.']) {
        return None;
    }
    Some(champion.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use ltk_meta::property::{NoMeta, values};
    use ltk_meta::{Bin, BinObject};
    use std::io::Cursor;

    const SKL: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body.skl";
    const SKN: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body.skn";
    const TEX: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body_TX_CM.tex";

    fn h(name: &str) -> BinHash {
        BinHash::hash_str(name)
    }

    fn skin_entry() -> BinObject {
        let mesh = values::Embedded(values::Struct {
            class_hash: h("SkinMeshDataProperties"),
            properties: IndexMap::from([
                (h("Skeleton"), values::String::from(SKL).into()),
                (h("SimpleSkin"), values::String::from(SKN).into()),
                (h("Texture"), values::String::from(TEX).into()),
            ]),
            meta: NoMeta,
        });

        BinObject::<NoMeta>::builder(
            h("Characters/Testchamp/Skins/Skin0"),
            h("SkinCharacterDataProperties"),
        )
        .property(h("SkinClassification"), values::U32::new(1))
        .property(h("SkinMeshProperties"), mesh)
        .build()
    }

    #[test]
    fn extracts_mesh_refs_from_skin_entry() {
        // Round-trip through the real wire format before extracting.
        let bin = Bin::builder().object(skin_entry()).build();
        let mut cursor = Cursor::new(Vec::new());
        bin.to_writer(&mut cursor).unwrap();
        let mut cursor = Cursor::new(cursor.into_inner());
        let bin = Bin::from_reader(&mut cursor).unwrap();

        let object = bin
            .get_object(h("Characters/Testchamp/Skins/Skin0"))
            .unwrap();
        let refs = skin_mesh_refs(object);
        assert_eq!(refs.entry_hash, *h("Characters/Testchamp/Skins/Skin0"));
        assert_eq!(refs.skeleton.as_deref(), Some(SKL));
        assert_eq!(refs.simple_skin.as_deref(), Some(SKN));
        assert_eq!(refs.texture.as_deref(), Some(TEX));
        assert_eq!(refs.slots().count(), 3);
        assert!(refs.missing_required_slots().is_empty());
    }

    #[test]
    fn unset_properties_are_missing_required_slots() {
        let object = BinObject::<NoMeta>::builder(
            h("Characters/Testchamp/Skins/Skin0"),
            h("SkinCharacterDataProperties"),
        )
        .property(h("SkinClassification"), values::U32::new(1))
        .build();
        let refs = skin_mesh_refs(&object);
        assert_eq!(refs.slots().count(), 0);
        assert_eq!(
            refs.missing_required_slots(),
            vec![MeshSlot::Skeleton, MeshSlot::SimpleSkin]
        );
    }

    #[test]
    fn missing_texture_is_not_a_missing_required_slot() {
        let mesh = values::Embedded(values::Struct {
            class_hash: h("SkinMeshDataProperties"),
            properties: IndexMap::from([
                (h("Skeleton"), values::String::from(SKL).into()),
                (h("SimpleSkin"), values::String::from(SKN).into()),
            ]),
            meta: NoMeta,
        });
        let object = BinObject::<NoMeta>::builder(
            h("Characters/Testchamp/Skins/Skin0"),
            h("SkinCharacterDataProperties"),
        )
        .property(h("SkinMeshProperties"), mesh)
        .build();

        let refs = skin_mesh_refs(&object);
        assert_eq!(refs.slots().count(), 2);
        assert!(refs.missing_required_slots().is_empty());
    }

    #[test]
    fn champion_wad_detection() {
        assert_eq!(
            champion_from_wad_path("DATA/FINAL/Champions/Aatrox.wad.client").as_deref(),
            Some("aatrox")
        );
        assert_eq!(
            champion_from_wad_path("data\\final\\champions\\Nautilus.wad.client").as_deref(),
            Some("nautilus")
        );
        // Localized champion WADs are out of scope.
        assert_eq!(
            champion_from_wad_path("DATA/FINAL/Champions/Aatrox.en_US.wad.client"),
            None
        );
        // Non-champion WADs are out of scope.
        assert_eq!(
            champion_from_wad_path("DATA/FINAL/Maps/Shipping/Map11.wad.client"),
            None
        );
        // A bare filename carries no placement — not a champion WAD.
        assert_eq!(champion_from_wad_path("Aatrox.wad.client"), None);
        assert_eq!(champion_from_wad_path(""), None);
    }
}
