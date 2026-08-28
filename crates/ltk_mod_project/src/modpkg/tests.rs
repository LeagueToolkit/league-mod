use super::*;
use crate::{
    ImportError, ImportProgress, ImportStage, ModProject, ModProjectAuthor, ModProjectLayer,
    ModProjectLicense, PackError, PackReport, ProjectImporter, ProjectPacker, ProjectPath,
    ProjectPaths,
};
use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;
use ltk_modpkg::builder::{ModpkgBuilder, ModpkgBuilderError, ModpkgLayerBuilder};
use ltk_modpkg::{
    ChunkKey, ChunkPath, LayerHash, Modpkg, ModpkgCompression, ModpkgLayerMetadata, ModpkgMetadata,
    LICENSE_CHUNK_PATH,
};
use std::fs;
use std::io::Cursor;
use std::sync::atomic::AtomicBool;

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

/// A file shared by several WADs (the game requires it to be byte-identical
/// in all of them) packs as one chunk registered under each WAD.
#[test]
fn same_path_in_two_wads_with_identical_content_packs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(
        &root,
        "base",
        "Aatrox.wad.client/data/shared.bin",
        b"shared",
    );
    create_content_file(&root, "base", "Ahri.wad.client/data/shared.bin", b"shared");

    let (modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let key = ChunkKey::new(
        ChunkPath::new("data/shared.bin").hash(),
        LayerHash::from_name("base"),
    );
    let layer_idx = modpkg.layer_index("base").expect("base layer");
    for wad in ["aatrox.wad.client", "ahri.wad.client"] {
        let wad_idx = modpkg.wad_index(wad).expect("wad in table");
        assert_eq!(
            modpkg.chunks_for_wad_layer(wad_idx, layer_idx),
            [key],
            "{wad} should hold the shared chunk"
        );
    }
}

#[test]
fn same_path_in_two_wads_with_inconsistent_content_fails_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "Aatrox.wad.client/data/shared.bin", b"a");
    create_content_file(&root, "base", "Ahri.wad.client/data/shared.bin", b"b");

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let err = try_pack(project, &root).unwrap_err();
    assert!(
        matches!(
            err,
            PackError::Format(ModpkgPackError::Builder(
                ModpkgBuilderError::InconsistentChunk {
                    ref path,
                    ref first_wad,
                    ref second_wad,
                    ..
                }
            )) if path == "data/shared.bin"
                && first_wad == "aatrox.wad.client"
                && second_wad == "ahri.wad.client"
        ),
        "Expected InconsistentChunk, got: {err}"
    );
}

