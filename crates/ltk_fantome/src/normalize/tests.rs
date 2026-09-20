use super::*;

use std::io::{Cursor, Read, Write};

use camino::{Utf8Path, Utf8PathBuf};
use tempfile::TempDir;
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

const PACKED_WAD_ENTRY: &str = "WAD/Aatrox.wad.client";
const PAYLOAD: &[u8] = b"packed content";
/// A CRC32 that matches no bytes anyone could write.
const WRONG_CRC: u32 = 0xDEAD_BEEF;

/// A packed WAD holding one stored chunk - the shape a mod's `WAD/` entry has
/// when its author packed it rather than shipping loose files.
fn packed_wad_bytes(payload: &[u8]) -> Vec<u8> {
    use ltk_wad::{WadBuilder, WadChunkBuilder, WadChunkCompression};

    let payload = payload.to_vec();
    let mut cursor = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(
            WadChunkBuilder::default()
                .with_path("packed/file.bin")
                .with_force_compression(WadChunkCompression::None),
        )
        .build_to_writer(&mut cursor, move |_hash, writer| {
            writer.write_all(&payload)?;
            Ok(())
        })
        .unwrap();
    cursor.into_inner()
}

/// An archive holding metadata, a loose WAD file and a packed WAD held under
/// `wads`. Everything but the packed WAD is deflated, which is how the Fantome
/// tools in the wild write all four.
fn archive(wads: CompressionMethod) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("META/info.json", deflated).unwrap();
    zip.write_all(br#"{"Name":"Mod","Author":"A","Version":"1","Description":"d"}"#)
        .unwrap();
    zip.start_file("WAD/Ahri.wad.client/data/loose.bin", deflated)
        .unwrap();
    zip.write_all(b"a loose file, small and worth deflating")
        .unwrap();
    zip.start_file(PACKED_WAD_ENTRY, deflated.compression_method(wads))
        .unwrap();
    zip.write_all(&packed_wad_bytes(PAYLOAD)).unwrap();

    zip.finish().unwrap().into_inner()
}

/// Overwrite every CRC32 in the archive with a value that matches nothing,
/// which is what the Fantome tools this crate reads for actually write.
///
/// The header scan is blind, so the counts are asserted against the entries the
/// caller wrote: a signature that happened to match inside deflated data would
/// aim the patch at content bytes and quietly turn the test into another one.
fn with_corrupt_crcs(mut bytes: Vec<u8>, entries: usize) -> Vec<u8> {
    const LOCAL_HEADER: u32 = 0x0403_4b50;
    const CENTRAL_HEADER: u32 = 0x0201_4b50;

    let (mut local, mut central) = (0, 0);
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        // The CRC32 field sits at +14 in a local header, +16 in a central one.
        let at = match u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) {
            LOCAL_HEADER => {
                local += 1;
                i + 14
            }
            CENTRAL_HEADER => {
                central += 1;
                i + 16
            }
            _ => {
                i += 1;
                continue;
            }
        };
        bytes[at..at + 4].copy_from_slice(&WRONG_CRC.to_le_bytes());
        i += 4;
    }

    assert_eq!(local, entries, "one local header per entry");
    assert_eq!(central, entries, "one central header per entry");
    bytes
}

/// The archive's entry names, in the order it holds them.
fn entry_names(archive: &[u8]) -> Vec<String> {
    zip::ZipArchive::new(Cursor::new(archive.to_vec()))
        .unwrap()
        .file_names()
        .map(str::to_owned)
        .collect()
}

/// The whole point of a normalize: a packed WAD a tool deflated comes out
/// stored, so a reader can seek to its bytes where they lie instead of
/// inflating the archive into memory to reach them.
#[test]
fn a_deflated_packed_wad_comes_out_stored() {
    let mut reader = FantomeReader::new(Cursor::new(archive(CompressionMethod::Deflated))).unwrap();
    let mut sink = Cursor::new(Vec::new());

    let outcome = store_packed_wads(&mut reader, &mut sink).unwrap();

    assert_eq!(outcome, NormalizeOutcome::Normalized { wads_stored: 1 });

    let mut out = zip::ZipArchive::new(Cursor::new(sink.into_inner())).unwrap();
    let mut entry = out.by_name(PACKED_WAD_ENTRY).unwrap();
    assert_eq!(entry.compression(), CompressionMethod::Stored);

    let mut stored = Vec::new();
    entry.read_to_end(&mut stored).unwrap();
    assert_eq!(
        stored,
        packed_wad_bytes(PAYLOAD),
        "the WAD's bytes must survive the change of container encoding"
    );
}

