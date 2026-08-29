//! End-to-end tests for *pass-through*: a mod whose container already holds a
//! chunk in a WAD's stored form has those bytes copied into the overlay WAD
//! verbatim, never decoded and re-encoded.
//!
//! The two guarantees that matter to the game are pinned here: the bytes reach
//! the overlay unchanged, and the checksum the overlay TOC carries is the one
//! computed over them - not whatever the container claimed. The client kills the
//! process over a chunk whose checksum disagrees with its bytes, so a container
//! shipping wrong metadata must not be able to put that value in a WAD it loads.

use camino::{Utf8Path, Utf8PathBuf};
use ltk_overlay::utils::resolve_chunk_hash;
use ltk_overlay::{EnabledMod, FantomeContent, OverlayBuildResult, OverlayBuilder};
use ltk_wad::{Wad, WadBuilder, WadChunk, WadChunkBuilder, WadChunkCompression, WadHash};
use std::fs;
use std::io::{Cursor, Write};
use xxhash_rust::xxh3::xxh3_64;

const AATROX_WAD: &str = "Aatrox.wad.client";
const AATROX_CHUNK: &str = "assets/characters/aatrox/skin0.tex";
const GAME_BYTES: &[u8] = b"the game's own aatrox skin, uncompressed";
const MOD_BYTES: &[u8] = b"a modded aatrox skin, long enough that zstd has something to chew on";

/// A v3.4 WAD's chunk count, which its 268-byte header precedes.
const CHUNK_COUNT_OFFSET: usize = 268;
/// A v3.4 WAD's first TOC entry, which the chunk count precedes.
const TOC_OFFSET: usize = 272;
/// Offset of an entry's checksum field within it.
const TOC_CHECKSUM_FIELD: usize = 24;

fn chunk_hash() -> WadHash {
    resolve_chunk_hash(Utf8Path::new(AATROX_CHUNK), b"").expect("chunk path hashes")
}

/// A WAD holding `AATROX_CHUNK` with `bytes` under `compression`.
fn build_wad(bytes: &[u8], compression: WadChunkCompression) -> Vec<u8> {
    let bytes = bytes.to_vec();
    let mut cursor = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(
            WadChunkBuilder::default()
                .with_path(AATROX_CHUNK)
                .with_force_compression(compression),
        )
        .build_to_writer(&mut cursor, move |_hash, writer| {
            writer.write_all(&bytes)?;
            Ok(())
        })
        .expect("fixture WAD builds");
    cursor.into_inner()
}

fn write_game_wad(game_dir: &Utf8Path) {
    let champions = game_dir.join("DATA").join("FINAL").join("Champions");
    fs::create_dir_all(champions.as_std_path()).expect("game directory is creatable");
    fs::write(
        champions.join(AATROX_WAD).as_std_path(),
        build_wad(GAME_BYTES, WadChunkCompression::None),
    )
    .expect("game WAD is writable");
}

/// Overwrite the checksum a WAD's TOC claims for its only chunk.
///
/// # Panics
///
/// Panics when the WAD does not hold exactly the fixture's one chunk, which
/// would mean the offsets patched here point at something else.
fn claim_checksum(wad: &mut [u8], checksum: u64) {
    let count = u32::from_le_bytes(
        wad[CHUNK_COUNT_OFFSET..TOC_OFFSET]
            .try_into()
            .expect("a chunk count"),
    );
    assert_eq!(count, 1, "the fixture WAD holds exactly one chunk");
    assert_eq!(
        u64::from_le_bytes(
            wad[TOC_OFFSET..TOC_OFFSET + 8]
                .try_into()
                .expect("a path hash")
        ),
        chunk_hash().0,
        "the first TOC entry must be the fixture's chunk"
    );

    let field = TOC_OFFSET + TOC_CHECKSUM_FIELD;
    wad[field..field + 8].copy_from_slice(&checksum.to_le_bytes());
}

/// A `.fantome` archive whose base layer ships `packed_wad` as a packed
/// `Aatrox.wad.client`.
fn fantome_with_packed_wad(packed_wad: Vec<u8>) -> Cursor<Vec<u8>> {
    let info = serde_json::to_vec(&ltk_fantome::FantomeInfo {
        name: "Passthrough Mod".to_string(),
        author: "Author".to_string(),
        version: "1.0.0".to_string(),
        description: String::new(),
        license: None,
        tags: Vec::new(),
        champions: Vec::new(),
        maps: Vec::new(),
        layers: std::collections::HashMap::new(),
        hashtables: Vec::new(),
        extra: Default::default(),
    })
    .expect("fantome info serializes");

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in [
        ("META/info.json", info),
        (&format!("WAD/{AATROX_WAD}"), packed_wad),
    ] {
        zip.start_file(name, options).expect("zip entry starts");
        zip.write_all(&bytes).expect("zip entry is writable");
    }

    let mut cursor = zip.finish().expect("archive finishes");
    cursor.set_position(0);
    cursor
}