/// Windows and macOS collapse file-name case, so this collision can only be
/// created on a case-sensitive file system.
#[cfg(target_os = "linux")]
#[test]
fn case_variant_paths_in_one_wad_fail_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    // Chunk paths are case-insensitive, so these hash to the same chunk.
    create_content_file(&root, "base", "Aatrox.wad.client/data/shared.bin", b"a");
    create_content_file(&root, "base", "Aatrox.wad.client/data/SHARED.bin", b"b");

    let project = test_mod_project(vec![ModProjectLayer::base()]);

    let err = try_pack(project, &root).unwrap_err();
    assert!(
        matches!(
            err,
            PackError::Format(ModpkgPackError::DuplicateChunkPath {
                ref rel_path,
                ref layer,
                ..
            }) if rel_path.eq_ignore_ascii_case("data/shared.bin") && layer == "base"
        ),
        "Expected DuplicateChunkPath, got: {err}"
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

    assert_eq!(modpkg.wads().len(), 1);
    assert_eq!(modpkg.wads().values().next().unwrap(), "graves.wad.client");

    let layer_idx = modpkg.layer_index("base").expect("base layer");
    let wad_idx = modpkg.wad_index("graves.wad.client").unwrap();
    assert_eq!(modpkg.chunks_for_wad_layer(wad_idx, layer_idx).len(), 2);

    for path in modpkg.chunk_paths().values() {
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

    assert_eq!(modpkg.wads().len(), 0);
    assert!(modpkg
        .chunk_paths()
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

    assert_eq!(modpkg.wads().len(), 2);
    let wad_names: Vec<&str> = modpkg.wads().values().map(|s| s.as_str()).collect();
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

    let chunk = *modpkg.chunk(LICENSE_CHUNK_PATH, None).unwrap();
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

    let chunk = *modpkg.chunk(LICENSE_CHUNK_PATH, None).unwrap();
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

    assert!(modpkg.chunk_paths().values().any(|p| p == "tex.dds"));
    assert!(!modpkg.chunk_paths().values().any(|p| p == "src.psd"));

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

// -- reading a project back out of a package --------------------------------

/// A package whose header layers and metadata layers are set independently, so
/// a test can tell which of the two a conversion read a field from.
fn build_modpkg(header: &[(&str, i32)], metadata: ModpkgMetadata) -> Modpkg<Cursor<Vec<u8>>> {
    let mut builder = ModpkgBuilder::default().with_metadata(metadata);
    for (name, priority) in header {
        builder = builder.with_layer(
            ModpkgLayerBuilder::new(name)
                .unwrap()
                .with_priority(*priority),
        );
    }

    let mut buffer = Cursor::new(Vec::new());
    builder
        .build_to_writer(&mut buffer, |_| Ok(Vec::new()))
        .unwrap();
    buffer.set_position(0);
    Modpkg::mount_from_reader(buffer).unwrap()
}

fn layer_metadata(name: &str, priority: i32) -> ModpkgLayerMetadata {
    ModpkgLayerMetadata {
        name: name.to_string(),
        display_name: None,
        priority,
        description: None,
        string_overrides: IndexMap::new(),
    }
}

fn layer_names(project: &ModProject) -> Vec<&str> {
    project.layers.iter().map(|l| l.name.as_str()).collect()
}

#[test]
fn read_project_recovers_what_the_pack_stored() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "Test.wad.client/data.bin", b"content");

    let project = ModProject {
        description: "A packed mod".to_string(),
        authors: vec![
            ModProjectAuthor::Name("Alice".to_string()),
            ModProjectAuthor::Role {
                name: "Bob".to_string(),
                role: "Artist".to_string(),
            },
        ],
        license: Some(ModProjectLicense::Spdx("MIT".to_string())),
        tags: vec!["champion-skin".into()],
        champions: vec!["Aatrox".to_string()],
        maps: vec!["summoners-rift".into()],
        ..test_mod_project(vec![ModProjectLayer::base()])
    };

    let (mut packed, _) = pack(project.clone(), &root);
    let read = read_project(&mut packed).unwrap();

    assert_eq!(read.name, project.name);
    assert_eq!(read.display_name, project.display_name);
    assert_eq!(read.version, project.version);
    assert_eq!(read.description, project.description);
    assert_eq!(read.authors, project.authors, "author roles came across");
    assert_eq!(read.license, project.license);
    assert_eq!(read.tags, project.tags);
    assert_eq!(read.champions, project.champions);
    assert_eq!(read.maps, project.maps);
}

/// The header is the source of truth for a layer's priority; the metadata copy
/// is informational and can disagree.
#[test]
fn read_project_takes_layer_priority_from_the_header() {
    let mut packed = build_modpkg(
        &[("base", 0), ("skins", 10)],
        ModpkgMetadata {
            layers: vec![layer_metadata("base", 0), layer_metadata("skins", 999)],
            ..ModpkgMetadata::default()
        },
    );

    let read = read_project(&mut packed).unwrap();

    let skins = read.layers.iter().find(|l| l.name == "skins").unwrap();
    assert_eq!(skins.priority, 10);
}

/// The header carries a layer's name and priority and nothing else, so
/// everything a user typed about the layer has to come off the metadata.
#[test]
fn read_project_keeps_the_metadata_a_header_layer_has_no_room_for() {
    let overrides = IndexMap::from([(
        "en_us".to_string(),
        IndexMap::from([("key".to_string(), "value".to_string())]),
    )]);

    let mut packed = build_modpkg(
        &[("base", 0), ("skins", 10)],
        ModpkgMetadata {
            layers: vec![ModpkgLayerMetadata {
                name: "skins".to_string(),
                display_name: Some("Skins".to_string()),
                priority: 10,
                description: Some("Extra skins".to_string()),
                string_overrides: overrides.clone(),
            }],
            ..ModpkgMetadata::default()
        },
    );

    let read = read_project(&mut packed).unwrap();

    let skins = read.layers.iter().find(|l| l.name == "skins").unwrap();
    assert_eq!(skins.display_name.as_deref(), Some("Skins"));
    assert_eq!(skins.description.as_deref(), Some("Extra skins"));
    assert_eq!(skins.string_overrides, overrides);
}

/// The header's layer table is hashed, so only a sort makes two reads of one
/// package agree. The order matches the one a fantome import produces.
#[test]
fn read_project_orders_layers_base_first_then_by_priority_then_by_name() {
    let mut packed = build_modpkg(
        &[("zed", 5), ("aatrox", 5), ("late", 20), ("base", 0)],
        ModpkgMetadata::default(),
    );

    let read = read_project(&mut packed).unwrap();

    assert_eq!(layer_names(&read), ["base", "aatrox", "zed", "late"]);
}

/// Without a package to read the header from, the metadata's own layer table is
/// all there is.
#[test]
fn a_project_converted_from_metadata_alone_reads_its_layer_table() {
    let metadata = ModpkgMetadata {
        name: "test-mod".to_string(),
        display_name: "Test Mod".to_string(),
        layers: vec![layer_metadata("skins", 10), layer_metadata("base", 0)],
        ..ModpkgMetadata::default()
    };

    let project = ModProject::from(&metadata);

    assert_eq!(project.name, "test-mod");
    assert_eq!(layer_names(&project), ["base", "skins"]);
}

#[test]
fn a_project_converted_from_metadata_naming_no_layer_gets_the_default_base() {
    let project = ModProject::from(&ModpkgMetadata::default());

    assert_eq!(project.layers, crate::ModProjectLayer::default_table());
}

#[test]
fn a_project_converted_from_metadata_reads_a_custom_license() {
    let metadata = ModpkgMetadata {
        license: ltk_modpkg::ModpkgLicense::Custom {
            name: "My License".to_string(),
            url: Some("https://example.com".to_string()),
        },
        ..ModpkgMetadata::default()
    };

    let project = ModProject::from(&metadata);

    assert_eq!(
        project.license,
        Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: Some("https://example.com".to_string()),
        })
    );
}

