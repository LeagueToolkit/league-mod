//! End-to-end coverage for a chunk two game WADs hold.
//!
//! The client validates a chunk shared by two mounted WADs against its compressed checksum,
//! so every overlay WAD holding one path hash has to hold one encoding of it. A build that
//! rebuilds one WAD and reuses another can otherwise leave two
//! (`docs/adr/0025-per-chunk-checksums-in-the-layout-record.md`).

mod common;

use camino::Utf8PathBuf;
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuilder};
use std::fs;

const AATROX: &str = "DATA/FINAL/Champions/Aatrox.wad.client";
const MAP11: &str = "DATA/FINAL/Maps/Shipping/Map11.wad.client";
const SHARED: &str = "assets/shared/common.bin";
const AATROX_ONLY: &str = "assets/characters/aatrox/skin0.bin";

/// The overlay's compressed bytes for `chunk_path` inside `wad_rel`.
fn overlay_chunk_checksum(overlay_root: &Utf8PathBuf, wad_rel: &str, chunk_path: &str) -> u64 {
    let facts = common::chunk_facts(&overlay_root.join(wad_rel));
    facts
        .get(&common::hash(chunk_path))
        .expect("the overlay WAD holds the chunk")
        .checksum
}

#[test]
fn a_reused_wad_disagreeing_about_a_shared_chunk_is_rebuilt() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game = root.join("Game");
    let profile = root.join("profile");
    let overlay = profile.join("overlay");

    // One chunk path lives in both game WADs, which is how the client shares it.
    common::write_game_wad(
        &game.join(AATROX),
        &[
            (SHARED, b"the original shared chunk"),
            (AATROX_ONLY, b"the original aatrox chunk"),
        ],
    );
    common::write_game_wad(&game.join(MAP11), &[(SHARED, b"the original shared chunk")]);

    // The mod overrides the shared chunk, so routing fans it to both WADs, and one chunk
    // only Aatrox holds, which routes to Aatrox alone.
    let mod_dir = common::write_mod_dir(
        &root,
        "shared-mod",
        "Aatrox.wad.client",
        &[
            (SHARED, b"a modded shared chunk"),
            (AATROX_ONLY, b"a modded aatrox chunk"),
        ],
    );

    let mut builder = OverlayBuilder::new(game.clone(), overlay.clone(), profile.clone());
    let enabled = || {
        vec![EnabledMod {
            id: "shared-mod".to_string(),
            content: Box::new(FsModContent::new(mod_dir.clone())),
            enabled_layers: None,
        }]
    };
    builder.set_enabled_mods(enabled());
    let first = builder.build().unwrap();
    assert_eq!(first.wads_built.len(), 2, "{first:?}");

    let shared_checksum = overlay_chunk_checksum(&overlay, MAP11, SHARED);
    assert_eq!(
        overlay_chunk_checksum(&overlay, AATROX, SHARED),
        shared_checksum,
        "one build must write one encoding of a shared chunk"
    );

    // Stand in for a compressor whose output moved between builds: the record says Map11
    // holds bytes this build would not produce. Nothing else about Map11 changes, so its
    // fingerprint still matches and it is chosen for reuse.
    let state_path = profile.join("overlay.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(state_path.as_std_path()).unwrap()).unwrap();
    let key = common::hash(SHARED).0.to_string();
    state["wadLayouts"][MAP11]["overrides"][&key]["checksum"] =
        serde_json::json!(shared_checksum ^ 0xFFFF_FFFF);
    fs::write(
        state_path.as_std_path(),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();

    // Give Aatrox a reason to rebuild that Map11 does not share.
    fs::write(
        mod_dir
            .join("content/base/Aatrox.wad.client")
            .join(AATROX_ONLY)
            .as_std_path(),
        b"an edited aatrox chunk",
    )
    .unwrap();

    builder.set_enabled_mods(enabled());
    let second = builder.build().unwrap();

    assert!(
        second
            .wads_built
            .iter()
            .any(|path| path.file_name() == Some("Map11.wad.client")),
        "Map11 records bytes this build does not produce, so it is rebuilt: {second:?}"
    );
    assert!(second.wads_reused.is_empty(), "{second:?}");
    assert_eq!(
        overlay_chunk_checksum(&overlay, AATROX, SHARED),
        overlay_chunk_checksum(&overlay, MAP11, SHARED),
        "both overlay WADs must carry one encoding of the shared chunk"
    );
}
