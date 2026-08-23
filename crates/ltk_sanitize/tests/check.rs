//! Integration tests for the base-skin check, one per invariant, driving
//! everything through the public API over real in-memory WADs.

use indexmap::IndexMap;
use ltk_sanitize::ltk_meta::property::{NoMeta, values};
use ltk_sanitize::ltk_meta::{Bin, PropertyValueEnum};
use ltk_sanitize::{
    BaselineAnomaly, BinHash, BinObject, ChunkSource, Hash as _, MeshSlot, ModAnomaly,
    ModifiedSkin, RefMissingKind, RefStatus, ResolveError, SkinCheckOutcome, WadChunkSource,
    WadHash, champion_from_wad_path, check_base_skin,
};
use ltk_wad::Wad;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Cursor, Write};

const CHAMP: &str = "testchamp";
const ROOT: &str = "data/characters/testchamp/skins/skin0.bin";
const CONCAT: &str = "data/testchamp_skin0_concat.bin";
const SKL: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body.skl";
const SKN: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body.skn";
const TEX: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body_TX_CM.tex";
const NO_WORLD: Option<&dyn Fn(u64) -> Vec<String>> = None;

fn h(name: &str) -> BinHash {
    BinHash::hash_str(name)
}

fn chunk_hash(path: &str) -> u64 {
    *WadHash::hash_str(path)
}

/// A skin0 bin whose entry references the given slot paths; `scale` varies
/// the bytes so tests can produce a "modded" variant. A Texture property is
/// still written when given — the check must ignore it.
fn skin_bin(
    skeleton: Option<&str>,
    simple_skin: Option<&str>,
    texture: Option<&str>,
    scale: f32,
) -> Vec<u8> {
    let mut properties = IndexMap::new();
    if let Some(path) = skeleton {
        properties.insert(h("Skeleton"), values::String::from(path).into());
    }
    if let Some(path) = simple_skin {
        properties.insert(h("SimpleSkin"), values::String::from(path).into());
    }
    if let Some(path) = texture {
        properties.insert(h("Texture"), values::String::from(path).into());
    }
    properties.insert(h("SkinScale"), values::F32::new(scale).into());
    let mesh = values::Embedded(values::Struct {
        class_hash: h("SkinMeshDataProperties"),
        properties,
        meta: NoMeta,
    });
    let entry = BinObject::<NoMeta>::builder(
        h("Characters/Testchamp/Skins/Skin0"),
        h("SkinCharacterDataProperties"),
    )
    .property(h("SkinMeshProperties"), mesh)
    .build();
    bin_bytes(&Bin::builder().object(entry).build())
}

fn bin_bytes(bin: &Bin) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    bin.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

/// Build an in-memory WAD from `path hash -> uncompressed contents`.
fn build_wad(contents: &BTreeMap<u64, Vec<u8>>) -> Wad<Cursor<Vec<u8>>> {
    use ltk_wad::{WadBuilder, WadChunkBuilder};

    let mut builder = WadBuilder::default();
    for &hash in contents.keys() {
        builder = builder.with_chunk(WadChunkBuilder::default().with_hash(hash));
    }
    let mut out = Cursor::new(Vec::new());
    builder
        .build_to_writer(&mut out, |hash, cursor| {
            cursor.write_all(&contents[&hash]).unwrap();
            Ok(())
        })
        .unwrap();
    out.set_position(0);
    Wad::mount(out).unwrap()
}

/// A valid original: skin0 references all slots and every asset exists.
fn original_contents() -> BTreeMap<u64, Vec<u8>> {
    BTreeMap::from([
        (
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 1.0),
        ),
        (chunk_hash(SKL), b"skeleton-data".to_vec()),
        (chunk_hash(SKN), b"mesh-data".to_vec()),
        (chunk_hash(TEX), b"texture-data".to_vec()),
    ])
}

fn check(
    original: &BTreeMap<u64, Vec<u8>>,
    merged: &BTreeMap<u64, Vec<u8>>,
    world: Option<&dyn Fn(u64) -> Vec<String>>,
) -> SkinCheckOutcome {
    let mut original = build_wad(original);
    let mut merged = build_wad(merged);
    check_base_skin(
        &mut WadChunkSource(&mut original),
        &mut WadChunkSource(&mut merged),
        CHAMP,
        world,
    )
}

