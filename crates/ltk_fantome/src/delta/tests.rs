//! The invariants a repaired WAD has to hold, pinned one test each.
//!
//! The client kills the process over a WAD that breaks any of them, so they are
//! stated where this codebase already states them - `docs/adr/0001-*`,
//! [`EncodedChunk`]'s own docs, and the `wad-shared-chunk-invariant` note - and
//! asserted here rather than inferred by diffing against the repack path, whose
//! output was never the target.

use super::*;

use std::io::Cursor;

use camino::Utf8PathBuf;
use ltk_hash::Hash as _;
use ltk_wad::{WadBuilder, WadChunkBuilder, WadChunkCompression, chunk_hash_of};
use tempfile::TempDir;
use xxhash_rust::xxh3::xxh3_64;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const WAD_ENTRY: &str = "WAD/Aatrox.wad.client";
const WAD_NAME: &str = "Aatrox.wad.client";
/// A CRC32 that matches no bytes anyone could write, as tools in the wild emit.
const WRONG_CRC: u32 = 0xDEAD_BEEF;

/// The chunk paths every fixture WAD holds, in the order they are added.
const CHUNK_PATHS: [&str; 3] = ["data/one.bin", "data/two.bin", "assets/three.bin"];

fn hash_of(path: &str) -> WadHash {
    WadHash::hash_str(path)
}

/// A packed WAD holding [`CHUNK_PATHS`], each under `codec`.
fn packed_wad(bodies: &[(&str, Vec<u8>)], codec: WadChunkCompression) -> Vec<u8> {
    let bodies: BTreeMap<WadHash, Vec<u8>> = bodies
        .iter()
        .map(|(path, body)| (hash_of(path), body.clone()))
        .collect();

    let mut builder = WadBuilder::default();
    for path in CHUNK_PATHS {
        builder = builder.with_chunk(
            WadChunkBuilder::default()
                .with_path(path)
                .with_force_compression(codec),
        );
    }

    let mut cursor = Cursor::new(Vec::new());
    builder
        .build_to_writer(&mut cursor, move |hash, writer| {
            writer.write_all(&bodies[&hash])?;
            Ok(())
        })
        .unwrap();
    cursor.into_inner()
}

/// The three chunk bodies a fixture WAD starts with.
fn original_bodies() -> Vec<(&'static str, Vec<u8>)> {
    CHUNK_PATHS
        .iter()
        .map(|path| (*path, format!("the original body of {path}").into_bytes()))
        .collect()
}

/// A normalized archive: metadata and a table deflated, the packed WAD stored.
///
/// The packed WAD is written *first*, which is the order a normalize preserves
/// and the order that proves a replace moves it last.
fn archive(wad: &[u8]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file(
        WAD_ENTRY,
        deflated.compression_method(CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(wad).unwrap();
    zip.start_file("META/info.json", deflated).unwrap();
    zip.write_all(br#"{"Name":"Mod","Author":"A","Version":"1","Description":"d"}"#)
        .unwrap();
    zip.start_file("RAW/assets/note.txt", deflated).unwrap();
    zip.write_all(b"a loose file that lives outside any WAD directory")
        .unwrap();

    with_wrong_crcs(zip.finish().unwrap().into_inner())
}

/// Overwrite every deflated entry's CRC32 with a value matching nothing, the
/// way the Fantome tools this crate reads for actually write them.
///
/// The stored WAD entry's own CRC is left alone: it is the one entry a reader
/// seeks into, and a normalize is what makes it true.
fn with_wrong_crcs(bytes: Vec<u8>) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
    let mut targets = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).unwrap();
        if entry.compression() == CompressionMethod::Deflated {
            targets.push(entry.crc32().to_le_bytes());
        }
    }

    let mut bytes = bytes;
    for crc in targets {
        let mut at = 0;
        while at + 4 <= bytes.len() {
            if bytes[at..at + 4] == crc {
                bytes[at..at + 4].copy_from_slice(&WRONG_CRC.to_le_bytes());
            }
            at += 1;
        }
    }
    bytes
}

/// A directory holding `archive.fantome`, plus that path.
fn staged(bytes: &[u8]) -> (TempDir, Utf8PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("archive.fantome")).unwrap();
    std::fs::write(path.as_std_path(), bytes).unwrap();
    (dir, path)
}

