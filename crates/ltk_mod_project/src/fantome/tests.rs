use super::*;
use crate::{
    Cancellation, ImportError, ImportProgress, ImportStage, ModProject, ModProjectAuthor,
    ModProjectLayer, ModProjectLicense, PackError, ProjectImporter, ProjectPacker, ProjectPath,
    ProjectPaths,
};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{FantomeInfo, FantomeLicense, FantomeReader};
use std::io::Cursor;
use std::sync::atomic::AtomicBool;
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
        layers: ModProjectLayer::default_table(),
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
    let imported = ProjectImporter::new(&extracted)
        .import(FantomeImporter::new(buffer))
        .unwrap();

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

/// The progress reports, as owned values a test can compare.
///
/// The match is total on purpose: it is the branching a consumer has to do, and
/// a stage added later fails here rather than being folded into its neighbour.
fn describe(progress: ImportProgress<'_>) -> (String, u32, u32) {
    let stage = match progress.stage {
        ImportStage::Extracting { item } => format!("extracting {item}"),
        ImportStage::WritingMetadata => "writing metadata".to_owned(),
        ImportStage::Complete => "complete".to_owned(),
    };
    (stage, progress.current, progress.total)
}
/// Import an in-memory archive with every driver hook left at its default.
fn import(
    data: Vec<u8>,
    output_dir: &Utf8Path,
) -> Result<ModProject, ImportError<FantomeImportError>> {
    ProjectImporter::new(output_dir).import(FantomeImporter::new(Cursor::new(data)))
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
    ProjectImporter::new(&output)
        .import(FantomeImporter::new(Cursor::new(buffer)).with_path_resolver(&FixedResolver))
        .unwrap();

    let skin = output.join("content/base/Aatrox.wad.client/assets/characters/aatrox/skin0.bin");
    assert_eq!(std::fs::read(&skin).unwrap(), b"skin bytes");
}

/// An archive whose `META/info.json` declares `layers`, each with one string
/// override so a dropped layer is visible as a dropped override.
fn create_fantome_with_layers(layers: &[(&str, i32)]) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let declared: Vec<String> = layers
        .iter()
        .map(|(name, priority)| {
            format!(
                r#""{name}": {{"Name": "{name}", "Priority": {priority}, "StringOverrides": {{"default": {{"key_{name}": "value"}}}}}}"#
            )
        })
        .collect();
    let info = format!(
        r#"{{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test", "Layers": {{{}}}}}"#,
        declared.join(",")
    );

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    zip.start_file("META/info.json", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(info.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

/// An archive whose `WAD/` holds `names`, each a directory of one file.
fn create_fantome_with_wads(names: &[&str]) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    let info = r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;
    zip.write_all(info.as_bytes()).unwrap();

    for name in names {
        zip.start_file(format!("WAD/{name}/data/file.bin"), options)
            .unwrap();
        zip.write_all(b"content").unwrap();
    }

    zip.finish().unwrap().into_inner()
}

/// Fantome stores content for the base layer alone, but the string overrides
/// on its other layers are metadata nothing downstream can recover.
#[test]
fn import_keeps_the_layers_the_archive_declares() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = import(create_fantome_with_layers(&[("skins", 10)]), &output).unwrap();

    let names: Vec<&str> = imported.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["base", "skins"], "base is added, skins is kept");

    let skins = &imported.layers[1];
    assert_eq!(skins.priority, 10);
    assert_eq!(
        skins.string_overrides["default"]["key_skins"], "value",
        "the overrides came across with the layer"
    );
}

/// `META/info.json` stores layers as a map, so only a sort makes two imports of
/// one archive agree.
#[test]
fn import_orders_layers_base_first_then_by_priority_then_by_name() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let archive = create_fantome_with_layers(&[("zed", 5), ("aatrox", 5), ("late", 20)]);
    let imported = import(archive, &output).unwrap();

    let names: Vec<&str> = imported.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["base", "aatrox", "zed", "late"]);
}

#[test]
fn import_of_an_archive_declaring_no_layers_gets_the_default_base() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = import(create_test_fantome(), &output).unwrap();

    assert_eq!(imported.layers, ModProjectLayer::default_table());
}

