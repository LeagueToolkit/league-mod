use super::*;
use crate::{
    default_layers, ImportFormat, ModProject, ModProjectAuthor, ModProjectLicense, PackError,
    ProjectPacker,
};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{FantomeInfo, FantomeLicense};
use std::io::Cursor;
use tempfile::TempDir;

/// The temp directory's path, which packing takes as UTF-8.
fn utf8_dir(dir: &TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap()
}

fn test_project(license: Option<ModProjectLicense>) -> ModProject {
    ModProject {
        name: "test-mod".to_string(),
        display_name: "Test Mod".to_string(),
        version: "1.0.0".to_string(),
        description: "A test mod".to_string(),
        authors: vec![ModProjectAuthor::Name("Alice".to_string())],
        license,
        layers: default_layers(),
        ..Default::default()
    }
}

/// Write a minimal project tree with one base-layer WAD file.
fn write_project_tree(root: &Utf8Path) {
    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::create_dir_all(&wad_dir).unwrap();
    std::fs::write(wad_dir.join("data.bin"), b"content").unwrap();
}

fn try_pack(
    project: &ModProject,
    root: &Utf8Path,
) -> Result<Cursor<Vec<u8>>, PackError<FantomePackError>> {
    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::new(project.clone(), root.to_owned()).pack(FantomeFormat::new(&mut buffer))?;
    buffer.set_position(0);
    Ok(buffer)
}

fn pack(project: &ModProject, root: &Utf8Path) -> Cursor<Vec<u8>> {
    try_pack(project, root).unwrap()
}

// -- packing tests ----------------------------------------------------------

#[test]
fn pack_writes_license_file_and_field() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);
    std::fs::write(root.join("LICENSE.md"), "The terms.").unwrap();

    let project = test_project(Some(ModProjectLicense::Spdx("MIT".to_string())));
    let buffer = pack(&project, &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();

    // The source file's name is preserved in the entry name.
    let mut license = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("META/LICENSE.md").unwrap(),
        &mut license,
    )
    .unwrap();
    assert_eq!(license, "The terms.");

    let mut info_content = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("META/info.json").unwrap(),
        &mut info_content,
    )
    .unwrap();
    let info: FantomeInfo = serde_json::from_str(&info_content).unwrap();
    assert_eq!(info.license, Some(FantomeLicense::Spdx("MIT".to_string())));
}

#[test]
fn pack_omits_license_entry_when_project_has_none() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();
    assert!(archive.by_name("META/LICENSE").is_err());
}

#[test]
fn license_survives_project_fantome_project_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp).join("project");
    write_project_tree(&root);
    std::fs::write(root.join("LICENSE.txt"), "Round trip terms.").unwrap();

    let project = test_project(Some(ModProjectLicense::Custom {
        name: "My License".to_string(),
        url: None,
    }));

    let buffer = pack(&project, &root);

    let extracted = utf8_dir(&tmp).join("extracted");
    let imported = FantomeImporter::new(buffer).import(&extracted).unwrap();

    assert_eq!(
        imported.license,
        Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: None,
        })
    );

    // The file comes back under the name it went in with.
    assert_eq!(
        std::fs::read_to_string(extracted.join("LICENSE.txt")).unwrap(),
        "Round trip terms."
    );
}

#[test]
fn pack_canonicalizes_license_entry_name() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);
    std::fs::write(root.join("license.txt"), "The terms.").unwrap();

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();

    // A lowercase source name is written under its canonical spelling, so
    // repacking an extracted project is stable rather than case-drifting.
    assert!(archive.by_name("META/LICENSE.txt").is_ok());
}

#[test]
fn pack_skips_modignored_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::write(wad_dir.join("source.psd"), b"working file").unwrap();
    std::fs::write(root.join(".modignore"), "*.psd\n").unwrap();

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();

    // The rest of the WAD directory is packed as before.
    assert!(archive.by_name("WAD/Test.wad.client/data.bin").is_ok());
    assert!(archive.by_name("WAD/Test.wad.client/source.psd").is_err());
}

#[test]
fn pack_detects_wad_directories_case_insensitively() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let wad_dir = root.join("content").join("base").join("Upper.WAD.Client");
    std::fs::create_dir_all(&wad_dir).unwrap();
    std::fs::write(wad_dir.join("data.bin"), b"data").unwrap();

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();

    // The entry keeps the author's spelling; only detection is folded.
    assert!(archive.by_name("WAD/Upper.WAD.Client/data.bin").is_ok());
}

