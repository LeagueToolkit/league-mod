//! End-to-end tests for the incremental rebuild / skip decisions of
//! `OverlayBuilder::build`, against a synthetic game directory.
//!
//! Regression coverage for the stale-overlay bug: editing a mod's content
//! between builds must invalidate the exact-match skip even though the mod's
//! ID is unchanged (the workshop "Test again after a texture edit" flow).

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_overlay::utils::resolve_chunk_hash;
use ltk_overlay::wad_builder::{OverrideEncoding as _, build_patched_wad};
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuilder};
use ltk_wad::{EncodedChunk, Wad, WadBuilder, WadChunkBuilder, WadChunkCompression};
use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Write};

const GAME_WAD: &str = "Aatrox.wad.client";
const CHUNK_PATH: &str = "assets/characters/aatrox/skin0.tex";

fn write_game_wad(game_dir: &Utf8Path, chunk_bytes: Vec<u8>) {
    let champions_dir = game_dir.join("DATA").join("FINAL").join("Champions");
    fs::create_dir_all(champions_dir.as_std_path()).unwrap();

    let mut cursor = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(
            WadChunkBuilder::default()
                .with_path(CHUNK_PATH)
                .with_force_compression(WadChunkCompression::None),
        )
        .build_to_writer(&mut cursor, move |_hash, writer| {
            writer.write_all(&chunk_bytes)?;
            Ok(())
        })
        .unwrap();

    let wad_path = champions_dir.join(GAME_WAD);
    fs::write(wad_path.as_std_path(), cursor.into_inner()).unwrap();
}

fn write_mod_dir(root: &Utf8Path, name: &str, override_bytes: &[u8]) -> Utf8PathBuf {
    let mod_dir = root.join(name);
    let wad_dir = mod_dir.join("content").join("base").join(GAME_WAD);
    fs::create_dir_all(wad_dir.join("assets/characters/aatrox").as_std_path()).unwrap();
    fs::write(wad_dir.join(CHUNK_PATH).as_std_path(), override_bytes).unwrap();

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

fn read_overlay_chunk(overlay_root: &Utf8Path) -> Vec<u8> {
    let wad_path = overlay_root
        .join("DATA")
        .join("FINAL")
        .join("Champions")
        .join(GAME_WAD);
    let file = fs::File::open(wad_path.as_std_path()).unwrap();
    let mut wad = Wad::mount(file).unwrap();
    let hash = resolve_chunk_hash(Utf8Path::new(CHUNK_PATH), b"").unwrap();
    let chunk = *wad
        .chunks()
        .get(hash)
        .expect("patched WAD must contain the overridden chunk");
    wad.load_chunk_decompressed(&chunk).unwrap().to_vec()
}

#[test]
fn content_edit_invalidates_exact_match_skip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game_dir = root.join("Game");
    write_game_wad(&game_dir, b"GAME_ORIGINAL".to_vec());
    let profile_dir = root.join("profile");
    let overlay_root = profile_dir.join("overlay");

    let mod_dir = write_mod_dir(&root, "workshop-proj", b"MOD_V1");

    let mut builder = OverlayBuilder::new(game_dir, overlay_root.clone(), profile_dir);
    let enabled = || {
        vec![EnabledMod {
            id: "workshop:workshop-proj".to_string(),
            content: Box::new(FsModContent::new(mod_dir.clone())),
            enabled_layers: None,
        }]
    };

    builder.set_enabled_mods(enabled());
    let first = builder.build().unwrap();
    assert_eq!(first.wads_built.len(), 1);
    assert_eq!(read_overlay_chunk(&overlay_root), b"MOD_V1");

    // Unchanged content -> exact-match skip reuses the patched WAD.
    builder.set_enabled_mods(enabled());
    let rerun = builder.build().unwrap();
    assert!(rerun.wads_built.is_empty());
    assert_eq!(rerun.wads_reused.len(), 1);

    // Edit the override under the same mod ID. The new bytes differ in length
    // so the size-based part of the content fingerprint changes even when both
    // writes land within the same mtime second.
    let override_path = mod_dir
        .join("content")
        .join("base")
        .join(GAME_WAD)
        .join(CHUNK_PATH);
    fs::write(override_path.as_std_path(), b"MOD_V2_EDITED").unwrap();

    builder.set_enabled_mods(enabled());
    let after_edit = builder.build().unwrap();
    assert_eq!(
        after_edit.wads_built.len(),
        1,
        "content edit under an unchanged mod ID must trigger a rebuild"
    );
    assert_eq!(read_overlay_chunk(&overlay_root), b"MOD_V2_EDITED");
}

