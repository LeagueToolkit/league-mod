//! End-to-end tests for the base-skin integrity check: a mod that overrides a
//! champion's `skin0.bin` must leave the base skin's mesh references
//! resolvable in the overlay WAD the game loads (the in-game verifier's
//! closed-world assertion). Violations are attributed to the offending mod;
//! problems with the *original* game WAD are baseline anomalies and never
//! become mod diagnostics.

use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;
use ltk_meta::property::{NoMeta, values};
use ltk_meta::{Bin, BinObject};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuilder, SkinIntegrityOffender};
use ltk_sanitize::BinHash;
use ltk_wad::{WadBuilder, WadChunkBuilder, WadChunkCompression};
use std::fs;
use std::io::{Cursor, Write};

const CHAMP_WAD: &str = "Testchamp.wad.client";
const OTHER_WAD: &str = "Ahri.wad.client";
const SKIN0_BIN: &str = "data/characters/testchamp/skins/skin0.bin";
const SKL: &str = "assets/characters/testchamp/skins/base/body.skl";
const SKN: &str = "assets/characters/testchamp/skins/base/body.skn";
const TEX: &str = "assets/characters/testchamp/skins/base/body_tx_cm.tex";

fn h(name: &str) -> BinHash {
    use ltk_sanitize::Hash as _;
    BinHash::hash_str(name)
}

/// A skin0 bin whose entry references the given texture path (skeleton and
/// simple-skin always point at the stock assets). `scale` varies the bytes so
/// a "modded" bin differs from the original.
fn skin0_bin(texture: &str, scale: f32) -> Vec<u8> {
    let mesh = values::Embedded(values::Struct {
        class_hash: h("SkinMeshDataProperties"),
        properties: IndexMap::from([
            (h("Skeleton"), values::String::from(SKL).into()),
            (h("SimpleSkin"), values::String::from(SKN).into()),
            (h("Texture"), values::String::from(texture).into()),
            (h("SkinScale"), values::F32::new(scale).into()),
        ]),
        meta: NoMeta,
    });
    let entry = BinObject::<NoMeta>::builder(
        h("Characters/Testchamp/Skins/Skin0"),
        h("SkinCharacterDataProperties"),
    )
    .property(h("SkinMeshProperties"), mesh)
    .build();

    let bin = Bin::builder().object(entry).build();
    let mut cursor = Cursor::new(Vec::new());
    bin.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

fn write_game_wad(game_dir: &Utf8Path, wad_name: &str, chunks: &[(&str, Vec<u8>)]) {
    let champions_dir = game_dir.join("DATA").join("FINAL").join("Champions");
    fs::create_dir_all(champions_dir.as_std_path()).unwrap();

    let mut builder = WadBuilder::default();
    for (chunk_path, _) in chunks {
        builder = builder.with_chunk(
            WadChunkBuilder::default()
                .with_path(chunk_path)
                .with_force_compression(WadChunkCompression::None),
        );
    }
    let chunks: Vec<(u64, Vec<u8>)> = chunks
        .iter()
        .map(|(path, bytes)| {
            (
                ltk_overlay::utils::resolve_chunk_hash(Utf8Path::new(path), b"").unwrap(),
                bytes.clone(),
            )
        })
        .collect();
    let mut cursor = Cursor::new(Vec::new());
    builder
        .build_to_writer(&mut cursor, move |hash, writer| {
            let bytes = &chunks.iter().find(|(h, _)| *h == hash).unwrap().1;
            writer.write_all(bytes)?;
            Ok(())
        })
        .unwrap();

    fs::write(
        champions_dir.join(wad_name).as_std_path(),
        cursor.into_inner(),
    )
    .unwrap();
}

/// A game with a valid Testchamp baseline (skin0 + all three mesh assets)
/// and a second champion WAD for wrong-WAD scenarios.
fn write_game(game_dir: &Utf8Path) {
    write_game_wad(
        game_dir,
        CHAMP_WAD,
        &[
            (SKIN0_BIN, skin0_bin(TEX, 1.0)),
            (SKL, b"skeleton-data".to_vec()),
            (SKN, b"mesh-data".to_vec()),
            (TEX, b"texture-data".to_vec()),
        ],
    );
    write_game_wad(
        game_dir,
        OTHER_WAD,
        &[("assets/characters/ahri/vfx.tex", b"AHRI".to_vec())],
    );
}

/// Write a mod whose files are `(wad directory name, chunk path, bytes)`.
fn write_mod_dir(root: &Utf8Path, name: &str, files: &[(&str, &str, Vec<u8>)]) -> Utf8PathBuf {
    let mod_dir = root.join(name);
    for (wad_name, chunk_path, bytes) in files {
        let file = mod_dir
            .join("content")
            .join("base")
            .join(wad_name)
            .join(chunk_path);
        fs::create_dir_all(file.parent().unwrap().as_std_path()).unwrap();
        fs::write(file.as_std_path(), bytes).unwrap();
    }

    let project = ModProject {
        name: name.to_string(),
        display_name: name.to_string(),
        version: "1.0.0".to_string(),
        description: String::new(),
        authors: vec![],
        license: None,
        tags: vec![],
        champions: vec![],
        maps: vec![],
        transformers: vec![],
        layers: vec![ModProjectLayer {
            name: "base".to_string(),
            display_name: None,
            priority: 0,
            description: None,
            string_overrides: Default::default(),
        }],
        thumbnail: None,
    };
    fs::write(
        mod_dir.join("mod.config.json").as_std_path(),
        serde_json::to_string_pretty(&project).unwrap(),
    )
    .unwrap();
    mod_dir
}

fn build_and_take_offenders(root: &Utf8Path, mod_dir: &Utf8PathBuf) -> Vec<SkinIntegrityOffender> {
    let mut builder = OverlayBuilder::new(
        root.join("Game"),
        root.join("profile").join("overlay"),
        root.join("profile"),
    );
    builder.set_enabled_mods(vec![EnabledMod {
        id: "test-mod".to_string(),
        content: Box::new(FsModContent::new(mod_dir.clone())),
        enabled_layers: None,
    }]);
    builder.build().unwrap();
    builder.take_skin_integrity_offenders()
}

/// A modified skin0 whose references all resolve is not an offender.
#[test]
fn clean_skin_swap_is_not_flagged() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    write_game(&root.join("Game"));

    let mod_dir = write_mod_dir(
        &root,
        "clean-mod",
        &[
            (CHAMP_WAD, SKIN0_BIN, skin0_bin(TEX, 2.0)),
            (CHAMP_WAD, TEX, b"MODDED-texture".to_vec()),
        ],
    );

    assert_eq!(build_and_take_offenders(&root, &mod_dir), vec![]);
}