// -- embedded hashtables ----------------------------------------------------

fn game_table_project(root: &Utf8Path, names: &str) -> ModProject {
    fs::create_dir_all(root.join("hashes")).unwrap();
    fs::write(root.join("hashes/game.hashes.txt"), names).unwrap();

    ModProject {
        hashtables: vec![crate::ModProjectHashtable {
            path: "hashes/game.hashes.txt".to_string(),
            category: ltk_hashtable::Category::Game,
            algorithm: ltk_hashtable::Algorithm::Xxh64,
            bits: 64,
        }],
        ..test_mod_project(vec![ModProjectLayer::base()])
    }
}

/// The declared table becomes a chunk the package's metadata declares, at
/// `_meta_/hashes/` under the file name the project kept it at.
#[test]
fn pack_embeds_the_declared_hashtables() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "Test.wad.client/data.bin", b"content");
    let names = "ASSETS/Custom/One.tex\nASSETS/Custom/Two.tex\n";

    let (mut modpkg, _) = pack(game_table_project(&root, names), &root);

    let tables = modpkg.load_hashtables().unwrap();
    assert_eq!(tables.len(), 1);
    let (entry, table) = &tables[0];
    assert_eq!(entry.path(), "_meta_/hashes/game.hashes.txt");
    assert_eq!(
        table.names().collect::<Vec<_>>(),
        ["ASSETS/Custom/One.tex", "ASSETS/Custom/Two.tex"]
    );
}

/// User story 34: project -> modpkg -> project loses no table names and no
/// casing. The file comes back under `hashes/` and the imported
/// `mod.config.json` declares it where it landed.
#[test]
fn hashtables_survive_a_pack_import_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "Test.wad.client/data.bin", b"content");
    let names = "ASSETS/Custom/CasedName.tex\n";

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::new(game_table_project(&root, names), root.to_owned())
        .pack(ModpkgFormat::new(&mut buffer))
        .unwrap();

    let output = utf8_tempdir(&tmp).join("imported");
    let imported = import(buffer.into_inner(), &output);

    assert_eq!(
        fs::read_to_string(output.join("hashes/game.hashes.txt")).unwrap(),
        names
    );
    assert_eq!(
        imported.hashtables,
        [crate::ModProjectHashtable {
            path: "hashes/game.hashes.txt".to_string(),
            category: ltk_hashtable::Category::Game,
            algorithm: ltk_hashtable::Algorithm::Xxh64,
            bits: 64,
        }]
    );
}