/// Replace `path`'s chunk with `bytes` and hand back the rewritten archive.
fn replace_one(archive_bytes: &[u8], path: &str, bytes: &[u8]) -> (TempDir, Utf8PathBuf) {
    let (dir, source) = staged(archive_bytes);
    let mut delta = ArchiveDelta::new();
    delta.chunk(WAD_NAME, hash_of(path), bytes);
    apply_delta(&source, &source, &delta, None).unwrap();
    (dir, source)
}

/// The rewritten archive's WAD, mounted the way a health check mounts one.
fn mount(path: &Utf8PathBuf) -> Wad<Cursor<Vec<u8>>> {
    let mut reader = FantomeReader::new(Cursor::new(std::fs::read(path.as_std_path()).unwrap()))
        .expect("the rewritten archive must read back");
    let bytes = reader
        .read_packed_wad(WAD_NAME)
        .unwrap()
        .expect("a packed WAD");
    Wad::mount(Cursor::new(bytes)).expect("the rebased WAD must mount")
}

// -- crash rules ------------------------------------------------------------

/// Rule 1: a chunk's TOC checksum is xxh3_64 of the bytes it points at.
///
/// The client kills the process over a chunk whose checksum disagrees with its
/// content (`docs/adr/0001-*`), and a chunk shared across WADs is validated by
/// exactly this value.
#[test]
fn every_chunk_checksum_matches_the_bytes_it_points_at() {
    let (_dir, path) = replace_one(
        &archive(&packed_wad(&original_bodies(), WadChunkCompression::Zstd)),
        "data/two.bin",
        b"a repaired body, long enough that zstd has something to chew on",
    );

    let mut wad = mount(&path);
    for chunk in wad.chunks().as_slice().to_vec() {
        let raw = wad.load_chunk_raw(&chunk).unwrap();
        assert_eq!(
            xxh3_64(&raw),
            chunk.checksum,
            "chunk {:016x} carries a checksum its bytes do not produce",
            chunk.path_hash
        );
    }
}

/// Rule 2: `uncompressed_size` is what the bytes decode to.
///
/// The client allocates the chunk's buffer from this field, so a TOC that
/// overstates it makes the client read past what it allocated.
#[test]
fn every_chunk_decodes_to_the_size_its_entry_declares() {
    const REPAIRED: &[u8] = b"a repaired body of its own length";

    let (_dir, path) = replace_one(
        &archive(&packed_wad(&original_bodies(), WadChunkCompression::Zstd)),
        "data/two.bin",
        REPAIRED,
    );

    let mut wad = mount(&path);
    for chunk in wad.chunks().as_slice().to_vec() {
        let decoded = wad.load_chunk_decompressed(&chunk).unwrap();
        assert_eq!(decoded.len(), chunk.uncompressed_size, "{chunk:?}");
    }
    let repaired = *wad.chunks().get(hash_of("data/two.bin")).unwrap();
    assert_eq!(&*wad.load_chunk_decompressed(&repaired).unwrap(), REPAIRED);
}

/// Rule 3: the TOC ascends by path hash, counts honestly, and points inside the
/// file.
#[test]
fn the_toc_ascends_counts_honestly_and_stays_inside_the_wad() {
    let (_dir, path) = replace_one(
        &archive(&packed_wad(&original_bodies(), WadChunkCompression::Zstd)),
        "assets/three.bin",
        b"a repaired body",
    );

    let mut reader =
        FantomeReader::new(Cursor::new(std::fs::read(path.as_std_path()).unwrap())).unwrap();
    let bytes = reader.read_packed_wad(WAD_NAME).unwrap().unwrap();
    let wad = Wad::mount(Cursor::new(bytes.clone())).unwrap();
    let chunks = wad.chunks().as_slice();

    assert_eq!(chunks.len(), CHUNK_PATHS.len(), "the entry count changed");
    assert!(
        chunks
            .windows(2)
            .all(|pair| pair[0].path_hash < pair[1].path_hash),
        "the TOC is not ascending by path hash"
    );
    for chunk in chunks {
        assert!(
            chunk.data_offset + chunk.compressed_size <= bytes.len(),
            "chunk {:016x} points past the end of the WAD",
            chunk.path_hash
        );
    }
}

