use super::*;
use crate::NoResolver;
use camino::Utf8PathBuf;
use ltk_wad::WadHash;
use std::io::Write;
use tempfile::{TempDir, tempdir};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// The temp directory's path, which extraction takes as UTF-8.
fn utf8_dir(dir: &TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap()
}

fn create_test_fantome() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    let info = r#"{
            "Name": "Test Mod",
            "Author": "Test Author",
            "Version": "1.0.0",
            "Description": "A test mod"
        }"#;
    zip.write_all(info.as_bytes()).unwrap();

    zip.add_directory("WAD/test.wad.client", options).unwrap();
    zip.start_file("WAD/test.wad.client/assets/test.bin", options)
        .unwrap();
    zip.write_all(b"test content").unwrap();

    zip.start_file("RAW/assets/maps/map11/scene.bin", options)
        .unwrap();
    zip.write_all(b"map data").unwrap();

    zip.finish().unwrap().into_inner()
}

/// Where every local file header and every central directory header starts.
///
/// The scan is blind, so callers assert the counts against the entries they
/// wrote: a signature that happened to match inside compressed data would aim a
/// patch at content bytes and quietly turn the test into a different one.
fn header_offsets(archive: &[u8], entries: usize) -> (Vec<usize>, Vec<usize>) {
    const LOCAL_HEADER: u32 = 0x0403_4b50;
    const CENTRAL_HEADER: u32 = 0x0201_4b50;

    let (mut local, mut central) = (Vec::new(), Vec::new());
    let mut i = 0usize;
    while i + 4 <= archive.len() {
        match u32::from_le_bytes(archive[i..i + 4].try_into().unwrap()) {
            LOCAL_HEADER => local.push(i),
            CENTRAL_HEADER => central.push(i),
            _ => {
                i += 1;
                continue;
            }
        }
        i += 4;
    }

    assert_eq!(local.len(), entries, "one local header per entry");
    assert_eq!(central.len(), entries, "one central header per entry");
    (local, central)
}

/// Write a field that appears at `local_at` in a local header and `central_at`
/// in a central one, across every entry.
fn patch_headers(
    archive: &mut [u8],
    entries: usize,
    (local_at, central_at): (usize, usize),
    write: impl Fn(&mut [u8]),
) {
    let (local, central) = header_offsets(archive, entries);
    for (offsets, at) in [(local, local_at), (central, central_at)] {
        for start in offsets {
            write(&mut archive[start + at..start + at + 4]);
        }
    }
}

/// Overwrite every CRC32 with a value that matches nothing, which is what the
/// Fantome tools this crate reads for actually produce.
fn with_corrupt_crcs(mut archive: Vec<u8>, entries: usize) -> Vec<u8> {
    patch_headers(&mut archive, entries, (14, 16), |field| {
        field.copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    });
    archive
}

/// Claim more uncompressed bytes than the entry stores, which is what an
/// archive cut short looks like from its headers.
fn with_overstated_sizes(mut archive: Vec<u8>, entries: usize, extra: u32) -> Vec<u8> {
    patch_headers(&mut archive, entries, (22, 24), |field| {
        let declared = u32::from_le_bytes(field.try_into().unwrap());
        field.copy_from_slice(&(declared + extra).to_le_bytes());
    });
    archive
}

/// Bad CRC32 values are the norm in archives written by other tools, so every
/// read has to get past them rather than report the archive as broken.
#[test]
fn entries_read_despite_a_bad_checksum() {
    // info.json, the WAD directory entry, one file under it, and one RAW file.
    let archive = with_corrupt_crcs(create_test_fantome(), 4);
    let mut reader = FantomeReader::new(Cursor::new(archive)).unwrap();

    assert_eq!(reader.read_info().unwrap().name, "Test Mod");

    let tmp = tempdir().unwrap();
    let dest = utf8_dir(&tmp);
    reader
        .extract_wads(&dest.join("wads"), &NoResolver)
        .unwrap();
    reader.extract_raw(&dest.join("raw")).unwrap();

    assert_eq!(
        std::fs::read(dest.join("wads/test.wad.client/assets/test.bin")).unwrap(),
        b"test content"
    );
    assert_eq!(
        std::fs::read(dest.join("raw/assets/maps/map11/scene.bin")).unwrap(),
        b"map data"
    );
}

