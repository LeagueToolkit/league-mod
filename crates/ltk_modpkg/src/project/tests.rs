use super::*;
use crate::{Modpkg, ModpkgCompression, LICENSE_CHUNK_PATH};
use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;
use ltk_mod_project::{ModProject, ModProjectAuthor, ModProjectLayer, ModProjectLicense};
use std::fs::{self, File};
use std::io::Cursor;

// -- validation tests ------------------------------------------------------

#[test]
fn new_rejects_invalid_layer_slug() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    fs::create_dir_all(root.join("content/base")).unwrap();

    let project = test_mod_project(vec![ModProjectLayer {
        name: "UPPERCASE".to_string(),
        display_name: None,
        priority: 1,
        description: None,
        string_overrides: IndexMap::new(),
    }]);

    let err = ProjectPacker::with_mod_project(project, root.clone()).unwrap_err();
    assert!(
        matches!(err, PackError::InvalidLayerName(ref e) if e.value() == "UPPERCASE"),
        "Expected InvalidLayerName, got: {err}"
    );
}

#[test]
fn new_rejects_base_layer_with_wrong_priority() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    fs::create_dir_all(root.join("content/base")).unwrap();

    let project = test_mod_project(vec![ModProjectLayer {
        name: "base".to_string(),
        display_name: None,
        priority: 5,
        description: None,
        string_overrides: IndexMap::new(),
    }]);

    let err = ProjectPacker::with_mod_project(project, root.clone()).unwrap_err();
    assert!(
        matches!(err, PackError::InvalidBaseLayerPriority(5)),
        "Expected InvalidBaseLayerPriority(5), got: {err}"
    );
}

#[test]
fn new_rejects_missing_layer_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    fs::create_dir_all(root.join("content/base")).unwrap();
    // "high-res" layer declared but directory not created

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

    let err = ProjectPacker::with_mod_project(project, root.clone()).unwrap_err();
    assert!(
        matches!(err, PackError::LayerDirMissing { ref layer, .. } if layer == "high-res"),
        "Expected LayerDirMissing for high-res, got: {err}"
    );
}

// -- packing tests ---------------------------------------------------------

#[test]
fn new_loads_config_from_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    // Write a mod.config.json
    let config = r#"{
        "name": "auto-load-test",
        "display_name": "Auto Load Test",
        "version": "1.0.0",
        "description": "",
        "authors": [],
        "layers": [{"name": "base", "priority": 0}]
    }"#;
    fs::write(root.join("mod.config.json"), config).unwrap();
    create_content_file(&root, "base", "X.wad.client/f.bin", b"data");

    let output = root.join("build/out.modpkg");
    ProjectPacker::new(root).unwrap().pack(&output).unwrap();

    let modpkg = mount_modpkg(&output);
    assert_eq!(modpkg.wads.len(), 1);
}

#[test]
fn new_returns_error_for_missing_config() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    let err = ProjectPacker::new(root).unwrap_err();
    assert!(
        matches!(
            err,
            PackError::Config(ltk_mod_project::ModProjectError::ConfigNotFound(_))
        ),
        "Expected a missing config file, got: {err}"
    );
}