#[test]
fn import_reports_a_stage_for_each_wad_then_one_for_each_step_past_them() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let mut reported = Vec::new();
    let archive = create_fantome_with_wads(&["Zed.wad.client", "Aatrox.wad.client"]);
    ProjectImporter::new(&output)
        .import_with_progress(
            FantomeImporter::new(Cursor::new(archive)),
            &mut |progress| reported.push(describe(progress)),
        )
        .unwrap();

    assert_eq!(
        reported,
        [
            ("extracting Zed.wad.client".to_owned(), 0, 2),
            ("extracting Aatrox.wad.client".to_owned(), 1, 2),
            // No `RAW/` entries, so no `RAW/` pass and nothing counted for one.
            ("writing metadata".to_owned(), 2, 2),
            ("complete".to_owned(), 2, 2),
        ]
    );
}

#[test]
fn import_without_a_progress_callback_still_imports() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = ProjectImporter::new(&output)
        .import(FantomeImporter::new(Cursor::new(create_test_fantome())))
        .unwrap();

    assert_eq!(imported.name, "test-mod");
}

/// The config is written once, so what `with_config` sets is what the file on
/// disk says as well as what the call returns.
#[test]
fn with_config_names_the_project_and_the_written_config_agrees() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = ProjectImporter::new(&output)
        .with_config(|project| {
            project.name = "chosen-slug".to_owned();
            project.display_name = "Chosen Name".to_owned();
        })
        .import(FantomeImporter::new(Cursor::new(create_test_fantome())))
        .unwrap();

    assert_eq!(imported.name, "chosen-slug");
    assert_eq!(imported.display_name, "Chosen Name");

    let written = ModProject::load(&output).unwrap();
    assert_eq!(written, imported);
}

#[test]
fn a_cancellation_that_answers_true_fails_the_import() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let cancelled = || true;
    let result = ProjectImporter::new(&output)
        .with_cancellation(Cancellation::predicate(&cancelled))
        .import(FantomeImporter::new(Cursor::new(create_test_fantome())));

    assert!(matches!(result, Err(ImportError::Cancelled)));
    assert!(
        !output.join("mod.config.json").exists(),
        "the config is the last thing written, so a cancelled import has none"
    );
}

#[test]
fn a_cancellation_that_answers_false_imports_as_normal() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let flag = AtomicBool::new(false);
    let imported = ProjectImporter::new(&output)
        .with_cancellation(&flag)
        .import(FantomeImporter::new(Cursor::new(create_test_fantome())))
        .unwrap();

    assert_eq!(imported.name, "test-mod");
}

/// An archive can hold nothing but metadata, and the project it becomes still
/// has to be one the packer accepts.
#[test]
fn import_of_a_metadata_only_archive_still_has_a_base_layer_directory() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    zip.start_file("META/info.json", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(br#"{"Name": "Bare", "Author": "A", "Version": "1.0.0", "Description": "d"}"#)
        .unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);
    import(archive, &output).unwrap();

    assert!(output.join("content/base").is_dir());
    ProjectPacker::from_dir(output)
        .unwrap()
        .pack(FantomeFormat::new(Cursor::new(Vec::new())))
        .unwrap();
}

/// An archive can declare a layer it holds no content for - Fantome stores
/// content for the base layer alone - and the project it becomes still has to be
/// one the packer accepts.
#[test]
fn import_of_an_archive_declaring_a_layer_gives_that_layer_a_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir).join("project");

    import(create_fantome_with_layers(&[("skins", 10)]), &output).unwrap();

    assert!(output.join("content/skins").is_dir());
    ProjectPacker::from_dir(output)
        .unwrap()
        .pack(FantomeFormat::new(Cursor::new(Vec::new())))
        .unwrap();
}

/// The attack this guards against: a `WAD/` entry that climbs out of the
/// output directory and lands beside it. The archive is refused whole, so
/// nothing is written - neither the escaping file nor the mod's own content.
#[test]
fn import_refuses_an_archive_whose_entry_escapes_the_output_directory() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name":"Evil","Author":"A","Version":"1.0.0","Description":""}"#)
        .unwrap();
    zip.start_file("WAD/test.wad.client/assets/test.bin", options)
        .unwrap();
    zip.write_all(b"test content").unwrap();
    zip.start_file("WAD/../../pwned.txt", options).unwrap();
    zip.write_all(b"pwned").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let root = utf8_dir(&temp_dir);
    let output_dir = root.join("nested").join("project");

    let error = import(archive, &output_dir).unwrap_err();

    assert!(
        matches!(
            &error,
            ImportError::Format(FantomeImportError::Extract(
                ltk_fantome::FantomeExtractError::EscapingEntry { .. }
            ))
        ),
        "{error:?}"
    );
    assert!(
        !root.join("pwned.txt").exists(),
        "the escaping entry was written outside the output directory"
    );
    assert!(
        !output_dir
            .join("content")
            .join("base")
            .join("test.wad.client")
            .exists(),
        "the archive's own content was extracted despite the refusal"
    );
}

