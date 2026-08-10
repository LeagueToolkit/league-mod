use super::*;
use crate::{
    ModProject, ModProjectAuthor, ModProjectLayer, ModProjectLicense, PackError, PackReport,
    ProjectPacker,
};
use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;
use ltk_modpkg::{Modpkg, ModpkgCompression, LICENSE_CHUNK_PATH};
use std::fs;
use std::io::Cursor;

// -- test helpers -----------------------------------------------------------

fn test_mod_project(layers: Vec<ModProjectLayer>) -> ModProject {
    ModProject {
        name: "test-mod".to_string(),
        display_name: "Test Mod".to_string(),
        version: "1.0.0".to_string(),
        layers,
        ..Default::default()
    }
}

fn utf8_tempdir(tmp: &tempfile::TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap()
}

/// Create a file inside `content/{layer}/{rel_path}`, creating directories as needed.
fn create_content_file(root: &Utf8Path, layer: &str, rel_path: &str, data: &[u8]) {
    let full_path = root.join("content").join(layer).join(rel_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&full_path, data).unwrap();
}

/// A mounted archive packed in memory, plus the driver's report.
type PackedModpkg = (Modpkg<Cursor<Vec<u8>>>, PackReport);

fn try_pack(
    project: ModProject,
    root: &Utf8Path,
) -> Result<PackedModpkg, PackError<ModpkgPackError>> {
    let mut buffer = Cursor::new(Vec::new());
    let report =
        ProjectPacker::new(project, root.to_owned()).pack(ModpkgFormat::new(&mut buffer))?;
    buffer.set_position(0);
    Ok((Modpkg::mount_from_reader(buffer).unwrap(), report))
}

fn pack(project: ModProject, root: &Utf8Path) -> (Modpkg<Cursor<Vec<u8>>>, PackReport) {
    try_pack(project, root).unwrap()
}

// -- format-specific validation ---------------------------------------------

#[test]
fn invalid_layer_slug_fails_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    fs::create_dir_all(root.join("content/base")).unwrap();
    fs::create_dir_all(root.join("content/UPPERCASE")).unwrap();

    let project = test_mod_project(vec![
        ModProjectLayer::base(),
        ModProjectLayer {
            name: "UPPERCASE".to_string(),
            priority: 1,
            ..Default::default()
        },
    ]);

    let err = try_pack(project, &root).unwrap_err();
    assert!(
        matches!(
            err,
            PackError::Format(ModpkgPackError::InvalidLayerName(ref e)) if e.value() == "UPPERCASE"
        ),
        "Expected InvalidLayerName, got: {err}"
    );
}

// -- packing tests ----------------------------------------------------------

#[test]
fn pack_single_wad() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "Graves.wad.client/data/skin0.bin", b"bin");
    create_content_file(&root, "base", "Graves.wad.client/assets/tex.dds", b"dds");

    let (modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert_eq!(modpkg.wads.len(), 1);
    assert_eq!(modpkg.wads.values().next().unwrap(), "graves.wad.client");

    let layer_idx = modpkg.layer_index("base").expect("base layer");
    let wad_idx = modpkg.wad_index("graves.wad.client").unwrap();
    assert_eq!(modpkg.chunks_for_wad_layer(wad_idx, layer_idx).len(), 2);

    for path in modpkg.chunk_paths.values() {
        assert!(
            !path.contains("graves.wad.client"),
            "WAD prefix leaked: {path}"
        );
    }
}

#[test]
fn pack_non_wad_directory_preserves_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "some_dir/file.bin", b"data");

    let (modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert_eq!(modpkg.wads.len(), 0);
    assert!(modpkg
        .chunk_paths
        .values()
        .any(|p| p == "some_dir/file.bin"));
}

