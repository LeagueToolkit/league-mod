use super::*;
use camino::Utf8PathBuf;
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
    reader.extract_wads(&dest, None).unwrap();

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
    reader.extract_wads(&dest.join("wads"), None).unwrap();
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
    reader.extract_wads(&dest, None).unwrap();
    assert_eq!(
        std::fs::read(dest.join("Test.wad.client/data/skin.bin")).unwrap(),
        b"skin"
    );
}