#[test]
fn pack_applies_nested_modignore_and_never_archives_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::write(wad_dir.join("source.psd"), b"working file").unwrap();
    std::fs::write(wad_dir.join(".modignore"), "*.psd\n").unwrap();

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();

    assert!(archive.by_name("WAD/Test.wad.client/data.bin").is_ok());
    assert!(archive.by_name("WAD/Test.wad.client/source.psd").is_err());
    assert!(
        archive.by_name("WAD/Test.wad.client/.modignore").is_err(),
        "filter metadata leaked into the archive"
    );
}

#[test]
fn pack_skips_content_outside_wad_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    // Loose files and plain directories are packable to modpkg but have no
    // place in a Fantome archive.
    std::fs::write(root.join("content/base/loose.bin"), b"loose").unwrap();
    let plain_dir = root.join("content/base/some_dir");
    std::fs::create_dir_all(&plain_dir).unwrap();
    std::fs::write(plain_dir.join("file.bin"), b"plain").unwrap();

    let buffer = pack(&test_project(None), &root);

    let archive = zip::ZipArchive::new(buffer).unwrap();
    let names: Vec<&str> = archive.file_names().collect();
    assert!(
        names
            .iter()
            .all(|name| !name.contains("loose.bin") && !name.contains("some_dir")),
        "non-WAD content leaked into the archive: {names:?}"
    );
}

#[test]
fn pack_drops_non_base_layers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let hires_wad = root
        .join("content")
        .join("high-res")
        .join("Test.wad.client");
    std::fs::create_dir_all(&hires_wad).unwrap();
    std::fs::write(hires_wad.join("extra.bin"), b"extra").unwrap();

    let mut project = test_project(None);
    project.layers.push(crate::ModProjectLayer {
        name: "high-res".to_string(),
        priority: 1,
        ..Default::default()
    });

    let buffer = pack(&project, &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();
    assert!(archive.by_name("WAD/Test.wad.client/data.bin").is_ok());
    assert!(archive.by_name("WAD/Test.wad.client/extra.bin").is_err());
}

#[test]
fn pack_embeds_an_unconfigured_default_thumbnail() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    // A 1x1 image, saved as the default thumbnail.webp with no config entry.
    let img = image::DynamicImage::new_rgb8(1, 1);
    img.save(root.join("thumbnail.webp").as_std_path()).unwrap();

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();
    assert!(
        archive.by_name("META/image.png").is_ok(),
        "the default thumbnail.webp must be embedded, as it is for modpkg"
    );
}

#[test]
fn pack_reports_an_unreadable_thumbnail_with_its_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    // Present, so packing reaches it, but not an image.
    std::fs::write(root.join("thumbnail.webp"), b"not an image").unwrap();

    let project = ModProject {
        thumbnail: Some("thumbnail.webp".to_string()),
        ..test_project(None)
    };

    let error = try_pack(&project, &root).unwrap_err();

    match error {
        PackError::Format(FantomePackError::Thumbnail { path, .. }) => {
            assert_eq!(path, root.join("thumbnail.webp"));
        }
        other => panic!("expected Thumbnail, got {other:?}"),
    }
}

/// An error's own message must not repeat what its source says, or an error
/// chain prints the same sentence twice.
#[test]
fn pack_error_display_does_not_embed_its_source() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);
    std::fs::write(root.join("thumbnail.webp"), b"not an image").unwrap();

    let project = ModProject {
        thumbnail: Some("thumbnail.webp".to_string()),
        ..test_project(None)
    };

    let error = try_pack(&project, &root).unwrap_err();

    let source = std::error::Error::source(&error).unwrap().to_string();
    assert!(
        !error.to_string().contains(&source),
        "`{error}` already contains its source `{source}`"
    );
}

// -- import tests -----------------------------------------------------------

/// Imports through the trait, keeping the forwarding [`ImportFormat`] impl
/// exercised; direct callers resolve to the inherent method instead.
fn import(data: Vec<u8>, output_dir: &Utf8Path) -> Result<ModProject, FantomeImportError> {
    ImportFormat::import(FantomeImporter::new(Cursor::new(data)), output_dir)
}

fn create_test_fantome() -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

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

    zip.finish().unwrap().into_inner()
}

/// Build a fantome archive whose license entry is named `license_entry`.
fn create_fantome_with_license(license_entry: &str, info: &str) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(info.as_bytes()).unwrap();

    zip.start_file(license_entry, options).unwrap();
    zip.write_all(b"The terms.").unwrap();

    zip.finish().unwrap().into_inner()
}