/// User story 35: a modpkg converted to a fantome archive carries its tables
/// as `META/hashes/` entries the info.json declares. There is no direct
/// converter - the path is modpkg -> project -> fantome - so this holds the
/// whole chain together.
#[cfg(feature = "fantome")]
#[test]
fn a_modpkg_converted_through_a_project_carries_its_tables_to_fantome() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "Test.wad.client/data.bin", b"content");
    let names = "ASSETS/Custom/CasedName.tex\n";

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::new(game_table_project(&root, names), root.to_owned())
        .pack(ModpkgFormat::new(&mut buffer))
        .unwrap();

    let output = utf8_tempdir(&tmp).join("imported");
    let imported = import(buffer.into_inner(), &output);

    let mut fantome_buffer = Cursor::new(Vec::new());
    ProjectPacker::new(imported, output)
        .pack(crate::fantome::FantomeFormat::new(&mut fantome_buffer))
        .unwrap();

    fantome_buffer.set_position(0);
    let mut reader = ltk_fantome::FantomeReader::new(fantome_buffer).unwrap();
    let tables = reader.read_hashtables().unwrap();
    assert_eq!(tables.len(), 1);
    let (entry, table) = &tables[0];
    assert_eq!(entry.path(), "META/hashes/game.hashes.txt");
    assert_eq!(
        table.names().collect::<Vec<_>>(),
        ["ASSETS/Custom/CasedName.tex"]
    );
}

/// The trim: a `game` name whose key the package's own chunk table already
/// recovers is not stored twice. Judged on keys, so a name that differs from
/// the stored path only in case is still redundant - the stored path (which
/// keeps its authored casing since ADR 0003) is the surviving copy.
#[test]
fn pack_trims_game_names_the_chunk_table_already_recovers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(
        &root,
        "base",
        "Test.wad.client/ASSETS/Custom/One.tex",
        b"tex",
    );
    // `one` differs from the stored path only in case; `two` is stored by no
    // chunk and must survive.
    let project = game_table_project(&root, "assets/custom/one.tex\nASSETS/Custom/Two.tex\n");

    let (mut modpkg, report) = pack(project, &root);

    let tables = modpkg.load_hashtables().unwrap();
    assert_eq!(
        tables[0].1.names().collect::<Vec<_>>(),
        ["ASSETS/Custom/Two.tex"]
    );
    // A silent trim and an empty table look identical from outside, and only
    // one of them is correct - the count is part of the pack's report.
    assert_eq!(report.trimmed_game_names(), 1);
}

/// `game` only: nothing in a package deduces a `binentries` or `binhashes`
/// name, so trimming any other category would simply delete it.
#[test]
fn the_trim_leaves_every_other_category_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(
        &root,
        "base",
        "Test.wad.client/ASSETS/Custom/One.tex",
        b"tex",
    );
    fs::create_dir_all(root.join("hashes")).unwrap();
    fs::write(
        root.join("hashes/binhashes.hashes.txt"),
        "ASSETS/Custom/One.tex\n",
    )
    .unwrap();

    let project = ModProject {
        hashtables: vec![crate::ModProjectHashtable {
            path: "hashes/binhashes.hashes.txt".to_string(),
            category: ltk_hashtable::Category::BinHashes,
            algorithm: ltk_hashtable::Algorithm::Fnv1a32,
            bits: 32,
        }],
        ..test_mod_project(vec![ModProjectLayer::base()])
    };

    let (mut modpkg, report) = pack(project, &root);

    let tables = modpkg.load_hashtables().unwrap();
    assert_eq!(
        tables[0].1.names().collect::<Vec<_>>(),
        ["ASSETS/Custom/One.tex"]
    );
    assert_eq!(report.trimmed_game_names(), 0);
}