/// Rule 4: a subchunked body cannot be rebased, so replacing one is refused.
///
/// Its subchunk records live in the archive rather than in the chunk, and a
/// rebase writes one run of bytes with those fields zeroed - an entry the game
/// cannot resolve. Refused before anything is written, not written wrong.
#[test]
fn replacing_a_subchunked_body_is_refused_and_writes_nothing() {
    let wad = subchunked_wad();
    let (dir, source) = staged(&archive(&wad));
    let dest = Utf8PathBuf::from_path_buf(dir.path().join("out.fantome")).unwrap();

    let mut delta = ArchiveDelta::new();
    delta.chunk(
        WAD_NAME,
        hash_of(CHUNK_PATHS[0]),
        b"a repaired body".as_slice(),
    );
    let error = apply_delta(&source, &dest, &delta, None).unwrap_err();

    assert!(
        matches!(&error, FantomeDeltaError::SubchunkedChunk { path_hash, .. }
            if *path_hash == hash_of(CHUNK_PATHS[0])),
        "expected a subchunked refusal, got {error:?}"
    );
    assert!(!dest.exists(), "a refusal wrote a destination archive");
}

/// A subchunked chunk is not one the builder emits, so its TOC entry is stamped
/// over a built WAD's: the byte holding the codec and the frame count.
fn subchunked_wad() -> Vec<u8> {
    let mut wad = packed_wad(&original_bodies(), WadChunkCompression::Zstd);
    let mounted = Wad::mount(Cursor::new(wad.clone())).unwrap();
    let index = mounted
        .chunks()
        .as_slice()
        .iter()
        .position(|chunk| chunk.path_hash == hash_of(CHUNK_PATHS[0]))
        .unwrap();

    // 268 header + 4 count, then the entry's 21st byte: 8 of path hash, 4 each
    // of offset and the two sizes, then `frame_count << 4 | codec`.
    let at = 268 + 4 + index * 32 + 20;
    wad[at] = (2 << 4) | (WadChunkCompression::ZstdMulti as u8);
    wad
}