/// A packed WAD is read whole before it is mounted, which is the one read that
/// does not stream to a file.
#[test]
fn packed_wads_unpack_despite_a_bad_checksum() {
    let archive = with_corrupt_crcs(packed_wad_fantome(), 1);
    let mut reader = FantomeReader::new(Cursor::new(archive)).unwrap();

    let tmp = tempdir().unwrap();
    let dest = utf8_dir(&tmp);
    let resolver = FixedResolver("assets/characters/aatrox/skin0.bin");
    reader.extract_wads(&dest, &resolver).unwrap();

    assert_eq!(
        std::fs::read(dest.join("test.wad.client/assets/characters/aatrox/skin0.bin")).unwrap(),
        b"packed content"
    );
}

/// Skipping the checksum is not skipping the length: an entry that holds less
/// than it declares still has to fail, or half an archive would import as a
/// whole one.
#[test]
fn a_short_entry_still_fails() {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    zip.start_file(
        "RAW/assets/data.bin",
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(b"the whole payload").unwrap();
    let archive = with_overstated_sizes(zip.finish().unwrap().into_inner(), 1, 8);

    let mut reader = FantomeReader::new(Cursor::new(archive)).unwrap();
    let tmp = tempdir().unwrap();
    assert!(reader.extract_raw(&utf8_dir(&tmp)).is_err());
}

#[test]
fn read_info_parses_metadata() {
    let mut reader = FantomeReader::new(Cursor::new(create_test_fantome())).unwrap();
    let info = reader.read_info().unwrap();

    assert_eq!(info.name, "Test Mod");
    assert_eq!(info.version, "1.0.0");
}

#[test]
fn extract_wads_preserves_paths_under_dest() {
    let mut reader = FantomeReader::new(Cursor::new(create_test_fantome())).unwrap();

    let tmp = tempdir().unwrap();
    let dest = utf8_dir(&tmp).join("wads");
    reader.extract_wads(&dest, &NoResolver).unwrap();

    assert_eq!(
        std::fs::read(dest.join("test.wad.client/assets/test.bin")).unwrap(),
        b"test content"
    );
}

#[test]
fn extract_raw_preserves_paths_under_dest() {
    let mut reader = FantomeReader::new(Cursor::new(create_test_fantome())).unwrap();

    let tmp = tempdir().unwrap();
    let dest = utf8_dir(&tmp).join("RAW");
    reader.extract_raw(&dest).unwrap();

    assert_eq!(
        std::fs::read(dest.join("assets/maps/map11/scene.bin")).unwrap(),
        b"map data"
    );
}

#[test]
fn extract_matches_the_prefix_case_insensitively() {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("wad/test.wad.client/assets/test.bin", options)
        .unwrap();
    zip.write_all(b"test content").unwrap();
    zip.start_file("raw/assets/maps/map11/scene.bin", options)
        .unwrap();
    zip.write_all(b"map data").unwrap();
    let data = zip.finish().unwrap().into_inner();

    let mut reader = FantomeReader::new(Cursor::new(data)).unwrap();
    let tmp = tempdir().unwrap();
    let dest = utf8_dir(&tmp);
    reader
        .extract_wads(&dest.join("wads"), &NoResolver)
        .unwrap();
    reader.extract_raw(&dest.join("raw")).unwrap();

    assert_eq!(
        std::fs::read(dest.join("wads/test.wad.client/assets/test.bin")).unwrap(),
        b"test content"
    );
    assert_eq!(
        std::fs::read(dest.join("raw/assets/maps/map11/scene.bin")).unwrap(),
        b"map data"
    );
}

#[test]
fn read_license_matches_case_and_extension_variants() {
    for (entry, expected_name) in [
        ("META/LICENSE", "LICENSE"),
        ("META/license.md", "LICENSE.md"),
        ("meta/LICENSE.TXT", "LICENSE.txt"),
    ] {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        zip.start_file(entry, options).unwrap();
        zip.write_all(b"The terms.").unwrap();
        let data = zip.finish().unwrap().into_inner();

        let mut reader = FantomeReader::new(Cursor::new(data)).unwrap();
        let (name, bytes) = reader
            .read_license()
            .unwrap()
            .unwrap_or_else(|| panic!("no license found for entry {entry}"));

        assert_eq!(name, expected_name);
        assert_eq!(bytes, b"The terms.");
    }
}

/// An archive that spells `META/` in lower case spells everything under it that
/// way too, so the readme and the thumbnail match on the same terms the license
/// and `info.json` already do.
#[test]
fn meta_entries_match_case_insensitively() {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();
    zip.start_file("meta/readme.md", options).unwrap();
    zip.write_all(b"How to use it.").unwrap();
    zip.start_file("meta/Image.PNG", options).unwrap();
    zip.write_all(b"png bytes").unwrap();
    let data = zip.finish().unwrap().into_inner();

    let mut reader = FantomeReader::new(Cursor::new(data)).unwrap();
    assert_eq!(reader.read_readme().unwrap().unwrap(), b"How to use it.");
    assert_eq!(reader.read_image_png().unwrap().unwrap(), b"png bytes");
}

#[test]
fn meta_entries_absent_read_as_none() {
    let mut reader = FantomeReader::new(Cursor::new(create_test_fantome())).unwrap();

    assert!(reader.read_readme().unwrap().is_none());
    assert!(reader.read_license().unwrap().is_none());
    assert!(reader.read_image_png().unwrap().is_none());
}

#[test]
fn missing_info_is_a_distinct_error() {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    zip.start_file("WAD/x.bin", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"x").unwrap();
    let data = zip.finish().unwrap().into_inner();

    let mut reader = FantomeReader::new(Cursor::new(data)).unwrap();
    assert!(matches!(
        reader.read_info(),
        Err(FantomeExtractError::MissingMetadata)
    ));
}

/// What the writer produces, the reader must give back.
#[test]
fn writer_reader_round_trip() {
    use crate::writer::FantomeWriter;

    let info = FantomeInfo {
        name: "Round Trip".to_string(),
        author: "Alice".to_string(),
        version: "1.0.0".to_string(),
        description: "".to_string(),
        license: None,
        tags: vec![],
        champions: vec![],
        maps: vec![],
        layers: Default::default(),
    };

    let mut writer = FantomeWriter::new(Cursor::new(Vec::new()));
    writer.write_info(&info).unwrap();
    writer
        .write_wad_entry("Test.wad.client", "data\\skin.bin", &mut &b"skin"[..])
        .unwrap();
    writer
        .write_license("LICENSE.md", &mut &b"terms"[..])
        .unwrap();
    writer.write_readme(&mut &b"readme"[..]).unwrap();
    writer.write_image_png(b"png bytes").unwrap();
    let mut buffer = writer.finish().unwrap();

    buffer.set_position(0);
    let mut reader = FantomeReader::new(buffer).unwrap();

    assert_eq!(reader.read_info().unwrap().name, "Round Trip");
    assert_eq!(
        reader.read_license().unwrap().unwrap(),
        ("LICENSE.md", b"terms".to_vec())
    );
    assert_eq!(reader.read_readme().unwrap().unwrap(), b"readme");
    assert_eq!(reader.read_image_png().unwrap().unwrap(), b"png bytes");

    // Backslashes in the relative path were normalized to `/`.
    let tmp = tempdir().unwrap();
    let dest = utf8_dir(&tmp);
    reader.extract_wads(&dest, &NoResolver).unwrap();
    assert_eq!(
        std::fs::read(dest.join("Test.wad.client/data/skin.bin")).unwrap(),
        b"skin"
    );
}

/// A resolver that names every chunk the same, so one chunk lands at a path
/// only the resolver could have chosen.
struct FixedResolver(&'static str);

impl PathResolver for FixedResolver {
    fn resolve(&self, _path_hash: WadHash) -> Option<String> {
        Some(self.0.to_owned())
    }
}

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

fn packed_wad_fantome() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("WAD/test.wad.client", options).unwrap();
    zip.write_all(&packed_wad_bytes(b"packed content")).unwrap();

    zip.finish().unwrap().into_inner()
}

/// Any name source drives the unpack, so a caller holding names in its own
/// form does not have to copy them into a table this crate owns.
#[test]
fn extract_wads_names_packed_chunks_through_any_resolver() {
    let mut reader = FantomeReader::new(Cursor::new(packed_wad_fantome())).unwrap();

    let tmp = tempdir().unwrap();
    let dest = utf8_dir(&tmp);
    let resolver = FixedResolver("assets/characters/aatrox/skin0.bin");
    reader.extract_wads(&dest, &resolver).unwrap();

    assert_eq!(
        std::fs::read(dest.join("test.wad.client/assets/characters/aatrox/skin0.bin")).unwrap(),
        b"packed content"
    );
}

/// A chunk no resolver and no bin names keeps its hex name, so a mod still
/// unpacks when no hashtable is available.
#[test]
fn extract_wads_falls_back_to_hex_names() {
    let mut reader = FantomeReader::new(Cursor::new(packed_wad_fantome())).unwrap();

    let tmp = tempdir().unwrap();
    let dest = utf8_dir(&tmp);
    reader.extract_wads(&dest, &NoResolver).unwrap();

    let unpacked: Vec<_> = std::fs::read_dir(dest.join("test.wad.client").as_std_path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(unpacked.len(), 1, "one chunk, one file: {unpacked:?}");

    let stem = unpacked[0].split('.').next().unwrap();
    assert_eq!(stem.len(), 16, "expected a hex name, got {}", unpacked[0]);
    assert!(u64::from_str_radix(stem, 16).is_ok());
}

/// A bin naming `path`, in the shape name recovery reads: the `PROP` magic,
/// then the little-endian `u16` length the format writes in front of a string.
fn bin_naming(path: &str) -> Vec<u8> {
    let mut bytes = b"PROP".to_vec();
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&u16::try_from(path.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(path.as_bytes());
    bytes
}

fn packed_wad_of(chunks: &[(&str, Vec<u8>)]) -> Vec<u8> {
    use ltk_wad::{WadBuilder, WadChunkBuilder, WadChunkCompression};

    let mut builder = WadBuilder::default();
    for (path, _) in chunks {
        builder = builder.with_chunk(
            WadChunkBuilder::default()
                .with_path(*path)
                .with_force_compression(WadChunkCompression::None),
        );
    }

    let by_hash: std::collections::HashMap<WadHash, Vec<u8>> = chunks
        .iter()
        .map(|(path, bytes)| (WadHash::from(*path), bytes.clone()))
        .collect();

    let mut cursor = Cursor::new(Vec::new());
    builder
        .build_to_writer(&mut cursor, move |hash, writer| {
            writer.write_all(&by_hash[&hash])?;
            Ok(())
        })
        .unwrap();
    cursor.into_inner()
}

/// A mod's WAD holds paths no game hashtable ever had, because its author
/// invented them, and its bins are where those paths are written down. Without
/// the recovery pass those chunks land under their hashes and the unpacked
/// project is unreadable.
#[test]
fn extract_wads_recovers_names_from_the_bins_of_a_packed_wad() {
    let skin = "assets/characters/invented/skin99.bin";
    let packed = packed_wad_of(&[
        ("assets/characters/invented/root.bin", bin_naming(skin)),
        (skin, b"invented bytes".to_vec()),
    ]);

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    zip.start_file("WAD/test.wad.client", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&packed).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let mut reader = FantomeReader::new(Cursor::new(archive)).unwrap();
    let tmp = tempdir().unwrap();
    let dest = utf8_dir(&tmp);
    reader.extract_wads(&dest, &NoResolver).unwrap();

    assert_eq!(
        std::fs::read(dest.join("test.wad.client").join(skin)).unwrap(),
        b"invented bytes"
    );
}