#[test]
fn pack_multi_wad_multi_layer() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "Aatrox.wad.client/data/skin0.bin", b"s");
    create_content_file(&root, "base", "Map11.wad.client/data/map.bin", b"m");
    create_content_file(&root, "high-res", "Aatrox.wad.client/assets/tex.dds", b"t");

    let project = test_mod_project(vec![
        ModProjectLayer::base(),
        ModProjectLayer {
            name: "high-res".to_string(),
            display_name: None,
            priority: 1,
            description: None,
            string_overrides: IndexMap::new(),
        },
    ]);

    let (modpkg, _) = pack(project, &root);

    assert_eq!(modpkg.wads.len(), 2);
    let wad_names: Vec<&str> = modpkg.wads.values().map(|s| s.as_str()).collect();
    assert!(wad_names.contains(&"aatrox.wad.client"));
    assert!(wad_names.contains(&"map11.wad.client"));

    let base_idx = modpkg.layer_index("base").unwrap();
    let hires_idx = modpkg.layer_index("high-res").unwrap();
    let aatrox_idx = modpkg.wad_index("aatrox.wad.client").unwrap();
    let map_idx = modpkg.wad_index("map11.wad.client").unwrap();

    assert_eq!(modpkg.chunks_for_wad_layer(aatrox_idx, base_idx).len(), 1);
    assert_eq!(modpkg.chunks_for_wad_layer(map_idx, base_idx).len(), 1);
    assert_eq!(modpkg.chunks_for_wad_layer(aatrox_idx, hires_idx).len(), 1);
    assert_eq!(modpkg.chunks_for_wad_layer(map_idx, hires_idx).len(), 0);
}

#[test]
fn pack_preserves_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let project = ModProject {
        name: "cool-mod".to_string(),
        display_name: "Cool Mod".to_string(),
        version: "2.1.0".to_string(),
        description: "A cool mod".to_string(),
        authors: vec![ModProjectAuthor::Name("Alice".to_string())],
        license: Some(ModProjectLicense::Spdx("MIT".to_string())),
        champions: vec!["Graves".to_string()],
        layers: vec![ModProjectLayer::base()],
        ..Default::default()
    };

    let (mut modpkg, _) = pack(project, &root);
    let meta = modpkg.load_metadata().unwrap();

    assert_eq!(meta.name, "cool-mod");
    assert_eq!(meta.display_name, "Cool Mod");
    assert_eq!(meta.version.to_string(), "2.1.0");
    assert_eq!(meta.description, Some("A cool mod".to_string()));
    assert_eq!(meta.authors.len(), 1);
    assert_eq!(meta.authors[0].name, "Alice");
    assert_eq!(meta.champions, vec!["Graves"]);
}

#[test]
fn pack_rejects_a_non_semver_version() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let project = ModProject {
        version: "one point oh".to_string(),
        ..test_mod_project(vec![ModProjectLayer::base()])
    };

    let err = try_pack(project, &root).unwrap_err();
    assert!(
        matches!(err, PackError::Format(ModpkgPackError::InvalidVersion(_))),
        "expected InvalidVersion, got: {err}"
    );
}

#[test]
fn pack_includes_license_text_chunk() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    let license_text = "MIT License\n\nCopyright (c) 2026 Someone\n";
    fs::write(root.join("LICENSE"), license_text).unwrap();

    let (mut modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert_eq!(
        modpkg.load_license_text().unwrap(),
        license_text.as_bytes(),
        "license text must round-trip byte-for-byte"
    );
}

#[test]
fn pack_stores_license_text_compressed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    // Roughly the shape of a real license: long, repetitive, highly compressible.
    let license_text = "Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files.\n".repeat(60);
    fs::write(root.join("LICENSE"), &license_text).unwrap();

    let (mut modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let chunk = *modpkg.get_chunk(LICENSE_CHUNK_PATH, None).unwrap();
    assert_eq!(chunk.compression, ModpkgCompression::Zstd);
    assert!(
        chunk.compressed_size < chunk.uncompressed_size,
        "expected the license chunk to shrink: {} -> {}",
        chunk.uncompressed_size,
        chunk.compressed_size
    );

    // The reader decompresses transparently.
    assert_eq!(
        modpkg.load_license_text().unwrap(),
        license_text.as_bytes(),
        "compressed license text must still round-trip byte-for-byte"
    );
}

