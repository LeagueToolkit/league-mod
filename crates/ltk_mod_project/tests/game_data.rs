#![cfg(all(feature = "modpkg", feature = "fantome"))]

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::modpkg::{ModpkgFormat, ModpkgImporter};
use ltk_mod_project::{
    game_data::load_layer, ModIgnore, ModProject, ModProjectLayer, ProjectImporter, ProjectPacker,
};
use ltk_modpkg::Modpkg;
use std::{fs, io::Cursor};

/// The edits of a `target` module.
fn edits_of(module: &ltk_game_data::Module) -> &[ltk_game_data::Edit] {
    match &module.selector {
        ltk_game_data::Selector::Target { edits, .. } => edits,
        _ => panic!("expected a target selector"),
    }
}

fn fixture(root: &Utf8Path) -> ModProject {
    fs::create_dir_all(root.join("content/base/Test.wad.client")).unwrap();
    fs::write(root.join("content/base/game_data.yaml"), "version: 1\nmodules:\n  - target: shared\n    source: Test.wad.client/links.json\n  - target: '0123456789ABCDEF'\n    links: []\n").unwrap();
    fs::write(
        root.join("content/base/Test.wad.client/links.json"),
        r#"{"version":1,"edits":[{"links":["Added"]},{"-links":["Removed"]}]}"#,
    )
    .unwrap();
    fs::write(
        root.join("content/base/Test.wad.client/content.txt"),
        "game content",
    )
    .unwrap();
    ModProject {
        name: "declarations".into(),
        display_name: "Declarations".into(),
        version: "1.0.0".into(),
        layers: vec![ModProjectLayer::base()],
        ..Default::default()
    }
}

#[test]
fn modpkg_round_trip_preserves_edits_and_excludes_sources_from_chunks() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let source = root.join("source");
    let project = fixture(&source);
    let mut archive = Cursor::new(Vec::new());
    ProjectPacker::new(project, source)
        .pack(ModpkgFormat::new(&mut archive))
        .unwrap();
    archive.set_position(0);
    let mut package = Modpkg::mount_from_reader(archive.clone()).unwrap();
    assert!(package.chunk("links.json", Some("base")).is_err());
    assert!(package.chunk("game_data.yaml", Some("base")).is_err());
    let metadata = package.load_metadata().unwrap();
    let declarations = metadata.layers[0]
        .game_data
        .as_ref()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        edits_of(&declarations.modules[0])[0].links.add[0].as_str(),
        "Added"
    );
    assert_eq!(
        edits_of(&declarations.modules[0])[1].links.remove[0].as_str(),
        "Removed"
    );
    let output = root.join("output");
    ProjectImporter::new(output.clone())
        .import(ModpkgImporter::new(archive))
        .unwrap();
    let extracted = load_layer(&output, "base", &ModIgnore::empty(&output))
        .declarations
        .unwrap()
        .unwrap();
    assert_eq!(
        extracted.manifest_json().unwrap(),
        declarations.manifest_json().unwrap()
    );
    assert_eq!(
        fs::read_to_string(output.join("content/base/test.wad.client/content.txt")).unwrap(),
        "game content"
    );
}