/// One file, two shapes: two manifest entries over one table file survive the
/// modpkg hop as two declarations of one chunk - and the shared chunk is not
/// trimmed, because a name only one shape finds redundant must survive for
/// the others.
#[test]
fn two_declarations_over_one_file_both_survive_the_modpkg_hop() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(
        &root,
        "base",
        "Test.wad.client/ASSETS/Custom/One.tex",
        b"tex",
    );
    // `One.tex` is recoverable from the stored chunk path, so a lone `game`
    // declaration would trim it - the second shape must prevent that.
    fs::create_dir_all(root.join("hashes")).unwrap();
    fs::write(
        root.join("hashes/game.hashes.txt"),
        "ASSETS/Custom/One.tex\n",
    )
    .unwrap();

    let entry = |bits| crate::ModProjectHashtable {
        path: "hashes/game.hashes.txt".to_string(),
        category: ltk_hashtable::Category::Game,
        algorithm: ltk_hashtable::Algorithm::Xxh64,
        bits,
    };
    let project = ModProject {
        hashtables: vec![entry(64), entry(32)],
        ..test_mod_project(vec![ModProjectLayer::base()])
    };

    let (mut modpkg, report) = pack(project, &root);

    let tables = modpkg.load_hashtables().unwrap();
    assert_eq!(tables.len(), 2, "both declarations survive the hop");
    for (_, table) in &tables {
        assert_eq!(table.names().collect::<Vec<_>>(), ["ASSETS/Custom/One.tex"]);
    }
    assert_eq!(
        report.trimmed_game_names(),
        0,
        "a chunk two shapes declare is never trimmed"
    );
}

/// Tables land flat under `_meta_/hashes/` by file name, so two different
/// table files can collide on one archive name. Refused rather than
/// renamed: a silently renamed table would ship under a name nobody chose.
#[test]
fn colliding_table_file_names_fail_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "Test.wad.client/data.bin", b"content");
    fs::create_dir_all(root.join("hashes")).unwrap();
    fs::create_dir_all(root.join("backup")).unwrap();
    fs::write(
        root.join("hashes/game.hashes.txt"),
        "ASSETS/Custom/One.tex\n",
    )
    .unwrap();
    fs::write(
        root.join("backup/game.hashes.txt"),
        "ASSETS/Custom/Two.tex\n",
    )
    .unwrap();

    let entry = |path: &str| crate::ModProjectHashtable {
        path: path.to_string(),
        category: ltk_hashtable::Category::Game,
        algorithm: ltk_hashtable::Algorithm::Xxh64,
        bits: 64,
    };
    let project = ModProject {
        hashtables: vec![
            entry("hashes/game.hashes.txt"),
            entry("backup/game.hashes.txt"),
        ],
        ..test_mod_project(vec![ModProjectLayer::base()])
    };

    let err = try_pack(project, &root).unwrap_err();
    assert!(
        matches!(
            err,
            PackError::Format(ModpkgPackError::DuplicateHashtableName(ref e))
                if e.destination() == "_meta_/hashes/game.hashes.txt"
                    && e.first() == "hashes/game.hashes.txt"
                    && e.second() == "backup/game.hashes.txt"
        ),
        "Expected DuplicateHashtableName, got: {err}"
    );
}

/// Two case-variant declared paths land on one chunk (chunk paths are
/// case-insensitive), but whether they are one file is the filesystem's
/// secret - on a case-sensitive one they can be two files with different
/// bytes. The pack refuses the ambiguous pair on every platform rather
/// than pack differently on different filesystems.
#[test]
fn case_variant_table_declarations_fail_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "Test.wad.client/data.bin", b"content");
    fs::create_dir_all(root.join("hashes")).unwrap();
    fs::write(
        root.join("hashes/Game.hashes.txt"),
        "ASSETS/Custom/One.tex\n",
    )
    .unwrap();
    fs::write(
        root.join("hashes/game.hashes.txt"),
        "ASSETS/Custom/Two.tex\n",
    )
    .unwrap();

    let entry = |path: &str| crate::ModProjectHashtable {
        path: path.to_string(),
        category: ltk_hashtable::Category::Game,
        algorithm: ltk_hashtable::Algorithm::Xxh64,
        bits: 64,
    };
    let project = ModProject {
        hashtables: vec![
            entry("hashes/Game.hashes.txt"),
            entry("hashes/game.hashes.txt"),
        ],
        ..test_mod_project(vec![ModProjectLayer::base()])
    };

    let err = try_pack(project, &root).unwrap_err();
    assert!(
        matches!(
            err,
            PackError::Format(ModpkgPackError::DuplicateHashtableName(ref e))
                if e.destination().eq_ignore_ascii_case("_meta_/hashes/game.hashes.txt")
                    && e.first() == "hashes/Game.hashes.txt"
                    && e.second() == "hashes/game.hashes.txt"
        ),
        "Expected DuplicateHashtableName, got: {err}"
    );
}