#[test]
fn pack_stores_incompressible_license_text_raw() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    // Too short for compression to pay: the builder must fall back to raw
    // storage and record that in the TOC entry.
    let license_text = "MIT";
    fs::write(root.join("LICENSE"), license_text).unwrap();

    let (mut modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let chunk = *modpkg.get_chunk(LICENSE_CHUNK_PATH, None).unwrap();
    assert_eq!(chunk.compression, ModpkgCompression::None);
    assert_eq!(chunk.compressed_size, chunk.uncompressed_size);

    assert_eq!(modpkg.load_license_text().unwrap(), license_text.as_bytes());
}

#[test]
fn pack_finds_license_file_by_extension_precedence() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    fs::write(root.join("LICENSE.md"), "markdown terms").unwrap();

    let (mut modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert_eq!(modpkg.load_license_text().unwrap(), b"markdown terms");
}

#[test]
fn pack_without_license_file_has_no_license_chunk() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let (mut modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert!(!modpkg.has_chunk(LICENSE_CHUNK_PATH, None));
    assert!(
        matches!(
            modpkg.load_license_text(),
            Err(ltk_modpkg::ModpkgError::MissingChunk(_))
        ),
        "expected a clean MissingChunk error"
    );
}

#[test]
fn pack_preserves_custom_license_without_url() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let mut project = test_mod_project(vec![ModProjectLayer::base()]);
    project.license = Some(ModProjectLicense::Custom {
        name: "My License".to_string(),
        url: None,
    });

    let (mut modpkg, _) = pack(project, &root);
    let meta = modpkg.load_metadata().unwrap();

    assert_eq!(
        meta.license,
        ltk_modpkg::ModpkgLicense::Custom {
            name: "My License".to_string(),
            url: None,
        }
    );
}

#[test]
fn pack_preserves_non_utf8_license_text() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    // Latin-1 "Copyright © 2026", a realistic encoding for a license text and
    // one the project never opted into shipping.
    fs::write(root.join("LICENSE.txt"), b"Copyright \xA9 2026").unwrap();

    let (mut modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    // The exact bytes, not a lossy decode. A license is a legal document, so
    // silently rewriting the byte we could not decode is worse than either
    // failing or storing what the author wrote.
    assert_eq!(modpkg.load_license_text().unwrap(), b"Copyright \xA9 2026");
}

#[test]
fn pack_preserves_non_utf8_readme() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    fs::write(root.join("README.md"), b"Caf\xE9 mod").unwrap();

    let (mut modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert_eq!(modpkg.load_readme().unwrap(), b"Caf\xE9 mod");
}

/// Pack -> extract must return the author's exact bytes. This is the property
/// `meta_chunk_target` in `ltk_modpkg`'s extractor exists for, and a lossy
/// decode on the pack side would break it while every assertion above still
/// passed.
#[test]
fn license_survives_a_pack_extract_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    let original: &[u8] = b"Copyright \xA9 2026 Someone";
    fs::write(root.join("LICENSE"), original).unwrap();

    let (mut modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let out = tmp.path().join("extracted");
    ltk_modpkg::ModpkgExtractor::new(&mut modpkg)
        .extract_all(&out)
        .unwrap();

    assert_eq!(fs::read(out.join("LICENSE")).unwrap(), original);
}

/// End-to-end check that the driver's `.modignore` filtering reaches the
/// archive. The filter semantics themselves are covered by the driver tests
/// in `crate::pack`.
#[test]
fn modignore_excludes_matching_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/tex.dds", b"dds");
    create_content_file(&root, "base", "X.wad.client/src.psd", b"psd");
    fs::write(root.join(".modignore"), "*.psd\n").unwrap();

    let (modpkg, report) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert!(modpkg.chunk_paths.values().any(|p| p == "tex.dds"));
    assert!(!modpkg.chunk_paths.values().any(|p| p == "src.psd"));

    assert_eq!(
        report.ignored_files(),
        [root
            .join("content")
            .join("base")
            .join("X.wad.client")
            .join("src.psd")]
    );
    assert_eq!(report.ignored_count(), 1);
}