/// Rule 5: a chunk shared across WADs stays byte-identical in all of them.
///
/// League validates such a chunk by its compressed checksum, so one repair
/// landing different bytes in two WADs is a crash. The same encoding goes into
/// both because both are encoded from the same replacement bytes.
#[test]
fn one_repair_lands_byte_identical_in_every_wad_that_shares_the_chunk() {
    const SHARED: &str = "data/two.bin";
    const REPAIRED: &[u8] = b"the one body both WADs must end up holding";

    let wad = packed_wad(&original_bodies(), WadChunkCompression::Zstd);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for entry in ["WAD/Aatrox.wad.client", "WAD/Ahri.wad.client"] {
        zip.start_file(entry, stored).unwrap();
        zip.write_all(&wad).unwrap();
    }
    zip.start_file(
        "META/info.json",
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )
    .unwrap();
    zip.write_all(br#"{"Name":"Mod","Author":"A","Version":"1","Description":"d"}"#)
        .unwrap();
    let (_dir, source) = staged(&zip.finish().unwrap().into_inner());

    let mut delta = ArchiveDelta::new();
    delta.chunk("Aatrox.wad.client", hash_of(SHARED), REPAIRED);
    delta.chunk("Ahri.wad.client", hash_of(SHARED), REPAIRED);
    apply_delta(&source, &source, &delta, None).unwrap();

    let mut reader =
        FantomeReader::new(Cursor::new(std::fs::read(source.as_std_path()).unwrap())).unwrap();
    let mut bodies = Vec::new();
    for name in ["Aatrox.wad.client", "Ahri.wad.client"] {
        let bytes = reader.read_packed_wad(name).unwrap().unwrap();
        let mut wad = Wad::mount(Cursor::new(bytes)).unwrap();
        let chunk = *wad.chunks().get(hash_of(SHARED)).unwrap();
        bodies.push((chunk.checksum, wad.load_chunk_raw(&chunk).unwrap()));
    }

    assert_eq!(
        bodies[0].0, bodies[1].0,
        "the shared chunk's checksums differ"
    );
    assert_eq!(bodies[0].1, bodies[1].1, "the shared chunk's bytes differ");
}

/// Rule 5 again, where the two WADs disagree about how they stored the chunk.
///
/// The invariant is about the bytes a repair *writes*, not about what it found:
/// one WAD holding hash H uncompressed while another holds it Zstd is a valid
/// archive, and a repair naming H in both still has to land one encoding in
/// both. Choosing the codec from the replacement's own content is what makes
/// that hold, since the content is the same by construction.
#[test]
fn a_shared_chunk_is_encoded_alike_even_where_the_wads_disagree() {
    const SHARED: &str = "data/two.bin";
    const REPAIRED: &[u8] = b"PROP the one body both WADs must end up holding";

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (entry, codec) in [
        ("WAD/Aatrox.wad.client", WadChunkCompression::None),
        ("WAD/Ahri.wad.client", WadChunkCompression::Zstd),
    ] {
        zip.start_file(entry, stored).unwrap();
        zip.write_all(&packed_wad(&original_bodies(), codec))
            .unwrap();
    }
    zip.start_file(
        "META/info.json",
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )
    .unwrap();
    zip.write_all(br#"{"Name":"Mod","Author":"A","Version":"1","Description":"d"}"#)
        .unwrap();
    let (_dir, source) = staged(&zip.finish().unwrap().into_inner());

    let mut delta = ArchiveDelta::new();
    delta.chunk("Aatrox.wad.client", hash_of(SHARED), REPAIRED);
    delta.chunk("Ahri.wad.client", hash_of(SHARED), REPAIRED);
    apply_delta(&source, &source, &delta, None).unwrap();

    let mut reader =
        FantomeReader::new(Cursor::new(std::fs::read(source.as_std_path()).unwrap())).unwrap();
    let mut written = Vec::new();
    for name in ["Aatrox.wad.client", "Ahri.wad.client"] {
        let bytes = reader.read_packed_wad(name).unwrap().unwrap();
        let mut wad = Wad::mount(Cursor::new(bytes)).unwrap();
        let chunk = *wad.chunks().get(hash_of(SHARED)).unwrap();
        written.push((
            chunk.compression_type,
            chunk.checksum,
            wad.load_chunk_raw(&chunk).unwrap(),
        ));
    }

    assert_eq!(
        written[0].0, written[1].0,
        "the shared chunk was written under two codecs"
    );
    assert_eq!(written[0].1, written[1].1, "its checksums differ");
    assert_eq!(written[0].2, written[1].2, "its stored bytes differ");
}

/// Rule 6: every offset fits the format's 4 GiB `u32` addressing.
///
/// A WAD whose own TOC reaches past it is refused before anything is written,
/// so the caller keeps its untouched archive and its full-repack fallback.
#[test]
fn a_wad_reaching_past_the_formats_offsets_is_refused() {
    let mut wad = packed_wad(&original_bodies(), WadChunkCompression::Zstd);
    let mounted = Wad::mount(Cursor::new(wad.clone())).unwrap();
    let index = mounted
        .chunks()
        .as_slice()
        .iter()
        .position(|chunk| chunk.path_hash == hash_of(CHUNK_PATHS[0]))
        .unwrap();

    // The entry's `compressed_size`, at its 13th byte, claiming a body that
    // ends past what a `u32` offset can address.
    let at = 268 + 4 + index * 32 + 12;
    wad[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());

    let (dir, source) = staged(&archive(&wad));
    let dest = Utf8PathBuf::from_path_buf(dir.path().join("out.fantome")).unwrap();

    let mut delta = ArchiveDelta::new();
    delta.chunk(WAD_NAME, hash_of(CHUNK_PATHS[1]), b"repaired".as_slice());
    let error = apply_delta(&source, &dest, &delta, None).unwrap_err();

    assert!(
        matches!(&error, FantomeDeltaError::Rebase { wad, .. } if wad == WAD_NAME),
        "expected a rebase refusal, got {error:?}"
    );
    assert!(!dest.exists(), "a refusal wrote a destination archive");
}

/// Rule 7: an unchanged chunk's TOC entry comes back byte-identical.
///
/// `offset_delta` is zero, so the shift is the identity and every field of an
/// untouched entry - offsets, sizes, codec, frame fields, checksum - is the
/// source's own.
#[test]
fn an_untouched_chunks_toc_entry_is_the_sources_own() {
    let source_wad = packed_wad(&original_bodies(), WadChunkCompression::Zstd);
    let before = Wad::mount(Cursor::new(source_wad.clone())).unwrap();
    let before: BTreeMap<WadHash, WadChunk> = before
        .chunks()
        .iter()
        .map(|chunk| (chunk.path_hash, *chunk))
        .collect();

    let (_dir, path) = replace_one(&archive(&source_wad), "data/two.bin", b"a repaired body");

    let wad = mount(&path);
    for chunk in wad.chunks().iter() {
        if chunk.path_hash == hash_of("data/two.bin") {
            continue;
        }
        assert_eq!(
            *chunk, before[&chunk.path_hash],
            "an untouched chunk's TOC entry changed"
        );
    }
}

// -- ordering, entries and the round trip -----------------------------------

/// The packed WAD ends up last, which is what leaves a later in-place edit only
/// the central directory to move.
#[test]
fn the_packed_wad_is_written_last() {
    let (_dir, path) = replace_one(
        &archive(&packed_wad(&original_bodies(), WadChunkCompression::Zstd)),
        "data/one.bin",
        b"a repaired body",
    );

    let archive = zip::ZipArchive::new(std::fs::File::open(path.as_std_path()).unwrap()).unwrap();
    let names: Vec<&str> = archive.file_names().collect();
    assert_eq!(
        names.last().copied(),
        Some(WAD_ENTRY),
        "the packed WAD is not the last entry: {names:?}"
    );
}

/// The repaired WAD is stored, not deflated: deflating it would undo exactly
/// what [`normalize_archive`](crate::normalize_archive) bought.
#[test]
fn the_repaired_wad_goes_back_in_seekable() {
    let (_dir, path) = replace_one(
        &archive(&packed_wad(&original_bodies(), WadChunkCompression::Zstd)),
        "data/one.bin",
        b"a repaired body",
    );

    let mut reader =
        FantomeReader::new(Cursor::new(std::fs::read(path.as_std_path()).unwrap())).unwrap();
    assert!(
        reader
            .packed_wad_source(WAD_NAME)
            .unwrap()
            .unwrap()
            .is_in_place(),
        "the repaired WAD was not written stored"
    );
}

/// Everything the replace did not name is copied byte for byte, wrong CRC32
/// values included - the same terms the hashtable rewrite copies on.
#[test]
fn untouched_entries_are_raw_copied_with_their_crcs_intact() {
    let (_dir, path) = replace_one(
        &archive(&packed_wad(&original_bodies(), WadChunkCompression::Zstd)),
        "data/one.bin",
        b"a repaired body",
    );

    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(path.as_std_path()).unwrap()).unwrap();
    for name in ["META/info.json", "RAW/assets/note.txt"] {
        let entry = archive.by_name(name).unwrap();
        assert_eq!(entry.compression(), CompressionMethod::Deflated, "{name}");
        assert_eq!(entry.crc32(), WRONG_CRC, "{name}");
    }
}

/// A repair changes hashtables and files outside a `.wad.client` directory too,
/// so chunk-only would silently drop those fixes.
#[test]
fn loose_entries_are_replaced_alongside_chunks() {
    const NOTE: &str = "RAW/assets/note.txt";
    const TABLE: &str = "META/hashes/game.hashes.txt";

    let (_dir, source) = staged(&archive(&packed_wad(
        &original_bodies(),
        WadChunkCompression::Zstd,
    )));

    let mut delta = ArchiveDelta::new();
    delta.chunk(
        WAD_NAME,
        hash_of("data/one.bin"),
        b"a repaired body".as_slice(),
    );
    delta.entry(NOTE, b"the repaired note".as_slice());
    delta.entry(TABLE, b"0123456789abcdef assets/thing.bin\n".as_slice());

    let report = apply_delta(&source, &source, &delta, None).unwrap();
    assert_eq!(
        report,
        DeltaReport {
            wads_rebased: 1,
            chunks_replaced: 1,
            entries_replaced: 2,
        }
    );

    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(source.as_std_path()).unwrap()).unwrap();
    let mut read = |name: &str| {
        let mut entry = archive.by_name(name).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        bytes
    };
    assert_eq!(read(NOTE), b"the repaired note");
    assert_eq!(read(TABLE), b"0123456789abcdef assets/thing.bin\n");
}