#[test]
fn pack_single_wad() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "Graves.wad.client/data/skin0.bin", b"bin");
    create_content_file(&root, "base", "Graves.wad.client/assets/tex.dds", b"dds");

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");

    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);

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

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");

    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);

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

    let output = root.join("build/out.modpkg");
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);

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
fn pack_to_writer_produces_valid_modpkg() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "Graves.wad.client/data/skin.bin", b"data");

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack_to_writer(&mut buffer)
        .unwrap();

    buffer.set_position(0);
    let modpkg = Modpkg::mount_from_reader(buffer).unwrap();

    assert_eq!(modpkg.wads.len(), 1);
    assert_eq!(modpkg.wads.values().next().unwrap(), "graves.wad.client");
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
        tags: vec![],
        champions: vec!["Graves".to_string()],
        maps: vec![],
        thumbnail: None,
        layers: vec![ModProjectLayer::base()],
        transformers: vec![],
    };

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack_to_writer(&mut buffer)
        .unwrap();

    buffer.set_position(0);
    let mut modpkg = Modpkg::mount_from_reader(buffer).unwrap();
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
fn pack_includes_license_text_chunk() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    let license_text = "MIT License\n\nCopyright (c) 2026 Someone\n";
    fs::write(root.join("LICENSE"), license_text).unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let output = root.join("build/out.modpkg");
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let mut modpkg = mount_modpkg(&output);

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

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack_to_writer(&mut buffer)
        .unwrap();

    buffer.set_position(0);
    let mut modpkg = Modpkg::mount_from_reader(buffer).unwrap();

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

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack_to_writer(&mut buffer)
        .unwrap();

    buffer.set_position(0);
    let mut modpkg = Modpkg::mount_from_reader(buffer).unwrap();

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

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack_to_writer(&mut buffer)
        .unwrap();

    buffer.set_position(0);
    let mut modpkg = Modpkg::mount_from_reader(buffer).unwrap();

    assert_eq!(modpkg.load_license_text().unwrap(), b"markdown terms");
}

#[test]
fn pack_without_license_file_has_no_license_chunk() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack_to_writer(&mut buffer)
        .unwrap();

    buffer.set_position(0);
    let mut modpkg = Modpkg::mount_from_reader(buffer).unwrap();

    assert!(!modpkg.has_chunk(LICENSE_CHUNK_PATH, None));
    assert!(
        matches!(
            modpkg.load_license_text(),
            Err(crate::error::ModpkgError::MissingChunk(_))
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

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack_to_writer(&mut buffer)
        .unwrap();

    buffer.set_position(0);
    let mut modpkg = Modpkg::mount_from_reader(buffer).unwrap();
    let meta = modpkg.load_metadata().unwrap();

    assert_eq!(
        meta.license,
        crate::ModpkgLicense::Custom {
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

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::with_mod_project(project, root.clone())
        .expect("a non-UTF-8 license file must not fail the pack")
        .pack_to_writer(&mut buffer)
        .unwrap();

    buffer.set_position(0);
    let mut modpkg = Modpkg::mount_from_reader(buffer).unwrap();

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

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::with_mod_project(project, root.clone())
        .expect("a non-UTF-8 readme must not fail the pack")
        .pack_to_writer(&mut buffer)
        .unwrap();

    buffer.set_position(0);
    let mut modpkg = Modpkg::mount_from_reader(buffer).unwrap();

    assert_eq!(modpkg.load_readme().unwrap(), b"Caf\xE9 mod");
}

/// Pack -> extract must return the author's exact bytes. This is the property
/// [`meta_chunk_target`](crate::extractor) exists for, and a lossy decode on
/// the pack side would break it while every assertion above still passed.
#[test]
fn license_survives_a_pack_extract_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    let original: &[u8] = b"Copyright \xA9 2026 Someone";
    fs::write(root.join("LICENSE"), original).unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack_to_writer(&mut buffer)
        .unwrap();

    buffer.set_position(0);
    let mut modpkg = Modpkg::mount_from_reader(buffer).unwrap();

    let out = tmp.path().join("extracted");
    crate::ModpkgExtractor::new(&mut modpkg)
        .extract_all(&out)
        .unwrap();

    assert_eq!(fs::read(out.join("LICENSE")).unwrap(), original);
}

// -- modignore tests -------------------------------------------------------

#[test]
fn modignore_excludes_matching_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/tex.dds", b"dds");
    create_content_file(&root, "base", "X.wad.client/src.psd", b"psd");
    fs::write(root.join(".modignore"), "*.psd\n").unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");
    let result = ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);
    assert!(modpkg.chunk_paths.values().any(|p| p == "tex.dds"));
    assert!(!modpkg.chunk_paths.values().any(|p| p == "src.psd"));

    assert_eq!(
        result.ignored_files(),
        [root
            .join("content")
            .join("base")
            .join("X.wad.client")
            .join("src.psd")]
    );
    assert_eq!(result.ignored_count(), 1);
}

