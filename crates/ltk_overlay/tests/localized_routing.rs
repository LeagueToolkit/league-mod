//! End-to-end tests for localized-WAD routing: original localized champion
//! WADs (`X.<locale>.wad.client`) only ever hold locale-tagged VO audio, yet
//! mods routinely pack their whole content into one. Non-audio overrides
//! declared under a localized champion WAD must be routed to the champion's
//! real WAD; audio overrides and non-champion localized WADs (`Localized/`)
//! keep their declared target.

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_overlay::utils::resolve_chunk_hash;
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuilder};
use ltk_wad::{Wad, WadBuilder, WadChunkBuilder, WadChunkCompression};
use std::fs;
use std::io::{Cursor, Write};

const MAIN_WAD: &str = "Testchamp.wad.client";
const LOC_WAD: &str = "Testchamp.en_US.wad.client";
const MAIN_TEX: &str = "assets/characters/testchamp/base/body.tex";
const VO: &str = "assets/sounds/wwise2016/vo/en_us/characters/testchamp/base_vo_audio.wpk";
const CUSTOM_TEX: &str = "assets/characters/testchamp/skins/base/custom_tx.tex";
const CUSTOM_VO: &str = "assets/sounds/wwise2016/vo/en_us/characters/testchamp/custom_vo.bnk";

/// A Wwise-package header (`r3d2` + version 1) — identified as audio.
fn wpk(data: &[u8]) -> Vec<u8> {
    let mut bytes = b"r3d2\x01\x00\x00\x00".to_vec();
    bytes.extend_from_slice(data);
    bytes
}

/// A Wwise-bank header — identified as audio.
fn bnk(data: &[u8]) -> Vec<u8> {
    let mut bytes = b"BKHD".to_vec();
    bytes.extend_from_slice(data);
    bytes
}

/// A texture header — identified as non-audio.
fn tex(data: &[u8]) -> Vec<u8> {
    let mut bytes = b"TEX\0".to_vec();
    bytes.extend_from_slice(data);
    bytes
}

fn write_game_wad_at(
    game_dir: &Utf8Path,
    sub_dir: &str,
    wad_name: &str,
    chunks: &[(&str, Vec<u8>)],
) {
    let dir = game_dir.join("DATA").join("FINAL").join(sub_dir);
    fs::create_dir_all(dir.as_std_path()).unwrap();

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
                resolve_chunk_hash(Utf8Path::new(path), b"").unwrap(),
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

    fs::write(dir.join(wad_name).as_std_path(), cursor.into_inner()).unwrap();
}