#[test]
fn fantome_import_uses_the_declared_layer_name() {
    use ltk_fantome::{FantomeInfo, FantomeLayerInfo, FantomeWriter};
    use ltk_mod_project::fantome::FantomeImporter;
    let tmp = tempfile::tempdir().unwrap();
    let output = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let declarations = ltk_game_data::load_declarations(
        "game_data.json",
        r#"{"version":1,"modules":[{"target":"shared","links":["Added"]}]}"#,
        |_| unreachable!(),
    )
    .unwrap();
    let info = FantomeInfo {
        name: "Declarations".into(),
        layers: [(
            "alias".into(),
            FantomeLayerInfo {
                name: "base".into(),
                game_data: Some(declarations.try_into().unwrap()),
                ..Default::default()
            },
        )]
        .into(),
        ..Default::default()
    };
    let mut archive = Cursor::new(Vec::new());
    let mut writer = FantomeWriter::new(&mut archive);
    writer.write_info(&info).unwrap();
    writer.finish().unwrap();
    archive.set_position(0);
    let project = ProjectImporter::new(output.clone())
        .import(FantomeImporter::new(archive))
        .unwrap();
    assert!(project.layers.iter().any(|layer| layer.name == "base"));
    let declarations = load_layer(&output, "base", &ModIgnore::empty(&output))
        .declarations
        .unwrap()
        .unwrap();
    assert_eq!(
        edits_of(&declarations.modules[0])[0].links.add[0].as_str(),
        "Added"
    );
    assert!(!output.join("content/alias/game_data.json").exists());
}

#[test]
fn fantome_round_trip_preserves_compiled_sources() {
    use ltk_mod_project::fantome::{FantomeFormat, FantomeImporter};
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let source = root.join("source");
    let project = fixture(&source);
    let mut archive = Cursor::new(Vec::new());
    ProjectPacker::new(project, source)
        .pack(FantomeFormat::new(&mut archive))
        .unwrap();
    archive.set_position(0);
    let mut reader = ltk_fantome::FantomeReader::new(archive.clone()).unwrap();
    let info = reader.read_info().unwrap();
    let declarations = info.layers["base"]
        .game_data
        .as_ref()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        edits_of(&declarations.modules[0])[0].links.add[0].as_str(),
        "Added"
    );
    let output = root.join("output");
    ProjectImporter::new(output.clone())
        .import(FantomeImporter::new(archive))
        .unwrap();
    let extracted = load_layer(&output, "base", &ModIgnore::empty(&output))
        .declarations
        .unwrap()
        .unwrap();
    assert_eq!(
        extracted.manifest_json().unwrap(),
        declarations.manifest_json().unwrap()
    );
    assert!(!output
        .join("content/base/Test.wad.client/links.json")
        .exists());
}

#[test]
fn packing_rejects_ignored_inputs_and_multiple_manifests_before_writing() {
    for multiple in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
        let project = fixture(&root);
        if multiple {
            fs::write(
                root.join("content/base/game_data.toml"),
                "version = 1\nmodules = []",
            )
            .unwrap();
        } else {
            fs::write(root.join(".modignore"), "base/Test.wad.client/\n").unwrap();
        }
        let mut archive = Cursor::new(Vec::new());
        let error = ProjectPacker::new(project, root)
            .pack(ModpkgFormat::new(&mut archive))
            .unwrap_err();
        assert!(
            matches!(error, ltk_mod_project::PackError::GameData(_)),
            "{error}"
        );
        assert!(archive.get_ref().is_empty());
    }
}

#[cfg(unix)]
#[test]
fn source_symlinks_cannot_escape_the_layer() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    fs::create_dir_all(root.join("content/base")).unwrap();
    fs::write(
        root.join("outside.json"),
        r#"{"version":1,"links":["outside"]}"#,
    )
    .unwrap();
    fs::write(
        root.join("content/base/game_data.json"),
        r#"{"version":1,"modules":[{"target":"shared","source":"linked.json"}]}"#,
    )
    .unwrap();
    std::os::unix::fs::symlink(
        root.join("outside.json"),
        root.join("content/base/linked.json"),
    )
    .unwrap();
    let error = load_layer(&root, "base", &ModIgnore::empty(&root))
        .declarations
        .unwrap_err();
    assert_eq!(error.kind, ltk_game_data::ErrorKind::InputEscapes);
}