/// Overlay files no build state accounts for - e.g. WADs written by a build
/// that died before saving `overlay.json`, or leftover `.tmp` files - must be
/// removed on the next build, exact-match skip included, because the injector
/// serves everything in the overlay directory.
#[test]
fn orphan_overlay_files_are_swept_on_exact_match_skip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game_dir = root.join("Game");
    write_game_wad(&game_dir, b"GAME_ORIGINAL".to_vec());
    let profile_dir = root.join("profile");
    let overlay_root = profile_dir.join("overlay");

    let mod_dir = write_mod_dir(&root, "some-mod", b"MOD_V1");

    let mut builder = OverlayBuilder::new(game_dir, overlay_root.clone(), profile_dir);
    let enabled = || {
        vec![EnabledMod {
            id: "some-mod".to_string(),
            content: Box::new(FsModContent::new(mod_dir.clone())),
            enabled_layers: None,
        }]
    };

    builder.set_enabled_mods(enabled());
    builder.build().unwrap();

    let champions_dir = overlay_root.join("DATA").join("FINAL").join("Champions");
    let real_wad = champions_dir.join(GAME_WAD);
    assert!(real_wad.as_std_path().exists());

    // Plant orphans: an unrecorded WAD, a stray temp file, and an orphan in a
    // directory of its own (its emptied parents must be cleaned up too).
    let orphan_wad = champions_dir.join("Orphan.wad.client");
    fs::copy(real_wad.as_std_path(), orphan_wad.as_std_path()).unwrap();
    let stray_tmp = champions_dir.join(format!("{GAME_WAD}.tmp"));
    fs::write(stray_tmp.as_std_path(), b"partial junk").unwrap();
    let orphan_dir = overlay_root.join("DATA").join("FINAL").join("Maps");
    fs::create_dir_all(orphan_dir.as_std_path()).unwrap();
    let nested_orphan = orphan_dir.join("Map11.wad.client");
    fs::write(nested_orphan.as_std_path(), b"stale").unwrap();

    // Unchanged config -> exact-match skip, which must still sweep.
    builder.set_enabled_mods(enabled());
    let rerun = builder.build().unwrap();
    assert!(rerun.wads_built.is_empty());

    assert!(!orphan_wad.as_std_path().exists());
    assert!(!stray_tmp.as_std_path().exists());
    assert!(!nested_orphan.as_std_path().exists());
    assert!(
        !orphan_dir.as_std_path().exists(),
        "emptied parent directories must be cleaned up"
    );
    assert!(
        real_wad.as_std_path().exists(),
        "recorded overlay WADs must survive the sweep"
    );
}

/// A failing WAD build must not leave a partial file at the destination path
/// (or a `.tmp` beside it) - the injector would serve it to the game.
#[test]
fn failed_wad_build_leaves_no_partial_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game_dir = root.join("Game");
    write_game_wad(&game_dir, b"GAME_ORIGINAL".to_vec());

    let src = game_dir
        .join("DATA")
        .join("FINAL")
        .join("Champions")
        .join(GAME_WAD);
    let dst = root.join("out").join(GAME_WAD);
    let override_hashes =
        HashSet::from([resolve_chunk_hash(Utf8Path::new(CHUNK_PATH), b"").unwrap()]);

    let result = build_patched_wad(&src, &dst, &override_hashes, |_hash| {
        Err(ltk_overlay::Error::from(std::io::Error::other(
            "override source vanished mid-build",
        )))
    });

    assert!(result.is_err());
    assert!(!dst.as_std_path().exists());
    let leftovers: Vec<_> = fs::read_dir(root.join("out").as_std_path())
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    assert!(leftovers.is_empty(), "unexpected leftovers: {leftovers:?}");
}

/// The patched WAD must carry the source WAD's header signature and checksum
/// verbatim - Riot's RSA signature over the original TOC is the provenance
/// record that ltk_sig's merged-WAD verification recovers from overlay files.
#[test]
fn patched_wad_preserves_original_signature_and_checksum() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    let signature = [0xA5u8; 256];
    let checksum = 0xDEAD_BEEF_CAFE_F00D_u64;

    // A "signed" game WAD with one chunk.
    let mut cursor = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_prebuilt_signature(&signature)
        .with_prebuilt_checksum(checksum)
        .with_chunk(
            WadChunkBuilder::default()
                .with_path(CHUNK_PATH)
                .with_force_compression(WadChunkCompression::None),
        )
        .build_to_writer(&mut cursor, |_hash, writer| {
            writer.write_all(b"GAME_ORIGINAL")?;
            Ok(())
        })
        .unwrap();
    let src = root.join(GAME_WAD);
    fs::write(src.as_std_path(), cursor.into_inner()).unwrap();

    // Replace the existing chunk and insert a brand-new entry, covering both
    // TOC-mutating paths.
    let override_hash = resolve_chunk_hash(Utf8Path::new(CHUNK_PATH), b"").unwrap();
    let new_hash = resolve_chunk_hash(Utf8Path::new("assets/new/entry.bin"), b"").unwrap();
    let dst = root.join("out").join(GAME_WAD);
    build_patched_wad(
        &src,
        &dst,
        &HashSet::from([override_hash, new_hash]),
        |hash| EncodedChunk::compress(hash, b"MODDED"),
    )
    .unwrap();

    let patched = Wad::mount(fs::File::open(dst.as_std_path()).unwrap()).unwrap();
    assert_eq!(patched.signature(), &signature);
    assert_eq!(patched.checksum(), checksum);
}