#[test]
fn modignore_directory_pattern_prunes_a_subtree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/data/ok.bin", b"ok");
    create_content_file(&root, "base", "X.wad.client/scratch/a.bin", b"a");
    create_content_file(&root, "base", "X.wad.client/scratch/deep/b.bin", b"b");
    fs::write(root.join(".modignore"), "scratch/\n").unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");
    let result = ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);
    assert!(modpkg.chunk_paths.values().any(|p| p == "data/ok.bin"));
    assert!(!modpkg.chunk_paths.values().any(|p| p.contains("scratch")));

    // The directory is recorded once; its files are not enumerated.
    assert_eq!(
        result.ignored_files(),
        [root
            .join("content")
            .join("base")
            .join("X.wad.client")
            .join("scratch")]
    );
}

#[test]
fn modignore_negation_reincludes_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/drop.psd", b"d");
    create_content_file(&root, "base", "X.wad.client/keep.psd", b"k");
    create_content_file(&root, "base", "X.wad.client/tex.dds", b"t");
    fs::write(root.join(".modignore"), "*.psd\n!keep.psd\n").unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");
    let result = ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);
    assert!(modpkg.chunk_paths.values().any(|p| p == "keep.psd"));
    assert!(modpkg.chunk_paths.values().any(|p| p == "tex.dds"));
    assert!(!modpkg.chunk_paths.values().any(|p| p == "drop.psd"));

    assert_eq!(
        result.ignored_files(),
        [root
            .join("content")
            .join("base")
            .join("X.wad.client")
            .join("drop.psd")]
    );
}

#[test]
fn modignore_filters_layer_root_files_and_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "loose.bin", b"b");
    create_content_file(&root, "base", "loose.psd", b"p");
    create_content_file(&root, "base", "notes/todo.txt", b"t");
    fs::write(root.join(".modignore"), "*.psd\nnotes/\n").unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");
    let result = ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);
    assert!(modpkg.chunk_paths.values().any(|p| p == "loose.bin"));
    assert!(!modpkg.chunk_paths.values().any(|p| p == "loose.psd"));
    assert!(!modpkg.chunk_paths.values().any(|p| p.contains("todo")));

    assert_eq!(result.ignored_count(), 2);
}

#[test]
fn modignore_disabled_packs_everything() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/src.psd", b"psd");
    fs::write(root.join(".modignore"), "*.psd\n").unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let options = PackOptions {
        ignore: IgnoreMode::Disabled,
    };
    let output = root.join("build/out.modpkg");
    let result = ProjectPacker::with_mod_project_and_options(project, root.clone(), options)
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);
    assert!(modpkg.chunk_paths.values().any(|p| p == "src.psd"));
    assert_eq!(result.ignored_count(), 0);
}

#[test]
fn absent_modignore_packs_everything() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/tex.dds", b"dds");
    create_content_file(&root, "base", "X.wad.client/src.psd", b"psd");

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");
    let result = ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);
    assert!(modpkg.chunk_paths.values().any(|p| p == "tex.dds"));
    assert!(modpkg.chunk_paths.values().any(|p| p == "src.psd"));
    assert_eq!(result.ignored_count(), 0);
}

/// A directory named like a glob pattern used to be a pack error
/// (`InvalidGlobPattern`); the walker has no pattern to reject.
#[test]
fn glob_special_directory_names_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "[base]/file.bin", b"data");

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);
    assert!(modpkg.chunk_paths.values().any(|p| p == "[base]/file.bin"));
}