#[test]
fn source_paths_resolve_within_the_layer_and_reject_duplicate_assignments() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    fixture(&root);
    let manifest = root.join("content/base/game_data.yaml");
    fs::write(&manifest, "version: 1\nmodules:\n- target: shared\n  source: Test.wad.client/../Test.wad.client/links.json\n").unwrap();
    let ignore = ModIgnore::empty(&root);
    assert!(load_layer(&root, "base", &ignore)
        .declarations
        .unwrap()
        .is_some());
    fs::write(&manifest, "version: 1\nmodules:\n- target: shared\n  source: Test.wad.client/links.json\n- target: SHARED\n  source: Test.wad.client/../Test.wad.client/links.json\n").unwrap();
    assert_eq!(
        load_layer(&root, "base", &ignore)
            .declarations
            .unwrap_err()
            .kind,
        ltk_game_data::ErrorKind::DuplicateAssignment
    );
}

/// A `PTCH` with no records.
fn empty_ptch() -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    ltk_meta::BinOverride::new().to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

/// A project whose base layer names two override files, one inside a WAD directory and one
/// through a source in that directory.
fn override_fixture(root: &Utf8Path) -> ModProject {
    fs::create_dir_all(root.join("content/base/Test.wad.client")).unwrap();
    fs::write(
        root.join("content/base/game_data.yaml"),
        "version: 1\nmodules:\n  - target: shared\n    overrides: [Test.wad.client/inner.ptch]\n  - target: other\n    source: Test.wad.client/links.yaml\n",
    )
    .unwrap();
    fs::write(
        root.join("content/base/Test.wad.client/links.yaml"),
        "version: 1\noverrides: [../top.ptch]\nlinks: [Added]\n",
    )
    .unwrap();
    fs::write(
        root.join("content/base/Test.wad.client/inner.ptch"),
        empty_ptch(),
    )
    .unwrap();
    fs::write(root.join("content/base/top.ptch"), empty_ptch()).unwrap();
    fs::write(
        root.join("content/base/Test.wad.client/content.txt"),
        "game content",
    )
    .unwrap();
    ModProject {
        name: "overrides".into(),
        display_name: "Overrides".into(),
        version: "1.0.0".into(),
        layers: vec![ModProjectLayer::base()],
        ..Default::default()
    }
}

fn override_paths(declarations: &ltk_game_data::Declarations) -> Vec<&str> {
    declarations
        .modules
        .iter()
        .flat_map(edits_of)
        .flat_map(|edit| edit.overrides.iter().map(|path| path.as_str()))
        .collect()
}

#[test]
fn override_files_are_inputs_and_load_as_layer_relative_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    override_fixture(&root);
    let layer = load_layer(&root, "base", &ModIgnore::empty(&root));
    let declarations = layer.declarations.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(
        override_paths(declarations),
        ["Test.wad.client/inner.ptch", "top.ptch"]
    );
    let files: Vec<&str> = layer
        .override_files()
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(files, ["Test.wad.client/inner.ptch", "top.ptch"]);
    assert_eq!(
        layer.override_files()[1].source,
        root.join("content/base/top.ptch")
    );
    for input in [
        "content/base/Test.wad.client/inner.ptch",
        "content/base/top.ptch",
        "content/base/Test.wad.client/links.yaml",
    ] {
        assert!(layer.is_declaration_input(&root.join(input)), "{input}");
    }
    assert!(!layer.is_declaration_input(&root.join("content/base/Test.wad.client/content.txt")));

    // A rejected manifest still classifies the override files it and its sources name.
    fs::write(
        root.join("content/base/game_data.yaml"),
        "version: 1\nversion: 1\nmodules:\n  - target: shared\n    overrides: [Test.wad.client/inner.ptch]\n  - target: other\n    source: Test.wad.client/links.yaml\n",
    )
    .unwrap();
    let rejected = load_layer(&root, "base", &ModIgnore::empty(&root));
    assert!(rejected.declarations.is_err());
    assert!(rejected.override_files().is_empty());
    for input in [
        "content/base/Test.wad.client/inner.ptch",
        "content/base/top.ptch",
    ] {
        assert!(rejected.is_declaration_input(&root.join(input)), "{input}");
    }
}