/// Build an overlay from a fantome holding `packed_wad`, and return the result
/// alongside the overlay's `Aatrox.wad.client`.
fn build_overlay(root: &Utf8Path, packed_wad: Vec<u8>) -> (OverlayBuildResult, Utf8PathBuf) {
    let game_dir = root.join("Game");
    write_game_wad(&game_dir);

    let profile_dir = root.join("profile");
    let overlay_root = profile_dir.join("overlay");

    let mut builder = OverlayBuilder::new(game_dir, overlay_root.clone(), profile_dir);
    builder.set_enabled_mods(vec![EnabledMod {
        id: "passthrough-mod".to_string(),
        content: Box::new(
            FantomeContent::new(fantome_with_packed_wad(packed_wad)).expect("the fantome mounts"),
        ),
        enabled_layers: None,
    }]);
    let result = builder.build().expect("the overlay builds");

    let overlay_wad = overlay_root
        .join("DATA")
        .join("FINAL")
        .join("Champions")
        .join(AATROX_WAD);
    (result, overlay_wad)
}

/// The TOC entry and stored bytes a WAD holds for the fixture's chunk.
fn stored_chunk(wad_path: &Utf8Path) -> (WadChunk, Vec<u8>) {
    let file = fs::File::open(wad_path.as_std_path()).expect("WAD opens");
    let mut wad = Wad::mount(file).expect("WAD mounts");
    let chunk = *wad
        .chunks()
        .get(chunk_hash())
        .expect("the WAD holds the chunk");
    let stored = wad
        .load_chunk_raw(&chunk)
        .expect("chunk data is inside the file")
        .to_vec();
    (chunk, stored)
}

/// The whole point of a pass-through: the container's own compressed bytes land
/// in the overlay unchanged, with a TOC that describes them.
#[test]
fn a_packed_chunk_reaches_the_overlay_without_being_re_encoded() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    let packed_wad = build_wad(MOD_BYTES, WadChunkCompression::Zstd);
    let mod_stored = {
        let path = root.join("mod-source.wad.client");
        fs::create_dir_all(root.as_std_path()).unwrap();
        fs::write(path.as_std_path(), &packed_wad).unwrap();
        stored_chunk(&path).1
    };

    let (result, overlay_wad) = build_overlay(&root, packed_wad);
    let (chunk, stored) = stored_chunk(&overlay_wad);

    assert!(
        result.checksum_mismatches.is_empty(),
        "an honest container must produce no mismatch report"
    );
    assert_eq!(
        stored, mod_stored,
        "the mod's stored bytes must be copied into the overlay verbatim"
    );
    assert_eq!(chunk.compression_type, WadChunkCompression::Zstd);
    assert_eq!(
        chunk.checksum,
        xxh3_64(&stored),
        "the overlay TOC checksum must match the bytes the file holds"
    );
    assert_eq!(
        zstd::decode_all(stored.as_slice()).unwrap(),
        MOD_BYTES,
        "and the copied bytes must still decode to the mod's content"
    );
}

/// Fantome tools in the wild ship wrong checksums over perfectly good bytes.
/// The build reports that and carries on, and the overlay carries the checksum
/// recomputed during the copy - never the claim (ADR-0001).
#[test]
fn a_lying_container_checksum_is_reported_and_never_written() {
    const LIE: u64 = 0xDEAD_BEEF;

    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    let mut packed_wad = build_wad(MOD_BYTES, WadChunkCompression::Zstd);
    claim_checksum(&mut packed_wad, LIE);

    let (result, overlay_wad) = build_overlay(&root, packed_wad);
    let (chunk, stored) = stored_chunk(&overlay_wad);

    assert_eq!(
        chunk.checksum,
        xxh3_64(&stored),
        "the overlay TOC must carry the checksum of its own bytes"
    );
    assert_ne!(chunk.checksum, LIE, "never the container's claim");
    assert_eq!(
        zstd::decode_all(stored.as_slice()).unwrap(),
        MOD_BYTES,
        "a wrong checksum over good bytes must not cost the mod its content"
    );

    let [mismatch] = result.checksum_mismatches.as_slice() else {
        panic!(
            "expected one reported mismatch, got {:?}",
            result.checksum_mismatches
        );
    };
    assert_eq!(mismatch.mod_id, "passthrough-mod");
    assert!(
        mismatch.wad_name.eq_ignore_ascii_case(AATROX_WAD),
        "the report must name the WAD the chunk was read from, got '{}'",
        mismatch.wad_name
    );
    assert_eq!(mismatch.path_hash, chunk_hash());
    assert_eq!(mismatch.claimed, LIE);
    assert_eq!(mismatch.computed, chunk.checksum);
}