/// The escape hatch, packing side: an extraction that cannot name a chunk
/// writes it at the WAD root under the hex of its path hash, so a WAD-root
/// file whose stem is exactly that width is the raw hash, not a path - the
/// chunk must come back under the hash the game knows it by.
#[test]
fn a_wad_root_hex_named_file_packs_as_a_raw_path_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(
        &root,
        "base",
        "Test.wad.client/abcdef1234567890.dds",
        b"tex",
    );

    let (modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let key = ChunkKey::new(
        ltk_modpkg::PathHash::new(0xabcdef1234567890),
        LayerHash::from_name("base"),
    );
    assert!(
        modpkg.chunks().contains_key(&key),
        "the chunk must be keyed by the raw hash the hex name encodes"
    );
}

/// Only the WAD root means raw hash: a hex-looking name inside a directory is
/// a real path, and so is a WAD-root name of any other shape.
#[test]
fn a_nested_hex_named_file_stays_a_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(
        &root,
        "base",
        "Test.wad.client/data/abcdef1234567890.dds",
        b"tex",
    );

    let (modpkg, _) = pack(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let key = ChunkKey::new(
        ChunkPath::new("data/abcdef1234567890.dds").hash(),
        LayerHash::from_name("base"),
    );
    assert!(modpkg.chunks().contains_key(&key));
}

/// A table with an unknown category survives all three hops: unknown means
/// "round-trip verbatim", never "disposable", so project -> modpkg ->
/// project -> fantome keeps its spelling and its names.
#[cfg(feature = "fantome")]
#[test]
fn an_unknown_category_survives_all_three_hops() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "Test.wad.client/data.bin", b"content");
    fs::create_dir_all(root.join("hashes")).unwrap();
    fs::write(
        root.join("hashes/wadnames.hashes.txt"),
        "some/opaque.name\n",
    )
    .unwrap();

    let project = ModProject {
        hashtables: vec![crate::ModProjectHashtable {
            path: "hashes/wadnames.hashes.txt".to_string(),
            category: ltk_hashtable::Category::Unknown("wadnames".to_string()),
            algorithm: ltk_hashtable::Algorithm::Unknown("crc32".to_string()),
            bits: 32,
        }],
        ..test_mod_project(vec![ModProjectLayer::base()])
    };

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::new(project, root.to_owned())
        .pack(ModpkgFormat::new(&mut buffer))
        .unwrap();

    let output = utf8_tempdir(&tmp).join("imported");
    let imported = import(buffer.into_inner(), &output);
    assert_eq!(
        imported.hashtables[0].category,
        ltk_hashtable::Category::Unknown("wadnames".to_string())
    );

    let mut fantome_buffer = Cursor::new(Vec::new());
    ProjectPacker::new(imported, output)
        .pack(crate::fantome::FantomeFormat::new(&mut fantome_buffer))
        .unwrap();

    fantome_buffer.set_position(0);
    let mut reader = ltk_fantome::FantomeReader::new(fantome_buffer).unwrap();
    let tables = reader.read_hashtables().unwrap();
    assert_eq!(tables.len(), 1);
    let (entry, table) = &tables[0];
    assert_eq!(
        *entry.category(),
        ltk_hashtable::Category::Unknown("wadnames".to_string())
    );
    assert_eq!(
        *entry.algorithm(),
        ltk_hashtable::Algorithm::Unknown("crc32".to_string())
    );
    assert_eq!(table.names().collect::<Vec<_>>(), ["some/opaque.name"]);
}

