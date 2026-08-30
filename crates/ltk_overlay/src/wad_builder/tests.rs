use super::*;
use crate::content::CompressedChunk;
use crate::test_support::{hash, write_game_wad};

const WAD_REL: &str = "DATA/FINAL/Champions/Test.wad.client";
const SKIN: &str = "assets/characters/test/skins/skin0.dds";
const VFX: &str = "assets/characters/test/particles.bin";

#[test]
fn test_compress_with_none() {
    let data = b"Hello, world!";
    let result = compress_with(data, OverrideCodec::Stored).unwrap();
    assert_eq!(result, data);
}

#[test]
fn test_compress_with_zstd() {
    let data = b"Hello, world!".repeat(100);
    let compressed = compress_with(&data, OverrideCodec::Zstd).unwrap();
    assert!(compressed.len() < data.len());
}

/// The header size `ltk_wad` reserves is the one this crate's writer emits.
///
/// `ltk_wad` pins the same property against its own builder; this pins it
/// against ours, which is the half that can drift. A patched WAD whose TOC did
/// not land where [`WadTailLayout::validate`] expects would be rebased by
/// seeking into Riot's signature.
#[test]
fn a_built_wad_puts_its_toc_exactly_past_the_header() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    let source_path = root.join("Game").join(WAD_REL);
    write_game_wad(&source_path, &[(SKIN, b"the original skin")]);

    let stats = build_patched_wad(
        &source_path,
        &root.join("overlay").join(WAD_REL),
        &HashSet::new(),
        |_| unreachable!("this build has no overrides"),
    )
    .expect("the overlay WAD builds");

    assert_eq!(
        stats.layout.chunk_count_offset().unwrap(),
        268,
        "the chunk count follows the header"
    );
    assert_eq!(
        stats.layout.toc_offset().unwrap(),
        272,
        "the TOC follows the chunk count"
    );
}

/// A pass-through copies its container's bytes verbatim but never its
/// claimed checksum: the value the overlay TOC carries is always recomputed
/// over the bytes actually written, because the client kills the process
/// when a chunk's checksum disagrees with them (ADR-0001).
#[test]
fn a_pass_through_recomputes_the_checksum_over_its_own_bytes() {
    const CONTENT: &[u8] = b"a chunk its container already holds compressed";
    let compressed = zstd::encode_all(CONTENT, 3).expect("test content compresses");

    let over = EncodedChunk::pass_through(
        hash(VFX),
        CompressedChunk {
            compressed: compressed.clone(),
            compression: WadChunkCompression::Zstd,
            uncompressed_size: CONTENT.len(),
            // What a container shipping wrong metadata claims.
            claimed_checksum: 0xDEAD_BEEF,
        },
    )
    .expect("a zstd chunk's sizes fit")
    .expect("zstd is a codec this crate emits");

    assert_eq!(
        over.compressed(),
        compressed,
        "the stored bytes must be copied verbatim, never re-encoded"
    );
    assert_eq!(
        over.checksum(),
        xxh3_64(&compressed),
        "the TOC checksum must be computed over the bytes being written"
    );
    assert_eq!(over.compression(), WadChunkCompression::Zstd);
    assert_eq!(over.uncompressed_size() as usize, CONTENT.len());
}

/// The client sizes a type-0 chunk's buffer from its uncompressed size and
/// then reads its compressed size into it, so a stored chunk whose two TOC
/// sizes disagree makes it read past that buffer. A container claiming a
/// size its own bytes do not have must not be able to put that claim in a
/// WAD the game loads.
#[test]
fn a_passed_through_stored_chunk_reports_equal_sizes() {
    const STORED: &[u8] = b"a wwise bank its container holds uncompressed";

    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let source_path = root.join("Game").join(WAD_REL);
    write_game_wad(&source_path, &[(SKIN, b"the original skin")]);

    let over = EncodedChunk::pass_through(
        hash(SKIN),
        CompressedChunk {
            compressed: STORED.to_vec(),
            compression: WadChunkCompression::None,
            uncompressed_size: 4096,
            claimed_checksum: xxh3_64(STORED),
        },
    )
    .expect("a stored chunk's sizes fit")
    .expect("stored is a codec this crate emits");

    let overlay_path = root.join("overlay").join(WAD_REL);
    build_patched_wad(
        &source_path,
        &overlay_path,
        &[hash(SKIN)].into_iter().collect(),
        |_| Ok(over.clone()),
    )
    .expect("the overlay WAD builds");

    let file = File::open(overlay_path.as_std_path()).unwrap();
    let wad = Wad::mount(std::io::BufReader::new(file)).unwrap();
    let chunk = *wad
        .chunks()
        .get(hash(SKIN))
        .expect("the passed-through override is in the TOC");

    assert_eq!(chunk.compression_type, WadChunkCompression::None);
    assert_eq!(chunk.compressed_size, STORED.len());
    assert_eq!(
        chunk.uncompressed_size, chunk.compressed_size,
        "a stored chunk's TOC sizes must be equal, whatever its source claimed"
    );
    assert_eq!(
        chunk.checksum,
        xxh3_64(STORED),
        "the TOC checksum must match the bytes the file holds"
    );
}

/// A `ZstdMulti` chunk's bytes mean nothing without the subchunk table in
/// its own WAD, and GZip and Satellite are codecs this crate never writes.
/// Refusing them here is what sends those chunks down the decode-and-
/// compress path instead of into an overlay the game cannot read.
#[test]
fn a_codec_this_crate_does_not_emit_refuses_to_pass_through() {
    for compression in [
        WadChunkCompression::GZip,
        WadChunkCompression::Satellite,
        WadChunkCompression::ZstdMulti,
    ] {
        let refused = EncodedChunk::pass_through(
            hash(VFX),
            CompressedChunk {
                compressed: b"stored under a codec we do not write".to_vec(),
                compression,
                uncompressed_size: 128,
                claimed_checksum: 0,
            },
        )
        .expect("refusing a codec is not an error");

        assert!(
            refused.is_none(),
            "{compression} must fall back to decode-and-compress"
        );
    }
}

/// Audio is already compressed, so it is stored; everything else is Zstd.
/// Codecs the format names but this crate never emits collapse onto Zstd
/// rather than reaching the writer, which is what keeps it total.
#[test]
fn the_codec_for_data_is_one_of_the_two_this_crate_writes() {
    // A Wwise bank header - the audio case.
    let mut audio = b"BKHD".to_vec();
    audio.extend_from_slice(&[0u8; 64]);

    assert_eq!(OverrideCodec::for_data(&audio), OverrideCodec::Stored);
    assert_eq!(
        OverrideCodec::for_data(b"an ordinary asset"),
        OverrideCodec::Zstd
    );
}