#[test]
fn missing_ignored_and_non_ptch_override_files_refuse_the_declarations() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    override_fixture(&root);
    fs::write(
        root.join("content/base/top.ptch"),
        b"PROP\x03\0\0\0\0\0\0\0\0\0\0\0",
    )
    .unwrap();
    let error = load_layer(&root, "base", &ModIgnore::empty(&root))
        .declarations
        .unwrap_err();
    assert!(error.to_string().contains("top.ptch"), "{error}");
    assert!(error.to_string().contains("PTCH"), "{error}");

    fs::remove_file(root.join("content/base/top.ptch")).unwrap();
    let error = load_layer(&root, "base", &ModIgnore::empty(&root))
        .declarations
        .unwrap_err();
    assert!(error.to_string().contains("top.ptch"), "{error}");

    fs::write(root.join("content/base/top.ptch"), empty_ptch()).unwrap();
    fs::write(root.join("content/.modignore"), "top.ptch\n").unwrap();
    let ignore = ModIgnore::load(&root).unwrap();
    let error = load_layer(&root, "base", &ignore).declarations.unwrap_err();
    assert!(error.to_string().contains("modignore"), "{error}");
}

#[test]
fn override_files_round_trip_through_modpkg() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let source = root.join("source");
    let project = override_fixture(&source);
    let mut archive = Cursor::new(Vec::new());
    ProjectPacker::new(project, source)
        .pack(ModpkgFormat::new(&mut archive))
        .unwrap();
    archive.set_position(0);
    let mut package = Modpkg::mount_from_reader(archive.clone()).unwrap();
    for path in ["Test.wad.client/inner.ptch", "top.ptch"] {
        let chunk = *package.chunk(path, Some("base")).unwrap();
        assert!(chunk.wad().is_none(), "{path}");
        assert_eq!(
            &*package
                .load_chunk_decompressed_by_path(path, Some("base"))
                .unwrap(),
            empty_ptch().as_slice()
        );
    }
    assert!(package.chunk("links.yaml", Some("base")).is_err());
    let declarations = package.load_metadata().unwrap().layers[0]
        .game_data
        .as_ref()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        override_paths(&declarations),
        ["Test.wad.client/inner.ptch", "top.ptch"]
    );
    let output = root.join("output");
    ProjectImporter::new(output.clone())
        .import(ModpkgImporter::new(archive))
        .unwrap();
    let extracted = load_layer(&output, "base", &ModIgnore::empty(&output));
    let extracted_declarations = extracted.declarations.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(
        extracted_declarations.manifest_json().unwrap(),
        declarations.manifest_json().unwrap()
    );
    assert_eq!(extracted.override_files().len(), 2);
    assert_eq!(
        fs::read(output.join("content/base/top.ptch")).unwrap(),
        empty_ptch()
    );
    assert_eq!(
        fs::read(output.join("content/base/Test.wad.client/inner.ptch")).unwrap(),
        empty_ptch()
    );
}

#[test]
fn override_files_round_trip_through_fantome() {
    use ltk_mod_project::fantome::{FantomeFormat, FantomeImporter};
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let source = root.join("source");
    let project = override_fixture(&source);
    let mut archive = Cursor::new(Vec::new());
    ProjectPacker::new(project, source)
        .pack(FantomeFormat::new(&mut archive))
        .unwrap();
    archive.set_position(0);
    let mut reader = ltk_fantome::FantomeReader::new(archive.clone()).unwrap();
    let names: Vec<String> = reader
        .entry_names()
        .filter(|name| name.contains("game_data"))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        names,
        [
            "META/game_data/base/Test.wad.client/inner.ptch",
            "META/game_data/base/top.ptch"
        ]
    );
    assert_eq!(
        reader
            .read_game_data_resource("base", "top.ptch")
            .unwrap()
            .unwrap(),
        empty_ptch()
    );
    assert!(reader
        .read_game_data_resource("base", "absent.ptch")
        .unwrap()
        .is_none());
    let output = root.join("output");
    ProjectImporter::new(output.clone())
        .import(FantomeImporter::new(archive))
        .unwrap();
    let extracted = load_layer(&output, "base", &ModIgnore::empty(&output));
    assert_eq!(
        override_paths(extracted.declarations.as_ref().unwrap().as_ref().unwrap()),
        ["Test.wad.client/inner.ptch", "top.ptch"]
    );
    assert_eq!(
        fs::read(output.join("content/base/Test.wad.client/inner.ptch")).unwrap(),
        empty_ptch()
    );
    assert!(!output
        .join("content/base/Test.wad.client/links.yaml")
        .exists());
}