fn write_game(game_dir: &Utf8Path) {
    write_game_wad_at(
        game_dir,
        "Champions",
        MAIN_WAD,
        &[(MAIN_TEX, tex(b"original"))],
    );
    write_game_wad_at(game_dir, "Champions", LOC_WAD, &[(VO, wpk(b"original"))]);
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

fn build_overlay(root: &Utf8Path, mod_dir: &Utf8PathBuf) -> Utf8PathBuf {
    let overlay_root = root.join("profile").join("overlay");
    let mut builder = OverlayBuilder::new(
        root.join("Game"),
        overlay_root.clone(),
        root.join("profile"),
    );
    builder.set_enabled_mods(vec![EnabledMod {
        id: "test-mod".to_string(),
        content: Box::new(FsModContent::new(mod_dir.clone())),
        enabled_layers: None,
    }]);
    builder.build().unwrap();
    overlay_root
}

fn read_chunk(wad_path: &Utf8Path, chunk_path: &str) -> Option<Vec<u8>> {
    let file = fs::File::open(wad_path.as_std_path()).unwrap();
    let mut wad = Wad::mount(file).unwrap();
    let hash = resolve_chunk_hash(Utf8Path::new(chunk_path), b"").unwrap();
    let chunk = *wad.chunks().get(hash)?;
    Some(wad.load_chunk_decompressed(&chunk).unwrap().to_vec())
}

/// A whole mod packed under the localized champion WAD: non-audio content is
/// re-homed to the real champion WAD; audio (custom and VO overrides) stays
/// localized; known main-WAD chunks are not duplicated into the localized WAD.
#[test]
fn non_audio_overrides_are_rehomed_to_the_champion_wad() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    write_game(&root.join("Game"));

    let mod_dir = write_mod_dir(
        &root,
        "packed-into-localized",
        &[
            (LOC_WAD, CUSTOM_TEX, tex(b"custom")),
            (LOC_WAD, CUSTOM_VO, bnk(b"custom-voice")),
            (LOC_WAD, VO, wpk(b"modded-voice")),
            (LOC_WAD, MAIN_TEX, tex(b"modded")),
        ],
    );
    let overlay = build_overlay(&root, &mod_dir);

    let champions = overlay.join("DATA").join("FINAL").join("Champions");
    let main_wad = champions.join(MAIN_WAD);
    let loc_wad = champions.join(LOC_WAD);

    // Non-audio chunks live in the champion's real WAD, not the localized one.
    assert_eq!(
        read_chunk(&main_wad, CUSTOM_TEX).as_deref(),
        Some(&tex(b"custom")[..])
    );
    assert_eq!(
        read_chunk(&main_wad, MAIN_TEX).as_deref(),
        Some(&tex(b"modded")[..])
    );
    assert_eq!(read_chunk(&loc_wad, CUSTOM_TEX), None);
    assert_eq!(read_chunk(&loc_wad, MAIN_TEX), None);

    // Audio stays localized and is not pulled into the main WAD.
    assert_eq!(
        read_chunk(&loc_wad, CUSTOM_VO).as_deref(),
        Some(&bnk(b"custom-voice")[..])
    );
    assert_eq!(
        read_chunk(&loc_wad, VO).as_deref(),
        Some(&wpk(b"modded-voice")[..])
    );
    assert_eq!(read_chunk(&main_wad, CUSTOM_VO), None);
    assert_eq!(read_chunk(&main_wad, VO), None);
}

/// A pure VO mod keeps targeting the localized WAD; the champion's main WAD
/// is not even built.
#[test]
fn pure_vo_mod_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    write_game(&root.join("Game"));

    let mod_dir = write_mod_dir(&root, "vo-only", &[(LOC_WAD, VO, wpk(b"new-voice"))]);
    let overlay = build_overlay(&root, &mod_dir);

    let champions = overlay.join("DATA").join("FINAL").join("Champions");
    assert_eq!(
        read_chunk(&champions.join(LOC_WAD), VO).as_deref(),
        Some(&wpk(b"new-voice")[..])
    );
    assert!(!champions.join(MAIN_WAD).as_std_path().exists());
}

/// Localized WADs outside `Champions/` (the `Localized/` string-table WADs)
/// are never delocalized, even for non-audio content.
#[test]
fn non_champion_localized_wads_are_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game_dir = root.join("Game");
    write_game(&game_dir);
    write_game_wad_at(
        &game_dir,
        "Localized",
        "Global.en_US.wad.client",
        &[("data/menu/en_us/main.stringtable", b"strings".to_vec())],
    );
    write_game_wad_at(
        &game_dir,
        "Localized",
        "Global.wad.client",
        &[("data/menu/fontconfig.txt", b"fonts".to_vec())],
    );

    let custom = "data/menu/en_us/custom.bin";
    let mod_dir = write_mod_dir(
        &root,
        "global-mod",
        &[("Global.en_US.wad.client", custom, b"not-audio".to_vec())],
    );
    let overlay = build_overlay(&root, &mod_dir);

    let localized = overlay.join("DATA").join("FINAL").join("Localized");
    assert_eq!(
        read_chunk(&localized.join("Global.en_US.wad.client"), custom).as_deref(),
        Some(b"not-audio".as_slice())
    );
    assert!(!localized.join("Global.wad.client").as_std_path().exists());
}
