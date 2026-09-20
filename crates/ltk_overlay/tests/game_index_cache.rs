//! The overlay's use of the game index cache and its report of skipped archives.

mod common;

use camino::Utf8PathBuf;
use common::{write_game_wad, write_mod_dir};
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuilder, StateDir};
use std::fs;

const GAME_WAD: &str = "Aatrox.wad.client";
const CHUNK_PATH: &str = "assets/characters/aatrox/skin0.tex";

struct Setup {
    _tmp: tempfile::TempDir,
    game_dir: Utf8PathBuf,
    profile_dir: Utf8PathBuf,
    mod_dir: Utf8PathBuf,
}

fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game_dir = root.join("Game");
    write_game_wad(
        &game_dir.join("DATA/FINAL/Champions").join(GAME_WAD),
        &[(CHUNK_PATH, b"GAME_ORIGINAL")],
    );
    let profile_dir = root.join("profile");
    let mod_dir = write_mod_dir(&root, "skin", GAME_WAD, &[(CHUNK_PATH, b"MOD_OVERRIDE")]);
    Setup {
        _tmp: tmp,
        game_dir,
        profile_dir,
        mod_dir,
    }
}

fn builder(setup: &Setup) -> OverlayBuilder {
    let mut builder = OverlayBuilder::new(
        setup.game_dir.clone(),
        setup.profile_dir.join("overlay"),
        setup.profile_dir.clone(),
    );
    builder.set_enabled_mods(vec![EnabledMod {
        id: "skin".to_string(),
        content: Box::new(FsModContent::new(setup.mod_dir.clone())),
        enabled_layers: None,
    }]);
    builder
}

#[test]
fn a_cache_from_a_previous_format_is_rebuilt_without_error() {
    let setup = setup();
    let state = StateDir::new(setup.profile_dir.clone());
    state.create().unwrap();

    // The previous format: one MessagePack map with a leading `version` field, as the
    // overlay's own index wrote it.
    let previous = rmp_serde::to_vec_named(&serde_json::json!({
        "version": 3,
        "game_fingerprint": 42u64,
        "wad_index": {},
        "hash_index": {},
        "subchunktoc_blocked": [],
    }))
    .unwrap();
    fs::write(state.game_index_cache().as_std_path(), previous).unwrap();

    let result = builder(&setup).build().unwrap();
    assert_eq!(result.wads_built.len(), 1);
    assert!(result.skipped_archives.is_empty());

    let rebuilt = ltk_game_index::GameIndex::load(&state.game_index_cache()).unwrap();
    assert_eq!(rebuilt.archives().len(), 1);
    assert_eq!(
        rebuilt.fingerprint(),
        ltk_game_index::GameIndex::fingerprint_of(&setup.game_dir).unwrap()
    );
}

#[test]
fn a_skipped_archive_surfaces_on_the_build_result() {
    let setup = setup();
    fs::write(
        setup
            .game_dir
            .join("DATA/FINAL/Champions/Broken.wad.client")
            .as_std_path(),
        b"not a wad",
    )
    .unwrap();

    let result = builder(&setup).build().unwrap();
    assert_eq!(result.wads_built.len(), 1);
    assert_eq!(result.skipped_archives.len(), 1);
    assert_eq!(
        result.skipped_archives[0].name,
        "Champions/Broken.wad.client"
    );
    assert!(result.skipped_archives[0].error.contains("mount"));

    // The cached index carries the skipped archive into the next build.
    let again = builder(&setup).build().unwrap();
    assert_eq!(again.skipped_archives, result.skipped_archives);
}