/// A project whose manifest holds a target body of property edits and an `entries` body
/// mixing links and property edits.
fn property_fixture(root: &Utf8Path) -> ModProject {
    fs::create_dir_all(root.join("content/base")).unwrap();
    fs::write(
        root.join("content/base/game_data.yaml"),
        "version: 1\nmodules:\n  - target: data/characters/teemo/skins/skin0.bin\n    Characters/Teemo/Skins/Skin0:\n      skinMeshProperties:\n        selfIllumination: !f32 1.0\n        +tagEventList: [Jade_Teemo]\n      healthBarData.unitHealthBarStyle: 12\n      ptr: !pointer(X) {a: 1}\n  - entries:\n      Characters/Teemo/Skins/Skin0/Resources:\n        links: [x.bin]\n        +resourceMap:\n          Teemo_R_Mis: !link Characters/Jade_Teemo/Particles/R_Mis\n          Teemo_R_Debuff: null\n",
    )
    .unwrap();
    ModProject {
        name: "properties".into(),
        display_name: "Properties".into(),
        version: "1.0.0".into(),
        layers: vec![ModProjectLayer::base()],
        ..Default::default()
    }
}

#[test]
fn entry_bodies_round_trip_through_both_archives() {
    use ltk_mod_project::fantome::{FantomeFormat, FantomeImporter};
    for format in ["modpkg", "fantome"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
        let source = root.join("source");
        let project = property_fixture(&source);
        let authored = load_layer(&source, "base", &ModIgnore::empty(&source))
            .declarations
            .unwrap()
            .unwrap();
        let mut archive = Cursor::new(Vec::new());
        let packer = ProjectPacker::new(project, source);
        if format == "modpkg" {
            packer.pack(ModpkgFormat::new(&mut archive)).unwrap();
        } else {
            packer.pack(FantomeFormat::new(&mut archive)).unwrap();
        }
        archive.set_position(0);
        let document = if format == "modpkg" {
            let mut package = Modpkg::mount_from_reader(archive.clone()).unwrap();
            package.load_metadata().unwrap().layers[0].game_data.clone()
        } else {
            let mut reader = ltk_fantome::FantomeReader::new(archive.clone()).unwrap();
            reader.read_info().unwrap().layers["base"].game_data.clone()
        };
        let declarations = document.unwrap().parse().unwrap();
        assert_eq!(declarations, authored, "{format}");
        let json = serde_json::to_string(&declarations).unwrap();
        assert!(json.contains(r#""selfIllumination":{"f32":1.0}"#), "{json}");
        assert!(json.contains(r#""Teemo_R_Debuff":null"#), "{json}");
        let output = root.join("output");
        let importer = ProjectImporter::new(output.clone());
        if format == "modpkg" {
            importer.import(ModpkgImporter::new(archive)).unwrap();
        } else {
            importer.import(FantomeImporter::new(archive)).unwrap();
        }
        let extracted = load_layer(&output, "base", &ModIgnore::empty(&output))
            .declarations
            .unwrap()
            .unwrap();
        assert_eq!(
            extracted.manifest_json().unwrap(),
            declarations.manifest_json().unwrap(),
            "{format}"
        );
        let modules: Vec<_> = extracted.modules.iter().map(|m| &m.selector).collect();
        let authored: Vec<_> = authored.modules.iter().map(|m| &m.selector).collect();
        assert_eq!(modules, authored, "{format}");
    }
}

#[test]
fn invalid_entry_bodies_refuse_the_layer_with_typed_kinds() {
    type Expected = fn(&ltk_game_data::ErrorKind) -> bool;
    let cases: [(&str, Expected); 3] = [
        (
            "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      'a[': 1\n",
            |kind: &ltk_game_data::ErrorKind| {
                matches!(kind, ltk_game_data::ErrorKind::InvalidPropertyPath { .. })
            },
        ),
        (
            "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      a: !widget 1\n",
            |kind: &ltk_game_data::ErrorKind| {
                matches!(kind, ltk_game_data::ErrorKind::Syntax { .. })
            },
        ),
        (
            "version: 1\nmodules:\n  - target: a.bin\n    modes: {}\n",
            |kind: &ltk_game_data::ErrorKind| {
                matches!(kind, ltk_game_data::ErrorKind::UnsupportedBinding { .. })
            },
        ),
    ];
    for (manifest, expected) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
        property_fixture(&root);
        fs::write(root.join("content/base/game_data.yaml"), manifest).unwrap();
        let layer = load_layer(&root, "base", &ModIgnore::empty(&root));
        assert!(layer.is_declaration_input(&root.join("content/base/game_data.yaml")));
        let error = layer.declarations.unwrap_err();
        assert!(expected(&error.kind), "{manifest}: {error}");
    }
}

#[test]
fn module_names_round_trip_through_both_archives_and_extraction() {
    use ltk_mod_project::fantome::{FantomeFormat, FantomeImporter};
    let names = |declarations: &ltk_game_data::Declarations| -> Vec<Option<String>> {
        declarations
            .modules
            .iter()
            .map(|module| module.name.as_ref().map(|name| name.as_str().to_owned()))
            .collect()
    };
    let expected = [Some("Teemo recolor".to_owned()), None];
    for format in ["modpkg", "fantome"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
        let source = root.join("source");
        fs::create_dir_all(source.join("content/base")).unwrap();
        fs::write(
            source.join("content/base/game_data.yaml"),
            "version: 1\nmodules:\n  - name: Teemo recolor\n    entries:\n      Characters/Teemo:\n        speed: 2\n  - target: shared\n    links: [x]\n",
        )
        .unwrap();
        let project = ModProject {
            name: "names".into(),
            display_name: "Names".into(),
            version: "1.0.0".into(),
            layers: vec![ModProjectLayer::base()],
            ..Default::default()
        };
        let mut archive = Cursor::new(Vec::new());
        let packer = ProjectPacker::new(project, source);
        if format == "modpkg" {
            packer.pack(ModpkgFormat::new(&mut archive)).unwrap();
        } else {
            packer.pack(FantomeFormat::new(&mut archive)).unwrap();
        }
        archive.set_position(0);
        let document = if format == "modpkg" {
            let mut package = Modpkg::mount_from_reader(archive.clone()).unwrap();
            package.load_metadata().unwrap().layers[0].game_data.clone()
        } else {
            let mut reader = ltk_fantome::FantomeReader::new(archive.clone()).unwrap();
            reader.read_info().unwrap().layers["base"].game_data.clone()
        };
        let carried = document.unwrap().parse().unwrap();
        assert_eq!(names(&carried), expected, "{format}");

        let output = root.join("output");
        let importer = ProjectImporter::new(output.clone());
        if format == "modpkg" {
            importer.import(ModpkgImporter::new(archive)).unwrap();
        } else {
            importer.import(FantomeImporter::new(archive)).unwrap();
        }
        let extracted = load_layer(&output, "base", &ModIgnore::empty(&output))
            .declarations
            .unwrap()
            .unwrap();
        assert_eq!(names(&extracted), expected, "{format}");
    }
}