/// An entry whose declared width no key can have is dropped from the imported
/// manifest rather than carried: `PackPlan::hashtables()` refuses an
/// impossible width, so carrying it would import a project that cannot pack.
#[test]
fn an_impossible_width_is_dropped_from_the_imported_manifest() {
    let metadata = ModpkgMetadata {
        hashtables: vec![ltk_modpkg::ModpkgHashtable {
            path: "_meta_/hashes/game.hashes.txt".to_string(),
            category: ltk_hashtable::Category::Game,
            algorithm: ltk_hashtable::Algorithm::Xxh64,
            bits: 0,
        }],
        ..ModpkgMetadata::default()
    };

    assert!(ModProject::from(&metadata).hashtables.is_empty());
}

// -- importing a package as a project ---------------------------------------

/// Pack a project with a base-layer and a second-layer file, plus a readme, and
/// return the archive bytes.
pub(super) fn packed_archive_with_two_layers() -> Vec<u8> {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "Test.wad.client/data.bin", b"base content");
    create_content_file(&root, "skins", "Test.wad.client/data.bin", b"skin content");
    fs::write(root.join("README.md"), "# Packed\n").unwrap();

    let project = test_mod_project(vec![
        ModProjectLayer::base(),
        ModProjectLayer {
            name: "skins".to_string(),
            priority: 10,
            ..Default::default()
        },
    ]);

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::new(project, root.to_owned())
        .pack(ModpkgFormat::new(&mut buffer))
        .unwrap();
    buffer.into_inner()
}

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

fn import(archive: Vec<u8>, output_dir: &Utf8Path) -> ModProject {
    ProjectImporter::new(output_dir)
        .import(ModpkgImporter::new(Cursor::new(archive)))
        .unwrap()
}

/// The WAD directories are named in lowercase on purpose: a package hashes a
/// WAD name lowercased and stores it that way, so the cased name the project
/// was packed from is gone by the time an import reads it back. Naming the
/// cased form here reads fine on a case-insensitive filesystem and is a
/// `NotFound` on Linux.
#[test]
fn import_writes_every_layer_under_the_content_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_tempdir(&tmp).join("imported");

    let imported = import(packed_archive_with_two_layers(), &output);

    assert_eq!(
        layer_names(&imported),
        ["base", "skins"],
        "a package extracts every layer, where a fantome has only the base"
    );
    assert_eq!(
        fs::read(output.join("content/base/test.wad.client/data.bin")).unwrap(),
        b"base content"
    );
    assert_eq!(
        fs::read(output.join("content/skins/test.wad.client/data.bin")).unwrap(),
        b"skin content"
    );
}

/// A project keeps its readme at the root, not beside its content.
#[test]
fn import_writes_the_meta_files_at_the_project_root() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_tempdir(&tmp).join("imported");

    import(packed_archive_with_two_layers(), &output);

    assert_eq!(
        fs::read_to_string(output.join("README.md")).unwrap(),
        "# Packed\n"
    );
    assert!(
        !output.join("content/README.md").exists(),
        "meta files leaked into the content directory"
    );
}

/// An import has to produce a project the packer will take back.
#[test]
fn an_imported_project_packs_again() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_tempdir(&tmp).join("imported");

    import(packed_archive_with_two_layers(), &output);

    ProjectPacker::from_dir(output)
        .unwrap()
        .pack(ModpkgFormat::new(Cursor::new(Vec::new())))
        .unwrap();
}

/// A package's content extracts a layer at a time, so the layer is the unit the
/// progress counts and the boundary a cancellation lands on.
#[test]
fn import_reports_a_stage_for_each_layer_then_one_for_each_step_past_them() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_tempdir(&tmp).join("imported");

    let mut reported = Vec::new();
    ProjectImporter::new(&output)
        .import_with_progress(
            ModpkgImporter::new(Cursor::new(packed_archive_with_two_layers())),
            &mut |progress| reported.push(describe(progress)),
        )
        .unwrap();

    assert_eq!(
        reported,
        [
            ("extracting base".to_owned(), 0, 2),
            ("extracting skins".to_owned(), 1, 2),
            ("writing metadata".to_owned(), 2, 2),
            ("complete".to_owned(), 2, 2),
        ]
    );
}

#[test]
fn a_cancellation_that_answers_true_fails_the_import() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_tempdir(&tmp).join("imported");
    let flag = AtomicBool::new(true);

    let result = ProjectImporter::new(&output)
        .with_cancellation(&flag)
        .import(ModpkgImporter::new(Cursor::new(
            packed_archive_with_two_layers(),
        )));

    assert!(matches!(result, Err(ImportError::Cancelled)));
    assert!(
        !output.join("mod.config.json").exists(),
        "the config is the last thing written, so a cancelled import has none"
    );
}