/// The round trip: a repaired archive mounts, extracts and re-reads clean
/// through the in-place health check the manager already runs.
#[test]
fn a_repaired_archive_still_mounts_extracts_and_reads_back() {
    const REPAIRED: &[u8] = b"the repaired body of data/two.bin";

    let (dir, path) = replace_one(
        &archive(&packed_wad(&original_bodies(), WadChunkCompression::Zstd)),
        "data/two.bin",
        REPAIRED,
    );

    let mut reader =
        FantomeReader::new(Cursor::new(std::fs::read(path.as_std_path()).unwrap())).unwrap();
    assert_eq!(reader.read_info().unwrap().name, "Mod");
    assert_eq!(reader.wad_names(), vec![WAD_NAME.to_owned()]);

    // The health check's own path: mount in place, read every chunk back.
    let mut wad = reader.mount_packed_wad(WAD_NAME).unwrap().unwrap();
    for chunk in wad.chunks().as_slice().to_vec() {
        let decoded = wad.load_chunk_decompressed(&chunk).unwrap();
        assert_eq!(
            xxh3_64(&wad.load_chunk_raw(&chunk).unwrap()),
            chunk.checksum
        );
        if chunk.path_hash == hash_of("data/two.bin") {
            assert_eq!(&*decoded, REPAIRED);
        }
    }
    drop(wad);

    // And an extraction lands the repaired body on disk under its own name.
    let out = Utf8PathBuf::from_path_buf(dir.path().join("extracted")).unwrap();
    reader
        .extract_wads(&out, crate::WadExtractOptions::new())
        .unwrap();
    let names: Vec<_> = std::fs::read_dir(out.join(WAD_NAME).as_std_path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), CHUNK_PATHS.len(), "{names:?}");
}

