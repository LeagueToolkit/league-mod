//! Base-skin (`skin0`) domain knowledge: entry paths, mesh reference
//! extraction, and champion WAD detection.

use std::fmt;

use ltk_hash::{BinHash, Hash as _};
use ltk_meta::{BinObject, PropertyValueEnum};

/// Which of a skin's two checked mesh assets a reference or diagnostic
/// refers to.
///
/// Both are required: every base-game `skin0` sets them (empirically
/// 172/172), so a skin without one is malformed. The entry's `Texture`
/// property is deliberately not checked — it is optional (material-override
/// skins like Evelynn, Mel, and Yuumi omit it) and a dangling texture
/// reference is a known authoring idiom for suppressing the vanilla base
/// texture, so it carries no correctness signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshSlot {
    Skeleton,
    SimpleSkin,
}

impl fmt::Display for MeshSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MeshSlot::Skeleton => "skeleton",
            MeshSlot::SimpleSkin => "simple-skin",
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
}

impl SkinMeshRefs {
    /// The referenced path for one slot, `None` when the entry does not set
    /// it.
    pub fn slot_path(&self, slot: MeshSlot) -> Option<&str> {
        match slot {
            MeshSlot::Skeleton => self.skeleton.as_deref(),
            MeshSlot::SimpleSkin => self.simple_skin.as_deref(),
        }
    }

    /// The set slots and their referenced paths, in a fixed order.
    pub fn slots(&self) -> impl Iterator<Item = (MeshSlot, &str)> {
        [
            (MeshSlot::Skeleton, self.skeleton.as_deref()),
            (MeshSlot::SimpleSkin, self.simple_skin.as_deref()),
        ]
        .into_iter()
        .filter_map(|(slot, path)| path.map(|p| (slot, p)))
    }

    /// Slots the entry does not set — every checked slot is required.
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

/// Extract the checked mesh references from one
/// `SkinCharacterDataProperties` object (its `SkinMeshProperties` embed).
/// The `Texture` property is deliberately not read (see [`MeshSlot`]).
/// Unset properties stay `None` — judging that is
/// [`SkinMeshRefs::missing_required_slots`]' job.
pub fn skin_mesh_refs(object: &BinObject) -> SkinMeshRefs {
    let mesh_prop = BinHash::hash_str("SkinMeshProperties");

    let mut refs = SkinMeshRefs {
        entry_hash: *object.path_hash,
        skeleton: None,
        simple_skin: None,
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