/// A package's header names every layer the project declared, including one its
/// author had yet to put content in. The import has to give that layer a
/// directory or the project it writes can never be packed again.
#[test]
fn import_of_a_package_with_an_empty_layer_gives_that_layer_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp).join("source");
    create_content_file(&root, "base", "Test.wad.client/data.bin", b"base content");
    fs::create_dir_all(root.join("content/empty")).unwrap();

    let project = test_mod_project(vec![
        ModProjectLayer::base(),
        ModProjectLayer {
            name: "empty".to_string(),
            priority: 5,
            ..Default::default()
        },
    ]);

    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::new(project, root)
        .pack(ModpkgFormat::new(&mut buffer))
        .unwrap();

    let output = utf8_tempdir(&tmp).join("imported");
    let imported = import(buffer.into_inner(), &output);

    assert_eq!(layer_names(&imported), ["base", "empty"]);
    assert!(output.join("content/empty").is_dir());
    ProjectPacker::from_dir(output)
        .unwrap()
        .pack(ModpkgFormat::new(Cursor::new(Vec::new())))
        .unwrap();
}

// -- where an import puts things -------------------------------------------

fn predicted_paths(archive: Vec<u8>) -> Vec<Utf8PathBuf> {
    let modpkg = Modpkg::mount_from_reader(Cursor::new(archive)).unwrap();
    let mut paths: Vec<_> = modpkg
        .extraction_plan()
        .iter_project_paths()
        .map(ProjectPath::into_path)
        .collect();
    paths.sort();
    paths
}

#[test]
fn every_layer_and_the_root_files_are_accounted_for() {
    assert_eq!(
        predicted_paths(packed_archive_with_two_layers()),
        [
            "README.md",
            // Lowercase: the package stores a WAD's name folded, so that is the
            // directory an extraction writes.
            "content/base/test.wad.client/data.bin",
            "content/skins/test.wad.client/data.bin",
        ]
        .map(Utf8PathBuf::from)
    );
}

/// A narrowed plan gives a narrowed answer, so a caller importing one layer can
/// size that layer alone.
#[test]
fn narrowing_the_plan_narrows_the_predicted_paths() {
    let modpkg = Modpkg::mount_from_reader(Cursor::new(packed_archive_with_two_layers()))
        .expect("the packed archive mounts");

    let base: Vec<_> = modpkg
        .extraction_plan()
        .layer("base")
        .iter_project_paths()
        .map(ProjectPath::into_path)
        .collect();
    assert_eq!(base, ["content/base/test.wad.client/data.bin"]);

    let root: Vec<_> = modpkg
        .extraction_plan()
        .root_files()
        .iter_project_paths()
        .map(ProjectPath::into_path)
        .collect();
    assert_eq!(root, ["README.md"]);
}

/// A preflight is only worth having if it agrees with the import, and shared
/// code does not prove that on its own: the `content/` prefix is this crate's
/// and the layout beneath it is the package format's, so the two are checked
/// against each other rather than assumed to line up.
#[test]
fn the_predicted_paths_match_what_an_import_writes() {
    let archive = packed_archive_with_two_layers();
    let predicted = predicted_paths(archive.clone());

    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_tempdir(&tmp).join("imported");
    import(archive, &output);

    for path in &predicted {
        assert!(
            output.join(path).is_file(),
            "{path} was predicted but not written"
        );
    }

    // And nothing was written that was not predicted, config aside: the config
    // is the driver's, not the package's.
    let mut written = Vec::new();
    collect_files(&output, &output, &mut written);
    written.retain(|path| path != "mod.config.json");
    written.sort();

    assert_eq!(written, predicted);
}

fn collect_files(root: &Utf8Path, dir: &Utf8Path, into: &mut Vec<Utf8PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = Utf8PathBuf::from_path_buf(entry.unwrap().path()).unwrap();
        if path.is_dir() {
            collect_files(root, &path, into);
        } else {
            into.push(path.strip_prefix(root).unwrap().to_owned());
        }
    }
}