// -- refusals and the contract around them ----------------------------------

/// A chunk the WAD does not hold would change the entry count, which the
/// format's zero-slack TOC has no room for.
#[test]
fn a_chunk_the_wad_does_not_hold_is_refused() {
    let (dir, source) = staged(&archive(&packed_wad(
        &original_bodies(),
        WadChunkCompression::Zstd,
    )));
    let dest = Utf8PathBuf::from_path_buf(dir.path().join("out.fantome")).unwrap();

    let mut delta = ArchiveDelta::new();
    delta.chunk(WAD_NAME, hash_of("data/absent.bin"), b"new".as_slice());
    let error = apply_delta(&source, &dest, &delta, None).unwrap_err();

    assert!(
        matches!(&error, FantomeDeltaError::ChunkAbsent { path_hash, .. }
            if *path_hash == hash_of("data/absent.bin")),
        "expected an absent-chunk refusal, got {error:?}"
    );
    assert!(!dest.exists(), "a refusal wrote a destination archive");
}

/// A WAD the archive ships as loose files has no packed bytes to rebase, and
/// saying so is what sends the caller to the entry half of the interface.
#[test]
fn a_wad_the_archive_holds_as_loose_files_is_refused() {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("WAD/Aatrox.wad.client/data/one.bin", deflated)
        .unwrap();
    zip.write_all(b"a loose file").unwrap();
    let (dir, source) = staged(&zip.finish().unwrap().into_inner());
    let dest = Utf8PathBuf::from_path_buf(dir.path().join("out.fantome")).unwrap();

    let mut delta = ArchiveDelta::new();
    delta.chunk(WAD_NAME, hash_of("data/one.bin"), b"repaired".as_slice());
    let error = apply_delta(&source, &dest, &delta, None).unwrap_err();

    assert!(
        matches!(&error, FantomeDeltaError::WadNotPacked { wad } if wad == WAD_NAME),
        "expected a not-packed refusal, got {error:?}"
    );
    assert!(!dest.exists());
}

/// A WAD older than v3.4 has TOC entries of another shape, so rewriting one as
/// v3.4 would move every chunk offset the game reads.
#[test]
fn a_wad_older_than_v3_4_is_refused() {
    let mut wad = packed_wad(&original_bodies(), WadChunkCompression::Zstd);
    wad[3] = 1; // v3.1

    let (dir, source) = staged(&archive(&wad));
    let dest = Utf8PathBuf::from_path_buf(dir.path().join("out.fantome")).unwrap();

    let mut delta = ArchiveDelta::new();
    delta.chunk(WAD_NAME, hash_of(CHUNK_PATHS[0]), b"repaired".as_slice());
    let error = apply_delta(&source, &dest, &delta, None).unwrap_err();

    assert!(
        matches!(
            error,
            FantomeDeltaError::UnsupportedWadVersion {
                major: 3,
                minor: 1,
                ..
            }
        ),
        "expected a version refusal, got {error:?}"
    );
    assert!(!dest.exists());
}