#[test]
fn nested_modignore_filters_its_subtree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/tex.dds", b"dds");
    create_content_file(&root, "base", "X.wad.client/src.psd", b"psd");
    create_content_file(&root, "base", "Y.wad.client/src.psd", b"psd");
    fs::write(root.join("content/base/X.wad.client/.modignore"), "*.psd\n").unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");
    let result = ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    // The nested filter governs only its own WAD directory.
    let modpkg = mount_modpkg(&output);
    let x_idx = modpkg.wad_index("x.wad.client").unwrap();
    let y_idx = modpkg.wad_index("y.wad.client").unwrap();
    let base_idx = modpkg.layer_index("base").unwrap();
    assert_eq!(modpkg.chunks_for_wad_layer(x_idx, base_idx).len(), 1);
    assert_eq!(modpkg.chunks_for_wad_layer(y_idx, base_idx).len(), 1);

    assert_eq!(
        result.ignored_files(),
        [root
            .join("content")
            .join("base")
            .join("X.wad.client")
            .join("src.psd")]
    );
}

#[test]
fn modignore_files_are_not_packed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "loose.bin", b"b");
    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    // Ignore files everywhere they can occur: layer root and inside a WAD.
    fs::write(root.join("content/base/.modignore"), "# nothing\n").unwrap();
    fs::write(root.join("content/base/X.wad.client/.modignore"), "\n").unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");
    let result = ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);
    assert!(modpkg.chunk_paths.values().any(|p| p == "loose.bin"));
    assert!(
        !modpkg
            .chunk_paths
            .values()
            .any(|p| p.contains(".modignore")),
        "filter metadata leaked into the archive"
    );
    assert_eq!(result.ignored_count(), 0);
}

#[test]
fn modignore_matching_is_case_insensitive() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/Thumbs.db", b"junk");
    create_content_file(&root, "base", "X.wad.client/tex.dds", b"dds");
    fs::write(root.join(".modignore"), "thumbs.db\n").unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let output = root.join("build/out.modpkg");
    ProjectPacker::with_mod_project(project, root.clone())
        .unwrap()
        .pack(&output)
        .unwrap();

    let modpkg = mount_modpkg(&output);
    assert!(modpkg.chunk_paths.values().any(|p| p == "tex.dds"));
    assert!(!modpkg.chunk_paths.values().any(|p| p == "Thumbs.db"));
}

#[test]
fn explicit_ignore_with_wrong_root_fails_construction() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let elsewhere = root.join("elsewhere");
    let options = PackOptions {
        ignore: IgnoreMode::Explicit(ltk_mod_project::ModIgnore::empty(&elsewhere)),
    };

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let err =
        ProjectPacker::with_mod_project_and_options(project, root.clone(), options).unwrap_err();

    assert!(
        matches!(err, PackError::IgnoreRootMismatch { .. }),
        "expected IgnoreRootMismatch, got: {err}"
    );
}

#[test]
fn invalid_modignore_pattern_fails_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    fs::write(root.join(".modignore"), "a{b\n").unwrap();

    let project = test_mod_project(vec![ModProjectLayer::base()]);
    let err = ProjectPacker::with_mod_project(project, root.clone()).unwrap_err();

    assert!(
        matches!(err, PackError::Ignore(_)),
        "expected an Ignore error, got: {err}"
    );
}

// -- utility tests ---------------------------------------------------------

#[test]
fn test_create_file_name() {
    let project = test_mod_project(vec![]);

    assert_eq!(create_file_name(&project, None), "test-mod_1.0.0.modpkg");
    assert_eq!(
        create_file_name(&project, Some("custom".to_string())),
        "custom.modpkg"
    );
    assert_eq!(
        create_file_name(&project, Some("custom.modpkg".to_string())),
        "custom.modpkg"
    );
}

// -- test helpers ----------------------------------------------------------

fn test_mod_project(layers: Vec<ModProjectLayer>) -> ModProject {
    ModProject {
        name: "test-mod".to_string(),
        display_name: "Test Mod".to_string(),
        version: "1.0.0".to_string(),
        description: String::new(),
        authors: vec![],
        license: None,
        tags: vec![],
        champions: vec![],
        maps: vec![],
        thumbnail: None,
        layers,
        transformers: vec![],
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

fn mount_modpkg(path: &Utf8Path) -> Modpkg<File> {
    let file = File::open(path.as_std_path()).unwrap();
    Modpkg::mount_from_reader(file).unwrap()
}