/// An archive that needs nothing is recognised from its entry table alone and
/// left where it is, so importing the same mod twice does not rewrite it the
/// second time - and so a caller holding a temp file knows not to keep it.
#[test]
fn an_archive_whose_wads_are_stored_is_left_alone() {
    let mut reader = FantomeReader::new(Cursor::new(archive(CompressionMethod::Stored))).unwrap();
    let mut sink = Cursor::new(Vec::new());

    let outcome = store_packed_wads(&mut reader, &mut sink).unwrap();

    assert_eq!(outcome, NormalizeOutcome::Unchanged);
    assert!(
        sink.into_inner().is_empty(),
        "an archive with nothing to normalize must not be rewritten"
    );
}

/// Only the packed WADs change container. The metadata, the loose files and
/// the tables are read whole or not at all, so deflating them costs a reader
/// nothing and saves the user disk - and they keep the CRC32 their author
/// wrote, wrong or not, exactly as the hashtable rewrite leaves them.
#[test]
fn every_other_entry_is_carried_through_untouched() {
    let source = with_corrupt_crcs(archive(CompressionMethod::Deflated), 3);
    let mut reader = FantomeReader::new(Cursor::new(source.clone())).unwrap();
    let mut sink = Cursor::new(Vec::new());

    store_packed_wads(&mut reader, &mut sink).unwrap();

    let out = sink.into_inner();
    let mut before = entry_names(&source);
    let mut after = entry_names(&out);
    before.sort();
    after.sort();
    assert_eq!(before, after, "the archive must hold the same entries");

    let mut normalized = zip::ZipArchive::new(Cursor::new(out.clone())).unwrap();
    for name in ["META/info.json", "WAD/Ahri.wad.client/data/loose.bin"] {
        let entry = normalized.by_name(name).unwrap();
        assert_eq!(entry.compression(), CompressionMethod::Deflated, "{name}");
        assert_eq!(entry.crc32(), WRONG_CRC, "{name}");
    }

    // The content survived, which the reader proves by bypassing the CRC the
    // way it does for every archive.
    let mut out_reader = FantomeReader::new(Cursor::new(out)).unwrap();
    assert_eq!(out_reader.read_info().unwrap().name, "Mod");
}

/// The one checksum a normalize corrects: an entry a reader will now seek into
/// declares a CRC32 computed over the bytes written beside it. The archive
/// gains a true checksum exactly where it gained a shape worth trusting.
#[test]
fn a_stored_wad_declares_a_checksum_that_matches_its_bytes() {
    let source = with_corrupt_crcs(archive(CompressionMethod::Deflated), 3);
    let mut reader = FantomeReader::new(Cursor::new(source)).unwrap();
    let mut sink = Cursor::new(Vec::new());

    store_packed_wads(&mut reader, &mut sink).unwrap();

    let mut normalized = zip::ZipArchive::new(Cursor::new(sink.into_inner())).unwrap();
    let mut entry = normalized.by_name(PACKED_WAD_ENTRY).unwrap();
    assert_ne!(entry.crc32(), WRONG_CRC);

    // Reading through to EOF is where the zip crate checks the CRC32 - the
    // check every other read in this crate has to duck.
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .expect("a normalized WAD's declared CRC32 must match its stored bytes");
    assert_eq!(bytes, packed_wad_bytes(PAYLOAD));
}

/// The temp directory's path, which the archive functions take as UTF-8.
fn utf8_dir(dir: &TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap()
}

/// How a packed WAD is held in the archive on disk at `path`.
fn compression_at(path: &Utf8Path) -> CompressionMethod {
    let bytes = std::fs::read(path.as_std_path()).unwrap();
    zip::ZipArchive::new(Cursor::new(bytes))
        .unwrap()
        .by_name(PACKED_WAD_ENTRY)
        .unwrap()
        .compression()
}

/// The import-shaped normalize reads the source and writes the result at
/// `dest`. The source is a file the importer was handed, so it is never
/// written to; `dest` is a copy the importer owns.
#[test]
fn a_normalize_writes_its_result_at_dest_and_leaves_the_source_alone() {
    let dir = tempfile::tempdir().unwrap();
    let dir = utf8_dir(&dir);
    let source = dir.join("downloaded.fantome");
    // A directory the importer has not created yet, as a library folder on a
    // first import is.
    let dest = dir.join("library/mod.fantome");
    let bytes = archive(CompressionMethod::Deflated);
    std::fs::write(source.as_std_path(), &bytes).unwrap();

    let outcome = normalize_archive(&source, &dest).unwrap();

    assert_eq!(outcome, NormalizeOutcome::Normalized { wads_stored: 1 });
    assert_eq!(
        std::fs::read(source.as_std_path()).unwrap(),
        bytes,
        "the archive the importer was handed must come out of a normalize untouched"
    );
    assert_eq!(compression_at(&dest), CompressionMethod::Stored);
}