/// Naming one WAD both ways would need one of the two dropped, and neither
/// answer is what the caller asked for.
#[test]
fn a_wad_named_both_whole_and_by_chunk_is_refused() {
    let (dir, source) = staged(&archive(&packed_wad(
        &original_bodies(),
        WadChunkCompression::Zstd,
    )));
    let dest = Utf8PathBuf::from_path_buf(dir.path().join("out.fantome")).unwrap();

    let mut delta = ArchiveDelta::new();
    delta.chunk(WAD_NAME, hash_of(CHUNK_PATHS[0]), b"repaired".as_slice());
    delta.entry(WAD_ENTRY, b"a whole new WAD".as_slice());
    let error = apply_delta(&source, &dest, &delta, None).unwrap_err();

    assert!(
        matches!(&error, FantomeDeltaError::ConflictingWad { wad } if wad == WAD_ENTRY),
        "expected a conflict refusal, got {error:?}"
    );
    assert!(!dest.exists());
}

// -- the shape of the interface ---------------------------------------------

/// Naming nothing still leaves the archive where the caller asked for it, so a
/// caller does not have to know whether it had anything to do first.
#[test]
fn naming_nothing_still_puts_the_archive_at_the_destination() {
    let bytes = archive(&packed_wad(&original_bodies(), WadChunkCompression::Zstd));
    let (dir, source) = staged(&bytes);
    let dest = Utf8PathBuf::from_path_buf(dir.path().join("out.fantome")).unwrap();

    let report = apply_delta(&source, &dest, &ArchiveDelta::new(), None).unwrap();

    assert_eq!(report, DeltaReport::default());
    let mut out = FantomeReader::new(std::fs::File::open(dest.as_std_path()).unwrap()).unwrap();
    assert_eq!(out.read_info().unwrap().name, "Mod");
    assert!(out.read_packed_wad(WAD_NAME).unwrap().is_some());
}

/// The replacement's own content picks its codec: audio stored, because its
/// bytes are already compressed, and everything else Zstd.
///
/// The same policy `ltk_wad`'s builder and the overlay builder apply, so a
/// chunk this repairs and one a repack would have written agree - and, because
/// it reads the content rather than the chunk being replaced, so do two WADs
/// that share the chunk.
#[test]
fn a_replacements_codec_comes_from_its_own_content() {
    // `BKHD` is the Wwise bank magic; `PROP` a property bin's.
    const AUDIO: &[u8] = b"BKHD and then some audio that will not compress";
    const BIN: &[u8] = b"PROP and then some properties that will";

    let source = archive(&packed_wad(&original_bodies(), WadChunkCompression::Zstd));
    let (_dir, path) = replace_one(&source, "data/two.bin", AUDIO);
    let mut wad = mount(&path);
    let chunk = *wad.chunks().get(hash_of("data/two.bin")).unwrap();
    assert_eq!(chunk.compression_type, WadChunkCompression::None);
    assert_eq!(&*wad.load_chunk_raw(&chunk).unwrap(), AUDIO);

    // And the other way round: a source chunk stored raw, replaced with content
    // that does compress, comes out Zstd.
    let source = archive(&packed_wad(&original_bodies(), WadChunkCompression::None));
    let (_dir, path) = replace_one(&source, "data/two.bin", BIN);
    let mut wad = mount(&path);
    let chunk = *wad.chunks().get(hash_of("data/two.bin")).unwrap();
    assert_eq!(chunk.compression_type, WadChunkCompression::Zstd);
    assert_eq!(&*wad.load_chunk_decompressed(&chunk).unwrap(), BIN);
}

/// A replacement too short to carry any magic still encodes.
///
/// `ltk_file` 0.2.11 panics on a buffer of exactly three bytes, and a repaired
/// file may well be that short, so the identification is bounded.
#[test]
fn a_replacement_too_short_to_identify_still_encodes() {
    let source = archive(&packed_wad(&original_bodies(), WadChunkCompression::Zstd));
    for body in [b"one".as_slice(), b"".as_slice()] {
        let (_dir, path) = replace_one(&source, "data/two.bin", body);
        let mut wad = mount(&path);
        let chunk = *wad.chunks().get(hash_of("data/two.bin")).unwrap();
        assert_eq!(&*wad.load_chunk_decompressed(&chunk).unwrap(), body);
    }
}