/// The `RAW/` pass is a unit of the extraction like a WAD is, so it is named,
/// counted, and inside the total. Leaving it out filled the bar before the pass
/// a raw-heavy mod spends most of its import in.
#[test]
fn the_raw_pass_is_a_counted_unit_of_the_extraction() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name": "T", "Author": "A", "Version": "1.0.0", "Description": "d"}"#)
        .unwrap();
    zip.start_file("WAD/Zed.wad.client/data/file.bin", options)
        .unwrap();
    zip.write_all(b"content").unwrap();
    zip.start_file("RAW/assets/loose.bin", options).unwrap();
    zip.write_all(b"loose").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let mut reported = Vec::new();
    ProjectImporter::new(&output)
        .import_with_progress(
            FantomeImporter::new(Cursor::new(archive)),
            &mut |progress| reported.push(describe(progress)),
        )
        .unwrap();

    assert_eq!(
        reported,
        [
            ("extracting Zed.wad.client".to_owned(), 0, 2),
            ("extracting RAW".to_owned(), 1, 2),
            ("writing metadata".to_owned(), 2, 2),
            ("complete".to_owned(), 2, 2),
        ]
    );
    assert!(output.join("content/base/raw/assets/loose.bin").is_file());
}

/// An unpacked `.wad.client` directory under `WAD/`, which is how an archive
/// ships a WAD without packing it. Real tools write an explicit zip directory
/// record for the folder and one for each subdirectory, so the fixture does
/// too: those records name no file, and the tree has to come out of the file
/// entries alone.
#[test]
fn a_wad_shipped_as_a_folder_imports_with_its_tree_intact() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name": "T", "Author": "A", "Version": "1.0.0", "Description": "d"}"#)
        .unwrap();

    zip.add_directory("WAD", options).unwrap();
    zip.add_directory("WAD/Aatrox.wad.client", options).unwrap();
    zip.add_directory("WAD/Aatrox.wad.client/assets", options)
        .unwrap();
    zip.add_directory("WAD/Aatrox.wad.client/assets/characters", options)
        .unwrap();

    let files = [
        ("WAD/Aatrox.wad.client/assets/characters/skin0.bin", "one"),
        ("WAD/Aatrox.wad.client/assets/characters/skin1.bin", "two"),
        ("WAD/Aatrox.wad.client/data/aatrox.bin", "three"),
        ("WAD/Zed.wad.client/data/zed.bin", "four"),
    ];
    for (name, body) in files {
        zip.start_file(name, options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    let archive = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir).join("project");

    let imported = import(archive, &output).unwrap();

    let base = output.join("content").join("base");
    for (name, body) in files {
        let landed = base.join(name.strip_prefix("WAD/").unwrap());
        assert_eq!(
            std::fs::read_to_string(&landed).unwrap(),
            body,
            "{name} did not land at {landed}"
        );
    }

    // Both folder WADs are directories of the project, so the packer reads the
    // WAD each file belongs to back out of the tree.
    assert!(base.join("Aatrox.wad.client").is_dir());
    assert!(base.join("Zed.wad.client").is_dir());

    ProjectPacker::new(imported, output)
        .pack(FantomeFormat::new(&mut Cursor::new(Vec::new())))
        .unwrap();
}