#[test]
fn import_materializes_a_project() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = import(create_test_fantome(), &output).unwrap();

    assert_eq!(imported.display_name, "Test Mod");
    assert_eq!(imported.name, "test-mod");
    assert_eq!(imported.version, "1.0.0");

    // Check that mod.config.json was created
    assert!(output.join("mod.config.json").exists());

    // Check that WAD content was extracted
    assert!(output
        .join("content/base/test.wad.client/assets/test.bin")
        .exists());
}

#[test]
fn import_license_entry_case_and_extension_variants() {
    let info = r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;

    for (entry, expected_file) in [
        ("META/LICENSE", "LICENSE"),
        ("META/license.md", "LICENSE.md"),
        ("meta/LICENSE.TXT", "LICENSE.txt"),
    ] {
        let data = create_fantome_with_license(entry, info);

        let temp_dir = tempfile::tempdir().unwrap();
        let output = utf8_dir(&temp_dir);
        import(data, &output).unwrap();

        let extracted = output.join(expected_file);
        assert!(
            extracted.exists(),
            "expected {expected_file} for archive entry {entry}"
        );
        assert_eq!(std::fs::read_to_string(&extracted).unwrap(), "The terms.");
    }
}

#[test]
fn import_reads_the_license_field() {
    let info = r#"{
        "Name": "Test",
        "Author": "Test",
        "Version": "1.0.0",
        "Description": "Test",
        "License": "Apache-2.0"
    }"#;
    let data = create_fantome_with_license("META/LICENSE", info);

    let temp_dir = tempfile::tempdir().unwrap();
    let imported = import(data, &utf8_dir(&temp_dir)).unwrap();

    assert_eq!(
        imported.license,
        Some(ModProjectLicense::Spdx("Apache-2.0".to_string()))
    );
}

#[test]
fn import_reads_a_custom_license_field_without_url() {
    let info = r#"{
        "Name": "Test",
        "Author": "Test",
        "Version": "1.0.0",
        "Description": "Test",
        "License": { "Name": "My License" }
    }"#;
    let data = create_fantome_with_license("META/LICENSE", info);

    let temp_dir = tempfile::tempdir().unwrap();
    let imported = import(data, &utf8_dir(&temp_dir)).unwrap();

    assert_eq!(
        imported.license,
        Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: None,
        })
    );
}

#[test]
fn import_of_a_legacy_fantome_has_no_license() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = import(create_test_fantome(), &output).unwrap();

    assert_eq!(imported.license, None);
    assert!(!output.join("LICENSE").exists());
}

#[test]
fn import_extracts_raw_files() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    let info = r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;
    zip.write_all(info.as_bytes()).unwrap();

    zip.add_directory("RAW", options).unwrap();
    zip.start_file("RAW/assets/characters/aatrox/skin0.bin", options)
        .unwrap();
    zip.write_all(b"aatrox data").unwrap();
    zip.start_file("RAW/assets/maps/map11/scene.bin", options)
        .unwrap();
    zip.write_all(b"map data").unwrap();

    let buffer = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);
    let imported = import(buffer, &output).unwrap();
    assert_eq!(imported.display_name, "Test");

    let raw_file1 = output.join("content/base/raw/assets/characters/aatrox/skin0.bin");
    assert!(raw_file1.exists());
    assert_eq!(std::fs::read(&raw_file1).unwrap(), b"aatrox data");

    let raw_file2 = output.join("content/base/raw/assets/maps/map11/scene.bin");
    assert!(raw_file2.exists());
    assert_eq!(std::fs::read(&raw_file2).unwrap(), b"map data");
}

/// Names every chunk the same, so one chunk lands at a path only the
/// resolver could have chosen.
struct FixedResolver;

impl PathResolver for FixedResolver {
    fn resolve(&self, _path_hash: ltk_wad::WadHash) -> Option<String> {
        Some(String::from("assets/characters/aatrox/skin0.bin"))
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
            std::io::Write::write_all(writer, &payload)?;
            Ok(())
        })
        .unwrap();
    cursor.into_inner()
}

/// The importer hands its resolver to the unpack, so a caller naming chunks
/// from its own tables gets real paths in the project tree.
#[test]
fn import_names_packed_wad_chunks_through_the_resolver() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    let info = r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;
    zip.write_all(info.as_bytes()).unwrap();

    zip.start_file("WAD/Aatrox.wad.client", options).unwrap();
    zip.write_all(&packed_wad_bytes(b"skin bytes")).unwrap();

    let buffer = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);
    FantomeImporter::new(Cursor::new(buffer))
        .with_path_resolver(&FixedResolver)
        .import(&output)
        .unwrap();

    let skin = output.join("content/base/Aatrox.wad.client/assets/characters/aatrox/skin0.bin");
    assert_eq!(std::fs::read(&skin).unwrap(), b"skin bytes");
}