/// Progress counts the rebases and the entries as one run, so a caller showing
/// a bar sees it fill once and reach its total.
#[test]
fn progress_counts_every_rebase_and_every_entry_once() {
    let (_dir, source) = staged(&archive(&packed_wad(
        &original_bodies(),
        WadChunkCompression::Zstd,
    )));

    let mut steps: Vec<(String, DeltaStep, u32, u32)> = Vec::new();
    let mut delta = ArchiveDelta::new();
    delta.chunk(WAD_NAME, hash_of(CHUNK_PATHS[0]), b"repaired".as_slice());
    apply_delta(
        &source,
        &source,
        &delta,
        Some(&mut |progress: DeltaProgress<'_>| {
            steps.push((
                progress.name.to_owned(),
                progress.step,
                progress.index,
                progress.total,
            ));
        }),
    )
    .unwrap();

    // One rebase plus the archive's three entries.
    assert_eq!(steps.len(), 4, "{steps:?}");
    assert_eq!(steps[0].1, DeltaStep::RebaseWad);
    assert_eq!(steps[0].0, WAD_NAME);
    assert!(
        steps[1..]
            .iter()
            .all(|step| step.1 == DeltaStep::WriteEntry)
    );
    assert!(
        steps
            .iter()
            .enumerate()
            .all(|(at, step)| step.2 == at as u32),
        "the indexes are not a run: {steps:?}"
    );
    assert!(steps.iter().all(|step| step.3 == 4), "{steps:?}");
}

/// Two spellings of one WAD are one WAD, since the archive matches its `WAD/`
/// entries case-insensitively.
#[test]
fn a_wad_named_in_two_casings_is_rebased_once() {
    let (_dir, source) = staged(&archive(&packed_wad(
        &original_bodies(),
        WadChunkCompression::Zstd,
    )));

    let mut delta = ArchiveDelta::new();
    delta.chunk(
        "Aatrox.wad.client",
        hash_of(CHUNK_PATHS[0]),
        b"one".as_slice(),
    );
    delta.chunk(
        "aatrox.WAD.CLIENT",
        hash_of(CHUNK_PATHS[1]),
        b"two".as_slice(),
    );

    let report = apply_delta(&source, &source, &delta, None).unwrap();
    assert_eq!(report.wads_rebased, 1);
    assert_eq!(report.chunks_replaced, 2);
}

/// The hash a caller derives from an extracted file's path is the one that
/// addresses its chunk, whichever of the naming policy's three shapes the file
/// landed under.
#[test]
fn a_chunk_is_addressed_by_the_hash_its_extracted_path_reads_back_as() {
    const REPAIRED: &[u8] = b"the repaired body";

    let (_dir, source) = staged(&archive(&packed_wad(
        &original_bodies(),
        WadChunkCompression::Zstd,
    )));

    // What a caller holds is the path the extraction wrote, `.ltk` suffix and
    // all; `chunk_hash_of` is what turns it back into the chunk's key.
    let mut delta = ArchiveDelta::new();
    delta.chunk(
        WAD_NAME,
        chunk_hash_of(camino::Utf8Path::new("data/two.bin.ltk")),
        REPAIRED,
    );
    apply_delta(&source, &source, &delta, None).unwrap();

    let mut wad = mount(&source);
    let chunk = *wad.chunks().get(hash_of("data/two.bin")).unwrap();
    assert_eq!(&*wad.load_chunk_decompressed(&chunk).unwrap(), REPAIRED);
}

/// A replacement set says how much it holds without printing any of it: a
/// repair of a map mod carries megabytes of file content.
#[test]
fn the_debug_shape_counts_rather_than_dumps() {
    let mut delta = ArchiveDelta::new();
    delta.chunk(WAD_NAME, hash_of(CHUNK_PATHS[0]), b"one".as_slice());
    delta.chunk(WAD_NAME, hash_of(CHUNK_PATHS[1]), b"two".as_slice());
    delta.entry("RAW/x.bin", b"three".as_slice());

    assert_eq!(
        format!("{delta:?}"),
        "ArchiveDelta { wads: 1, chunks: 2, entries: 1 }"
    );
    assert!(!delta.is_empty());
    assert!(ArchiveDelta::new().is_empty());
}
