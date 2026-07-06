//! End-to-end tests for cross-WAD imports: a mod that depends on an original
//! chunk from another game WAD and ships it itself, under the original path,
//! inside its own WAD directory.
//!
//! Regression coverage for the bug where such chunks were stripped as "lazy
//! overrides" (byte-identical to the game original) or routed only to the WAD
//! that already contained them — either way never reaching the mod's target
//! WAD, so the asset failed to load in-game.

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_overlay::utils::resolve_chunk_hash;
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuilder};
use ltk_wad::{Wad, WadBuilder, WadChunkBuilder, WadChunkCompression};
use std::fs;
use std::io::{Cursor, Write};

const AATROX_WAD: &str = "Aatrox.wad.client";
const AHRI_WAD: &str = "Ahri.wad.client";
const AATROX_CHUNK: &str = "assets/characters/aatrox/skin0.tex";
const AHRI_CHUNK: &str = "assets/characters/ahri/vfx.tex";
const AHRI_ORIGINAL: &[u8] = b"AHRI_ORIGINAL";

fn write_game_wad(game_dir: &Utf8Path, wad_name: &str, chunk_path: &str, bytes: &[u8]) {
    let champions_dir = game_dir.join("DATA").join("FINAL").join("Champions");
    fs::create_dir_all(champions_dir.as_std_path()).unwrap();

    let bytes = bytes.to_vec();
    let mut cursor = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(
            WadChunkBuilder::default()
                .with_path(chunk_path)
                .with_force_compression(WadChunkCompression::None),
        )
        .build_to_writer(&mut cursor, move |_hash, writer| {
            writer.write_all(&bytes)?;
            Ok(())
        })
        .unwrap();

    fs::write(
        champions_dir.join(wad_name).as_std_path(),
        cursor.into_inner(),
    )
    .unwrap();
}

/// Write a mod that overrides `AATROX_CHUNK` and ships `AHRI_CHUNK` (with the
/// given bytes) inside its `Aatrox.wad.client` directory.
fn write_mod_dir(root: &Utf8Path, name: &str, ahri_chunk_bytes: &[u8]) -> Utf8PathBuf {
    let mod_dir = root.join(name);
    let wad_dir = mod_dir.join("content").join("base").join(AATROX_WAD);
    for (chunk_path, bytes) in [
        (AATROX_CHUNK, b"AATROX_MODDED".as_slice()),
        (AHRI_CHUNK, ahri_chunk_bytes),
    ] {
        let file = wad_dir.join(chunk_path);
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

fn read_chunk(wad_path: &Utf8Path, chunk_path: &str) -> Option<Vec<u8>> {
    let file = fs::File::open(wad_path.as_std_path()).unwrap();
    let mut wad = Wad::mount(file).unwrap();
    let hash = resolve_chunk_hash(Utf8Path::new(chunk_path), b"").unwrap();
    let chunk = *wad.chunks().get(hash)?;
    Some(wad.load_chunk_decompressed(&chunk).unwrap().to_vec())
}

fn build_overlay(root: &Utf8Path, mod_dir: &Utf8PathBuf) -> Utf8PathBuf {
    let game_dir = root.join("Game");
    let profile_dir = root.join("profile");
    let overlay_root = profile_dir.join("overlay");

    let mut builder = OverlayBuilder::new(game_dir, overlay_root.clone(), profile_dir);
    builder.set_enabled_mods(vec![EnabledMod {
        id: "import-mod".to_string(),
        content: Box::new(FsModContent::new(mod_dir.clone())),
        enabled_layers: None,
    }]);
    builder.build().unwrap();
    overlay_root
}

/// A byte-identical copy of another WAD's chunk shipped under the mod's own
/// WAD directory must be added to that WAD as a new entry — and the WAD that
/// already holds the original must be left alone.
#[test]
fn identical_cross_wad_chunk_is_added_to_declared_wad() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game_dir = root.join("Game");
    write_game_wad(&game_dir, AATROX_WAD, AATROX_CHUNK, b"AATROX_ORIGINAL");
    write_game_wad(&game_dir, AHRI_WAD, AHRI_CHUNK, AHRI_ORIGINAL);

    let mod_dir = write_mod_dir(&root, "identical-import", AHRI_ORIGINAL);
    let overlay_root = build_overlay(&root, &mod_dir);

    let champions = overlay_root.join("DATA").join("FINAL").join("Champions");
    let overlay_aatrox = champions.join(AATROX_WAD);
    assert_eq!(
        read_chunk(&overlay_aatrox, AATROX_CHUNK).as_deref(),
        Some(b"AATROX_MODDED".as_slice())
    );
    assert_eq!(
        read_chunk(&overlay_aatrox, AHRI_CHUNK).as_deref(),
        Some(AHRI_ORIGINAL),
        "the imported original chunk must be present in the mod's target WAD"
    );
    assert!(
        !champions.join(AHRI_WAD).as_std_path().exists(),
        "an identical import must not rewrite the WAD that already holds the original"
    );
}

/// A *modified* copy of another WAD's chunk shipped under the mod's own WAD
/// directory must land in both: the WAD that holds the original (as an
/// override) and the mod's declared WAD (as a new entry).
#[test]
fn modified_cross_wad_chunk_lands_in_both_wads() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game_dir = root.join("Game");
    write_game_wad(&game_dir, AATROX_WAD, AATROX_CHUNK, b"AATROX_ORIGINAL");
    write_game_wad(&game_dir, AHRI_WAD, AHRI_CHUNK, AHRI_ORIGINAL);

    let mod_dir = write_mod_dir(&root, "modified-import", b"AHRI_MODDED");
    let overlay_root = build_overlay(&root, &mod_dir);

    let champions = overlay_root.join("DATA").join("FINAL").join("Champions");
    assert_eq!(
        read_chunk(&champions.join(AATROX_WAD), AHRI_CHUNK).as_deref(),
        Some(b"AHRI_MODDED".as_slice())
    );
    assert_eq!(
        read_chunk(&champions.join(AHRI_WAD), AHRI_CHUNK).as_deref(),
        Some(b"AHRI_MODDED".as_slice())
    );
}
