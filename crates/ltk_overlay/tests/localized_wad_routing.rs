//! End-to-end tests for mods that declare a localized WAD directory.
//!
//! League installs one locale and the in-game integrity scan skips localized
//! WADs, so new content declared into `Graves.en_US.wad.client` is routed to the
//! unlocalized sibling as well. Genuinely localized content keeps to its own WAD.

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_overlay::utils::resolve_chunk_hash;
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuilder};
use ltk_wad::{Wad, WadBuilder, WadChunkBuilder, WadChunkCompression, WadHash};
use std::fs;
use std::io::{Cursor, Write};

const BASE_WAD: &str = "Graves.wad.client";
const LOCALIZED_WAD: &str = "Graves.en_US.wad.client";

/// A chunk the unlocalized WAD ships, standing in for a champion mesh.
const BASE_CHUNK: &str = "assets/characters/graves/skins/skin0/graves.skn";
/// A chunk only the localized WAD ships, standing in for localized audio.
const LOCALIZED_CHUNK: &str = "assets/characters/graves/skins/skin0/graves_vo.bnk";
/// A path no game WAD holds, so it reaches the overlay only through routing.
const NEW_CHUNK: &str = "assets/characters/graves/skins/skin42/graves_skin42.skn";

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

/// A mod shipping `files` inside its `wad_name` directory.
fn write_mod_dir(
    root: &Utf8Path,
    name: &str,
    wad_name: &str,
    files: &[(&str, &[u8])],
) -> Utf8PathBuf {
    let mod_dir = root.join(name);
    let wad_dir = mod_dir.join("content").join("base").join(wad_name);
    for (chunk_path, bytes) in files {
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
        hashtables: vec![],
    };
    fs::write(
        mod_dir.join("mod.config.json").as_std_path(),
        serde_json::to_string_pretty(&project).unwrap(),
    )
    .unwrap();
    mod_dir
}

fn read_chunk(wad_path: &Utf8Path, chunk_path: &str) -> Option<Vec<u8>> {
    let file = fs::File::open(wad_path.as_std_path()).ok()?;
    let mut wad = Wad::mount(file).unwrap();
    let hash = resolve_chunk_hash(Utf8Path::new(chunk_path), b"").unwrap();
    let chunk = *wad.chunks().get(WadHash(hash))?;
    Some(wad.load_chunk_decompressed(&chunk).unwrap().to_vec())
}

/// The stored `(compression_type, compressed checksum)` pair League compares
/// across mounted WADs holding the same path.
fn chunk_identity(wad_path: &Utf8Path, chunk_path: &str) -> Option<(WadChunkCompression, u64)> {
    let file = fs::File::open(wad_path.as_std_path()).ok()?;
    let wad = Wad::mount(file).unwrap();
    let hash = resolve_chunk_hash(Utf8Path::new(chunk_path), b"").unwrap();
    let chunk = wad.chunks().get(WadHash(hash))?;
    Some((chunk.compression_type, chunk.checksum))
}

fn build_overlay(root: &Utf8Path, mod_dir: &Utf8PathBuf) -> Utf8PathBuf {
    let profile_dir = root.join("profile");
    let overlay_root = profile_dir.join("overlay");

    let mut builder = OverlayBuilder::new(root.join("Game"), overlay_root.clone(), profile_dir);
    builder.set_enabled_mods(vec![EnabledMod {
        id: "localized-mod".to_string(),
        content: Box::new(FsModContent::new(mod_dir.clone())),
        enabled_layers: None,
    }]);
    builder.build().unwrap();
    overlay_root
}

/// A game shipping both the unlocalized WAD and its `en_US` sibling.
fn write_game(root: &Utf8Path) {
    let game_dir = root.join("Game");
    write_game_wad(&game_dir, BASE_WAD, BASE_CHUNK, b"GRAVES_ORIGINAL");
    write_game_wad(&game_dir, LOCALIZED_WAD, LOCALIZED_CHUNK, b"VO_ORIGINAL");
}

/// New content declared into a localized WAD also reaches the unlocalized
/// sibling, so players on other locales load it and the integrity scan can
/// resolve it from the champion WAD that references it.
#[test]
fn new_content_in_a_localized_wad_reaches_the_sibling() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    write_game(&root);

    let mod_dir = write_mod_dir(
        &root,
        "misplaced-skin",
        LOCALIZED_WAD,
        &[(NEW_CHUNK, b"NEW_MESH")],
    );
    let overlay_root = build_overlay(&root, &mod_dir);
    let champions = overlay_root.join("DATA").join("FINAL").join("Champions");

    assert_eq!(
        read_chunk(&champions.join(LOCALIZED_WAD), NEW_CHUNK).as_deref(),
        Some(b"NEW_MESH".as_slice()),
        "the declared WAD keeps the content the mod placed there"
    );
    assert_eq!(
        read_chunk(&champions.join(BASE_WAD), NEW_CHUNK).as_deref(),
        Some(b"NEW_MESH".as_slice()),
        "the unlocalized sibling must carry it too, or only en_US players load it \
         and the integrity scan cannot resolve it"
    );
    assert_eq!(
        chunk_identity(&champions.join(LOCALIZED_WAD), NEW_CHUNK),
        chunk_identity(&champions.join(BASE_WAD), NEW_CHUNK),
        "both copies must share one compression and compressed checksum, or the \
         game's cross-WAD consistency check rejects the install"
    );
}

/// Genuinely localized content hash-matches its own WAD, so it never reaches
/// the fallback and is not copied into the unlocalized sibling.
#[test]
fn localized_content_stays_in_its_own_wad() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    write_game(&root);

    let mod_dir = write_mod_dir(
        &root,
        "localized-vo",
        LOCALIZED_WAD,
        &[(LOCALIZED_CHUNK, b"VO_MODDED")],
    );
    let overlay_root = build_overlay(&root, &mod_dir);
    let champions = overlay_root.join("DATA").join("FINAL").join("Champions");

    assert_eq!(
        read_chunk(&champions.join(LOCALIZED_WAD), LOCALIZED_CHUNK).as_deref(),
        Some(b"VO_MODDED".as_slice())
    );
    assert_eq!(
        read_chunk(&champions.join(BASE_WAD), LOCALIZED_CHUNK),
        None,
        "per-locale content must not be duplicated into the unlocalized WAD"
    );
}

/// A mod that placed its content correctly routes exactly as before.
#[test]
fn a_correctly_placed_mod_is_unaffected() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    write_game(&root);

    let mod_dir = write_mod_dir(
        &root,
        "correct-skin",
        BASE_WAD,
        &[(BASE_CHUNK, b"GRAVES_MODDED"), (NEW_CHUNK, b"NEW_MESH")],
    );
    let overlay_root = build_overlay(&root, &mod_dir);
    let champions = overlay_root.join("DATA").join("FINAL").join("Champions");

    assert_eq!(
        read_chunk(&champions.join(BASE_WAD), BASE_CHUNK).as_deref(),
        Some(b"GRAVES_MODDED".as_slice())
    );
    assert_eq!(
        read_chunk(&champions.join(BASE_WAD), NEW_CHUNK).as_deref(),
        Some(b"NEW_MESH".as_slice())
    );
    assert_eq!(
        read_chunk(&champions.join(LOCALIZED_WAD), NEW_CHUNK),
        None,
        "an unlocalized declaration has no sibling to widen to"
    );
}