fn modified(outcome: SkinCheckOutcome) -> ModifiedSkin {
    match outcome {
        SkinCheckOutcome::Modified(skin) => *skin,
        other => panic!("expected Modified, got {other:?}"),
    }
}

fn mod_anomaly(outcome: SkinCheckOutcome) -> ModAnomaly {
    match outcome {
        SkinCheckOutcome::ModAnomaly(anomaly) => anomaly,
        other => panic!("expected ModAnomaly, got {other:?}"),
    }
}

/// `SkinMeshProperties.SkinScale`, which the fixtures vary per side — proof
/// a parsed entry came from the side it claims.
fn skin_scale(object: &BinObject) -> f32 {
    let Some(PropertyValueEnum::Embedded(mesh)) = object.properties.get(&h("SkinMeshProperties"))
    else {
        panic!("no SkinMeshProperties embed");
    };
    match mesh.0.properties.get(&h("SkinScale")) {
        Some(PropertyValueEnum::F32(scale)) => scale.value,
        other => panic!("unexpected SkinScale: {other:?}"),
    }
}

/// Wraps a source, refusing to load one chunk — models a chunk that is
/// present in the TOC but unreadable (corruption).
struct Unreadable<S> {
    inner: S,
    chunk: u64,
}

impl<S: ChunkSource> ChunkSource for Unreadable<S> {
    fn contains(&mut self, name_hash: u64) -> bool {
        self.inner.contains(name_hash)
    }
    fn load(&mut self, name_hash: u64) -> Result<Vec<u8>, String> {
        if name_hash == self.chunk {
            return Err("simulated unreadable chunk".to_string());
        }
        self.inner.load(name_hash)
    }
}

// ----------------------------------------------------------------- skip

#[test]
fn unmodified_skin0_is_skipped() {
    let original = original_contents();
    let mut merged = original.clone();
    // Even a modified texture chunk: the root bin itself is vanilla.
    merged.insert(chunk_hash(TEX), b"MODDED-texture".to_vec());

    assert_eq!(
        check(&original, &merged, NO_WORLD),
        SkinCheckOutcome::SkippedUnmodified
    );
}

// ------------------------------------------------------------- modified

#[test]
fn modified_skin_carries_objects_and_fingerprints() {
    let original = original_contents();
    let mut merged = original.clone();
    merged.insert(
        chunk_hash(ROOT),
        skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
    );
    merged.insert(chunk_hash(SKN), b"MODDED-mesh".to_vec());

    let skin = modified(check(&original, &merged, NO_WORLD));
    assert_eq!(skin.bin_path, ROOT);
    // Both parsed entries, genuinely from their own side.
    assert_eq!(skin_scale(&skin.object), 2.0);
    assert_eq!(skin_scale(&skin.original_object), 1.0);
    assert_eq!(skin.skeleton.status, RefStatus::Unmodified);
    assert_eq!(
        skin.simple_skin.status,
        RefStatus::Modified {
            sha256: Sha256::digest(b"MODDED-mesh").into(),
        }
    );
}

#[test]
fn repointed_slot_reads_modified() {
    // The skin-unlock shape: a slot repointed at another vanilla asset
    // resolves and holds vanilla bytes, but not the bytes the original
    // SLOT renders — slot-to-slot comparison must read Modified.
    let alt = "ASSETS/Characters/Testchamp/Skins/Skin02/alt.skl";
    let mut original = original_contents();
    original.insert(chunk_hash(alt), b"alt-skeleton-data".to_vec());
    let mut merged = original.clone();
    merged.insert(
        chunk_hash(ROOT),
        skin_bin(Some(alt), Some(SKN), Some(TEX), 2.0),
    );

    let skin = modified(check(&original, &merged, NO_WORLD));
    assert_eq!(
        skin.skeleton.status,
        RefStatus::Modified {
            sha256: Sha256::digest(b"alt-skeleton-data").into(),
        }
    );
}