/// An archive that needed nothing still has to arrive: an importer looks for
/// the mod at `dest` without knowing which outcome the normalize reported, so
/// deciding not to rewrite must not decide not to deliver.
#[test]
fn an_archive_that_needs_nothing_still_lands_at_dest() {
    let dir = tempfile::tempdir().unwrap();
    let dir = utf8_dir(&dir);
    let source = dir.join("downloaded.fantome");
    let dest = dir.join("library/mod.fantome");
    let bytes = archive(CompressionMethod::Stored);
    std::fs::write(source.as_std_path(), &bytes).unwrap();

    let outcome = normalize_archive(&source, &dest).unwrap();

    assert_eq!(outcome, NormalizeOutcome::Unchanged);
    assert_eq!(std::fs::read(dest.as_std_path()).unwrap(), bytes);
}

/// The importer's own call: a copy it has already placed in the library, made
/// seekable where it lies. The archive keeps its path, so nothing that recorded
/// it has to be told, and the second run has nothing left to do.
#[test]
fn normalizing_in_place_replaces_the_archive_once() {
    let dir = tempfile::tempdir().unwrap();
    let library = utf8_dir(&dir).join("library");
    std::fs::create_dir_all(library.as_std_path()).unwrap();
    let mod_path = library.join("mod.fantome");
    std::fs::write(mod_path.as_std_path(), archive(CompressionMethod::Deflated)).unwrap();

    let outcome = normalize_archive(&mod_path, &mod_path).unwrap();

    assert_eq!(outcome, NormalizeOutcome::Normalized { wads_stored: 1 });
    assert_eq!(compression_at(&mod_path), CompressionMethod::Stored);

    let normalized = std::fs::read(mod_path.as_std_path()).unwrap();
    assert_eq!(
        normalize_archive(&mod_path, &mod_path).unwrap(),
        NormalizeOutcome::Unchanged
    );
    assert_eq!(std::fs::read(mod_path.as_std_path()).unwrap(), normalized);

    // Neither run may leave a temporary file behind for the importer to find
    // when it next lists the library.
    let left = std::fs::read_dir(library.as_std_path()).unwrap().count();
    assert_eq!(left, 1, "the library must hold the mod and nothing else");
}

/// The packed WADs end up last, whatever order the source held them in.
///
/// A WAD that is one entry at the end of the archive can later be grown in
/// place, with only the central directory behind it to move. A normalize is the
/// point where an archive takes the shape this crate wants, so it is where the
/// reordering costs nothing extra.
#[test]
fn the_packed_wads_come_last() {
    // A WAD in the middle, which is where a normalize has to move it from.
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("META/info.json", deflated).unwrap();
    zip.write_all(br#"{"Name":"Mod","Author":"A","Version":"1","Description":"d"}"#)
        .unwrap();
    zip.start_file(PACKED_WAD_ENTRY, deflated).unwrap();
    zip.write_all(&packed_wad_bytes(PAYLOAD)).unwrap();
    zip.start_file("RAW/assets/note.txt", deflated).unwrap();
    zip.write_all(b"a loose file the source holds after the WAD")
        .unwrap();
    let source = zip.finish().unwrap().into_inner();

    let mut reader = FantomeReader::new(Cursor::new(source)).unwrap();
    let mut sink = Cursor::new(Vec::new());
    store_packed_wads(&mut reader, &mut sink).unwrap();

    assert_eq!(
        entry_names(&sink.into_inner()),
        ["META/info.json", "RAW/assets/note.txt", PACKED_WAD_ENTRY],
        "the packed WAD must be the last entry, everything else in source order"
    );
}

/// An archive whose WADs are stored but not last is still left alone.
///
/// Reordering on its own is not worth rewriting every archive already in the
/// field for: such an archive is valid, and merely misses the fast path a
/// trailing WAD would later allow.
#[test]
fn an_archive_already_stored_is_not_rewritten_just_to_reorder() {
    let mut reader = FantomeReader::new(Cursor::new(archive(CompressionMethod::Stored))).unwrap();
    let mut sink = Cursor::new(Vec::new());

    let outcome = store_packed_wads(&mut reader, &mut sink).unwrap();

    assert_eq!(outcome, NormalizeOutcome::Unchanged);
    assert!(sink.into_inner().is_empty());
}
