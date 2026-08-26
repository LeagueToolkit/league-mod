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

/// Without a resolver a packed chunk keeps its hex name, so a mod still
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
