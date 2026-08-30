//! The patched-WAD file layout: a copied source data region followed by an
//! override tail.
//!
//! These tests pin the properties the incremental rebuild depends on. The
//! source region must arrive intact - including the now-unreferenced bytes of
//! chunks that were overridden, which is what lets a later build drop an
//! override with a TOC edit alone - and every override must live past its end,
//! so rewriting the tail never disturbs a transient chunk.

mod common;

use camino::Utf8PathBuf;
use common::{assert_wad_is_well_formed, chunk_facts, hash, write_game_wad};
use ltk_overlay::wad_builder::{OverrideEncoding as _, PatchedWadStats, build_patched_wad};
use ltk_wad::{EncodedChunk, Wad, WadHash};
use std::collections::HashSet;
use std::fs;

const KEPT: &str = "assets/kept.bin";
const REPLACED: &str = "assets/replaced.bin";
const ALSO_KEPT: &str = "assets/also_kept.bin";
const ADDED: &str = "assets/added.bin";

const KEPT_BYTES: &[u8] = b"the chunk nobody touched, padded out to a useful length";
const REPLACED_BYTES: &[u8] = b"the original bytes of the chunk the mod replaces";
const ALSO_KEPT_BYTES: &[u8] = b"another untouched chunk, so the region spans several";
const OVERRIDE_BYTES: &[u8] = b"MODDED CONTENT, longer than what it replaced, by design";
const ADDED_BYTES: &[u8] = b"a brand-new entry the source WAD never held";

/// Build a three-chunk source WAD and patch it with `overrides`.
///
/// Returns `(source path, patched path, stats)`.
fn patch_fixture(
    tmp: &tempfile::TempDir,
    overrides: &[(&str, &[u8])],
) -> (Utf8PathBuf, Utf8PathBuf, PatchedWadStats) {
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("temp dir is UTF-8");
    let src = root.join("game").join("Fixture.wad.client");
    write_game_wad(
        &src,
        &[
            (KEPT, KEPT_BYTES),
            (REPLACED, REPLACED_BYTES),
            (ALSO_KEPT, ALSO_KEPT_BYTES),
        ],
    );

    let dst = root.join("overlay").join("Fixture.wad.client");
    let by_hash: Vec<(WadHash, Vec<u8>)> = overrides
        .iter()
        .map(|(path, bytes)| (hash(path), bytes.to_vec()))
        .collect();
    let override_hashes: HashSet<WadHash> = by_hash.iter().map(|(h, _)| *h).collect();

    let stats = build_patched_wad(&src, &dst, &override_hashes, |h| {
        let bytes = by_hash
            .iter()
            .find(|(candidate, _)| *candidate == h)
            .map(|(_, bytes)| bytes.clone())
            .expect("the writer only asks for hashes it was given");
        EncodedChunk::compress(h, &bytes)
    })
    .expect("patched WAD builds");

    (src, dst, stats)
}

/// The source data region is copied as one block, so the bytes of a chunk that
/// an override replaced are still in the file - unreferenced by the TOC, but
/// present. Removing that override later is then a TOC edit alone, with no need
/// to reopen the game WAD.
#[test]
fn overridden_chunks_keep_their_original_bytes_in_the_region() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, dst, stats) = patch_fixture(&tmp, &[(REPLACED, OVERRIDE_BYTES)]);

    let source = Wad::mount(fs::File::open(src.as_std_path()).unwrap()).unwrap();
    let original = *source
        .chunks()
        .get(hash(REPLACED))
        .expect("the source WAD holds the replaced chunk");

    let shifted = (original.data_offset as i64 + stats.layout.offset_delta) as usize;
    let patched = fs::read(dst.as_std_path()).unwrap();
    assert_eq!(
        &patched[shifted..shifted + original.compressed_size],
        REPLACED_BYTES,
        "the replaced chunk's original bytes must survive in the copied region"
    );
}

/// Overrides and new entries go past the source region's end, which is what
/// makes an incremental rebuild able to truncate at the tail and start over
/// without touching a single transient chunk.
#[test]
fn overrides_and_new_entries_live_in_the_tail() {
    let tmp = tempfile::tempdir().unwrap();
    let (_src, dst, stats) =
        patch_fixture(&tmp, &[(REPLACED, OVERRIDE_BYTES), (ADDED, ADDED_BYTES)]);

    let patched = Wad::mount(fs::File::open(dst.as_std_path()).unwrap()).unwrap();
    for path in [REPLACED, ADDED] {
        let chunk = patched
            .chunks()
            .get(hash(path))
            .unwrap_or_else(|| panic!("the patched WAD holds {path}"));
        assert!(
            chunk.data_offset as u64 >= stats.layout.tail_offset,
            "{path} sits at {} but the tail starts at {}",
            chunk.data_offset,
            stats.layout.tail_offset
        );
    }

    for path in [KEPT, ALSO_KEPT] {
        let chunk = patched
            .chunks()
            .get(hash(path))
            .unwrap_or_else(|| panic!("the patched WAD holds {path}"));
        assert!(
            (chunk.data_offset as u64) < stats.layout.tail_offset,
            "{path} was passed through and must stay inside the copied region"
        );
    }
}

/// A passed-through chunk carries its source TOC entry unchanged apart from the
/// offset: its bytes were copied verbatim, so the sizes, compression, frame
/// fields and checksum that described them still do.
#[test]
fn transient_chunks_keep_their_bytes_and_toc_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, dst, _stats) = patch_fixture(&tmp, &[(REPLACED, OVERRIDE_BYTES)]);

    let source = chunk_facts(&src);
    let patched = chunk_facts(&dst);

    for path in [KEPT, ALSO_KEPT] {
        assert_eq!(
            patched.get(&hash(path)),
            source.get(&hash(path)),
            "{path} was not overridden and must come through untouched"
        );
    }
}

/// Whatever the layout, the archive itself has to stay valid: strictly
/// ascending TOC, an honest chunk count, and no entry pointing past the file.
#[test]
fn the_patched_wad_stays_well_formed() {
    let tmp = tempfile::tempdir().unwrap();
    let (_src, dst, stats) =
        patch_fixture(&tmp, &[(REPLACED, OVERRIDE_BYTES), (ADDED, ADDED_BYTES)]);

    assert_wad_is_well_formed(&dst);
    assert_eq!(stats.chunks_written, 4);
    assert_eq!(stats.overrides_applied, 2);
    assert_eq!(stats.new_entries_added, 1);
    assert_eq!(stats.chunks_transient, 2);

    let patched = Wad::mount(fs::File::open(dst.as_std_path()).unwrap()).unwrap();
    assert_eq!(patched.chunks().len(), 4);
}

/// With TOC slack at zero the reserved capacity is exactly the entry count, so
/// no gap exists between the last TOC entry and the first data byte. Enabling
/// slack is gated on proving the game tolerates that gap.
#[test]
fn toc_capacity_is_the_entry_count_while_slack_is_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let (_src, _dst, stats) =
        patch_fixture(&tmp, &[(REPLACED, OVERRIDE_BYTES), (ADDED, ADDED_BYTES)]);

    assert_eq!(stats.layout.toc_capacity as usize, stats.chunks_written);
}