/// Both spellings of a WAD arrive the same way, which is what makes a folder a
/// drop-in for a packed file.
#[test]
fn a_folder_wad_and_a_packed_wad_are_listed_and_reported_alike() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name": "T", "Author": "A", "Version": "1.0.0", "Description": "d"}"#)
        .unwrap();

    // A folder WAD, directory record and all.
    zip.add_directory("WAD/Folder.wad.client", options).unwrap();
    zip.start_file("WAD/Folder.wad.client/data/x.bin", options)
        .unwrap();
    zip.write_all(b"folder").unwrap();

    // A packed WAD, stored as one entry.
    zip.start_file("WAD/Packed.wad.client", options).unwrap();
    zip.write_all(&packed_wad_bytes(b"packed")).unwrap();

    let archive = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let mut reported = Vec::new();
    ProjectImporter::new(&output)
        .import_with_progress(
            FantomeImporter::new(Cursor::new(archive)),
            &mut |progress| reported.push(describe(progress)),
        )
        .unwrap();

    assert_eq!(
        reported,
        [
            ("extracting Folder.wad.client".to_owned(), 0, 2),
            ("extracting Packed.wad.client".to_owned(), 1, 2),
            ("writing metadata".to_owned(), 2, 2),
            ("complete".to_owned(), 2, 2),
        ],
        "a folder WAD counts as one unit, exactly as a packed one does"
    );

    let base = output.join("content").join("base");
    assert!(base.join("Folder.wad.client/data/x.bin").is_file());
    assert!(base.join("Packed.wad.client").is_dir());
}

// -- where an import puts things -------------------------------------------

/// An archive holding every kind of entry an import places, so the preflight is
/// checked against a tree with a `RAW/` file and root files in it as well as
/// WAD content.
fn create_fantome_with_every_kind_of_entry() -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name":"Test Mod","Author":"A","Version":"1.0.0","Description":"d"}"#)
        .unwrap();

    zip.start_file("META/README.md", options).unwrap();
    zip.write_all(b"# Test Mod\n").unwrap();

    zip.start_file("META/LICENSE.txt", options).unwrap();
    zip.write_all(b"The terms.").unwrap();

    zip.add_directory("WAD/test.wad.client", options).unwrap();
    zip.start_file("WAD/test.wad.client/assets/test.bin", options)
        .unwrap();
    zip.write_all(b"test content").unwrap();

    zip.start_file("RAW/assets/loose.bin", options).unwrap();
    zip.write_all(b"raw content").unwrap();

    zip.finish().unwrap().into_inner()
}

/// A preflight is only worth having if it agrees with the import. The two are
/// separate statements here - the importer writes through `extract_wads` and
/// `extract_raw`, the preflight reads the entry names - so nothing but this
/// holds them together.
#[test]
fn the_predicted_paths_match_what_an_import_writes() {
    let archive = create_fantome_with_every_kind_of_entry();

    let reader = FantomeReader::new(Cursor::new(archive.clone())).unwrap();
    let mut predicted: Vec<Utf8PathBuf> = reader
        .iter_project_paths()
        .map(|path| {
            assert!(
                !path.is_unpacked_wad(),
                "this archive holds no packed WAD, got {path}"
            );
            path.into_path()
        })
        .collect();
    predicted.sort();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);
    import(archive, &output).unwrap();

    for path in &predicted {
        assert!(
            output.join(path).is_file(),
            "{path} was predicted but not written"
        );
    }

    // And nothing was written that was not predicted, config aside: the config
    // is the driver's, not the archive's.
    let mut written = Vec::new();
    collect_files(&output, &output, &mut written);
    written.retain(|path| path != "mod.config.json");
    written.sort();

    assert_eq!(written, predicted);
}

/// A packed WAD is a directory the import unpacks into, and the answer says so
/// rather than naming a file that never lands.
#[test]
fn a_packed_wad_is_predicted_as_a_directory_the_import_unpacks_into() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();
    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name":"M","Author":"A","Version":"1.0.0","Description":"d"}"#)
        .unwrap();
    zip.start_file("WAD/test.wad.client", options).unwrap();
    zip.write_all(&packed_wad_bytes(b"payload")).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let reader = FantomeReader::new(Cursor::new(archive.clone())).unwrap();
    let predicted: Vec<ProjectPath> = reader.iter_project_paths().collect();

    assert_eq!(
        predicted,
        [ProjectPath::unpacked_wad("content/base/test.wad.client")]
    );

    // And the import does unpack into it, rather than writing a file there.
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);
    import(archive, &output).unwrap();

    assert!(output.join("content/base/test.wad.client").is_dir());
}

fn collect_files(root: &Utf8Path, dir: &Utf8Path, into: &mut Vec<Utf8PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = Utf8PathBuf::from_path_buf(entry.unwrap().path()).unwrap();
        if path.is_dir() {
            collect_files(root, &path, into);
        } else {
            into.push(path.strip_prefix(root).unwrap().to_owned());
        }
    }
}
