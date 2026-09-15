#![cfg(all(feature = "modpkg", feature = "fantome"))]

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::modpkg::{ModpkgFormat, ModpkgImporter};
use ltk_mod_project::{
    game_data::load_layer, ModIgnore, ModProject, ModProjectLayer, ProjectImporter, ProjectPacker,
};
use ltk_modpkg::Modpkg;
use std::{fs, io::Cursor};

fn fixture(root: &Utf8Path) -> ModProject {
    fs::create_dir_all(root.join("content/base/Test.wad.client")).unwrap();
    fs::write(root.join("content/base/game_data.yaml"), "version: 1\nmodules:\n  - target: shared\n    source: Test.wad.client/links.json\n  - target: '0123456789ABCDEF'\n    links: []\n").unwrap();
    fs::write(
        root.join("content/base/Test.wad.client/links.json"),
        r#"{"version":1,"steps":[{"links":["Added"]},{"-links":["Removed"]}]}"#,
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
fn modpkg_round_trip_preserves_steps_and_excludes_sources_from_chunks() {
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
        declarations.modules[0].steps[0].add_links[0].as_str(),
        "Added"
    );
    assert_eq!(
        declarations.modules[0].steps[1].remove_links[0].as_str(),
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
                game_data: Some(declarations.into()),
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
        declarations.modules[0].steps[0].add_links[0].as_str(),
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
        declarations.modules[0].steps[0].add_links[0].as_str(),
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
    assert!(error.to_string().contains("escapes its layer"));
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
    assert!(load_layer(&root, "base", &ignore)
        .declarations
        .unwrap_err()
        .to_string()
        .contains("duplicate canonical"));
}