/// A mod that does not touch skin0.bin is never checked (or flagged), no
/// matter what else it ships.
#[test]
fn mod_without_skin0_override_is_not_flagged() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    write_game(&root.join("Game"));

    let mod_dir = write_mod_dir(
        &root,
        "texture-mod",
        &[(CHAMP_WAD, TEX, b"MODDED-texture".to_vec())],
    );

    assert_eq!(build_and_take_offenders(&root, &mod_dir), vec![]);
}

/// A skin0 referencing a texture that exists nowhere — an outdated or broken
/// mod — is flagged with a "missing everywhere" violation, and the offender
/// survives an exact-match skip via the persisted overlay state.
#[test]
fn dangling_reference_is_flagged_and_persisted() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    write_game(&root.join("Game"));

    let stale = "assets/characters/testchamp/skins/base/removed_in_patch.tex";
    let mod_dir = write_mod_dir(
        &root,
        "broken-mod",
        &[(CHAMP_WAD, SKIN0_BIN, skin0_bin(stale, 2.0))],
    );

    let offenders = build_and_take_offenders(&root, &mod_dir);
    assert_eq!(offenders.len(), 1);
    let offender = &offenders[0];
    assert_eq!(offender.mod_id, "test-mod");
    assert_eq!(offender.wad, CHAMP_WAD);
    assert_eq!(offender.champion, "testchamp");
    assert_eq!(offender.violations.len(), 1);
    assert!(
        offender.violations[0].contains("broken or outdated"),
        "unexpected violation text: {}",
        offender.violations[0]
    );

    // Second build is an exact-match skip; offenders come back from state.
    assert_eq!(build_and_take_offenders(&root, &mod_dir), offenders);
}

/// A skin0 referencing a custom asset the mod shipped into a *different* WAD
/// is flagged as misplaced, naming the WAD that has it.
#[test]
fn misplaced_reference_is_flagged_with_the_wrong_wad() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    write_game(&root.join("Game"));

    let custom = "assets/characters/testchamp/skins/base/custom.tex";
    let mod_dir = write_mod_dir(
        &root,
        "misplaced-mod",
        &[
            (CHAMP_WAD, SKIN0_BIN, skin0_bin(custom, 2.0)),
            // New chunk shipped under the wrong WAD directory: it routes to
            // Ahri.wad.client, not the champion WAD referencing it.
            (OTHER_WAD, custom, b"custom-texture".to_vec()),
        ],
    );

    let offenders = build_and_take_offenders(&root, &mod_dir);
    assert_eq!(offenders.len(), 1);
    assert_eq!(offenders[0].violations.len(), 1);
    let violation = &offenders[0].violations[0];
    assert!(
        violation.contains("wrong WAD") && violation.contains(OTHER_WAD),
        "unexpected violation text: {violation}"
    );
}

/// A corrupt *original* skin0.bin is a baseline anomaly: logged, but never a
/// mod diagnostic.
#[test]
fn corrupt_original_is_not_blamed_on_the_mod() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game_dir = root.join("Game");
    write_game_wad(
        &game_dir,
        CHAMP_WAD,
        &[
            (SKIN0_BIN, b"not a property bin".to_vec()),
            (SKL, b"skeleton-data".to_vec()),
            (SKN, b"mesh-data".to_vec()),
            (TEX, b"texture-data".to_vec()),
        ],
    );

    let mod_dir = write_mod_dir(
        &root,
        "any-mod",
        &[(CHAMP_WAD, SKIN0_BIN, skin0_bin(TEX, 2.0))],
    );

    assert_eq!(build_and_take_offenders(&root, &mod_dir), vec![]);
}