#[test]
fn repointed_slot_with_identical_content_reads_unmodified() {
    // Content decides, not the path: a repoint landing on byte-identical
    // content renders exactly what vanilla renders.
    let alias = "ASSETS/Characters/Testchamp/Skins/Skin02/copy.skl";
    let mut original = original_contents();
    original.insert(chunk_hash(alias), b"skeleton-data".to_vec());
    let mut merged = original.clone();
    merged.insert(
        chunk_hash(ROOT),
        skin_bin(Some(alias), Some(SKN), Some(TEX), 2.0),
    );

    let skin = modified(check(&original, &merged, NO_WORLD));
    assert_eq!(skin.skeleton.status, RefStatus::Unmodified);
}

#[test]
fn unreadable_original_still_classifies_modified() {
    // An unreadable ORIGINAL chunk is never the mod's problem: equality
    // just cannot be proven, so the merged chunk reads Modified.
    let contents = original_contents();
    let mut merged_contents = contents.clone();
    merged_contents.insert(
        chunk_hash(ROOT),
        skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
    );

    let mut original = build_wad(&contents);
    let mut original = Unreadable {
        inner: WadChunkSource(&mut original),
        chunk: chunk_hash(SKL),
    };
    let mut merged = build_wad(&merged_contents);

    let outcome = check_base_skin(
        &mut original,
        &mut WadChunkSource(&mut merged),
        CHAMP,
        NO_WORLD,
    );
    assert!(matches!(
        modified(outcome).skeleton.status,
        RefStatus::Modified { .. }
    ));
}

#[test]
fn dangling_texture_is_ignored() {
    // The Texture property is never parsed — a dangling texture reference
    // is not a violation.
    let original = original_contents();
    let mut merged = original.clone();
    merged.insert(
        chunk_hash(ROOT),
        skin_bin(Some(SKL), Some(SKN), Some("gone.tex"), 2.0),
    );

    assert!(matches!(
        check(&original, &merged, NO_WORLD),
        SkinCheckOutcome::Modified(_)
    ));
}

#[test]
fn entry_found_via_linked_bin() {
    let original = original_contents();
    let mut merged = original.clone();
    merged.insert(
        chunk_hash(ROOT),
        bin_bytes(&Bin::builder().dependency(CONCAT).build()),
    );
    merged.insert(
        chunk_hash(CONCAT),
        skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
    );

    let skin = modified(check(&original, &merged, NO_WORLD));
    assert_eq!(skin.bin_path, CONCAT);
}

// ---------------------------------------------------------- mod anomaly

#[test]
fn dangling_skeleton_is_missing_everywhere() {
    let original = original_contents();
    let mut merged = original.clone();
    merged.insert(
        chunk_hash(ROOT),
        skin_bin(Some("gone.skl"), Some(SKN), Some(TEX), 2.0),
    );

    let anomaly = mod_anomaly(check(&original, &merged, Some(&|_| Vec::new())));
    assert!(matches!(
        anomaly,
        ModAnomaly::RefMissing {
            slot: MeshSlot::Skeleton,
            kind: RefMissingKind::Everywhere,
            ..
        }
    ));
    assert!(anomaly.to_string().contains("broken or outdated"));
}

#[test]
fn misplaced_ref_names_the_wad_that_has_it() {
    let custom = "ASSETS/Characters/Testchamp/Skins/Base/custom.skn";
    let original = original_contents();
    let mut merged = original.clone();
    merged.insert(
        chunk_hash(ROOT),
        skin_bin(Some(SKL), Some(custom), Some(TEX), 2.0),
    );

    let world = |hash: u64| {
        if hash == chunk_hash(custom) {
            vec!["Testchamp.en_US.wad.client".to_string()]
        } else {
            Vec::new()
        }
    };
    let anomaly = mod_anomaly(check(&original, &merged, Some(&world)));
    assert!(matches!(
        &anomaly,
        ModAnomaly::RefMissing { kind: RefMissingKind::Misplaced { found_in }, .. }
            if found_in == &["Testchamp.en_US.wad.client".to_string()]
    ));
    assert!(anomaly.to_string().contains("wrong WAD"));
}

#[test]
fn unreadable_ref_fails_closed() {
    // Present in the TOC but unreadable: fails closed as RefMissing.
    let contents = original_contents();
    let mut merged_contents = contents.clone();
    merged_contents.insert(
        chunk_hash(ROOT),
        skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
    );

    let mut original = build_wad(&contents);
    let mut merged = build_wad(&merged_contents);
    let mut merged = Unreadable {
        inner: WadChunkSource(&mut merged),
        chunk: chunk_hash(SKN),
    };

    let outcome = check_base_skin(
        &mut WadChunkSource(&mut original),
        &mut merged,
        CHAMP,
        NO_WORLD,
    );
    assert!(matches!(
        mod_anomaly(outcome),
        ModAnomaly::RefMissing {
            slot: MeshSlot::SimpleSkin,
            kind: RefMissingKind::Unreadable { .. },
            ..
        }
    ));
}

#[test]
fn unset_required_slot_is_a_mod_anomaly() {
    let original = original_contents();
    let mut merged = original.clone();
    merged.insert(chunk_hash(ROOT), skin_bin(None, Some(SKN), Some(TEX), 2.0));

    assert_eq!(
        mod_anomaly(check(&original, &merged, NO_WORLD)),
        ModAnomaly::MissingRequiredSlot(MeshSlot::Skeleton)
    );
}

#[test]
fn corrupt_merged_bin_is_a_mod_anomaly() {
    let original = original_contents();
    let mut merged = original.clone();
    merged.insert(chunk_hash(ROOT), b"not a property bin".to_vec());

    assert!(matches!(
        mod_anomaly(check(&original, &merged, NO_WORLD)),
        ModAnomaly::CorruptBin(_)
    ));
}

#[test]
fn linked_bin_cycles_terminate() {
    let original = original_contents();
    let mut merged = original.clone();
    merged.insert(
        chunk_hash(ROOT),
        bin_bytes(&Bin::builder().dependency(CONCAT).build()),
    );
    merged.insert(
        chunk_hash(CONCAT),
        bin_bytes(&Bin::builder().dependency(ROOT).build()),
    );

    assert!(matches!(
        mod_anomaly(check(&original, &merged, NO_WORLD)),
        ModAnomaly::Resolve(ResolveError::EntryNotFound { .. })
    ));
}

// ------------------------------------------------------------- baseline

#[test]
fn corrupt_original_is_a_baseline_anomaly() {
    let mut original = original_contents();
    original.insert(chunk_hash(ROOT), b"garbage original".to_vec());
    let mut merged = original_contents();
    merged.insert(
        chunk_hash(ROOT),
        skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
    );

    assert!(matches!(
        check(&original, &merged, NO_WORLD),
        SkinCheckOutcome::BaselineAnomaly(BaselineAnomaly::OriginalCorruptBin(_))
    ));
}

#[test]
fn original_missing_required_slot_is_a_baseline_anomaly() {
    // The 172/172 assumption: a game patch shipping a skin0 without a
    // skeleton must be reported to us, never blamed on the mod.
    let mut original = original_contents();
    original.insert(chunk_hash(ROOT), skin_bin(None, Some(SKN), Some(TEX), 1.0));
    let mut merged = original.clone();
    merged.insert(
        chunk_hash(ROOT),
        skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
    );

    assert!(matches!(
        check(&original, &merged, NO_WORLD),
        SkinCheckOutcome::BaselineAnomaly(BaselineAnomaly::OriginalMissingRequiredSlot(
            MeshSlot::Skeleton
        ))
    ));
}

// ----------------------------------------------------------- wad scope

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
    // Localized champion WADs, non-champion WADs, and bare filenames are
    // out of scope.
    assert_eq!(
        champion_from_wad_path("DATA/FINAL/Champions/Aatrox.en_US.wad.client"),
        None
    );
    assert_eq!(
        champion_from_wad_path("DATA/FINAL/Maps/Shipping/Map11.wad.client"),
        None
    );
    assert_eq!(champion_from_wad_path("Aatrox.wad.client"), None);
}
