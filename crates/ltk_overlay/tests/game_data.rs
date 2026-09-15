mod common;

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuilder};
use ltk_wad::Wad;
use std::{fs, io::Cursor};

const WAD: &str = "DATA/FINAL/Champions/Test.wad.client";

fn bin(links: &[&str]) -> Vec<u8> {
    let mut bytes = b"PROP\x03\0\0\0".to_vec();
    bytes.extend_from_slice(&(links.len() as u32).to_le_bytes());
    for link in links {
        bytes.extend_from_slice(&(link.len() as u16).to_le_bytes());
        bytes.extend_from_slice(link.as_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes
}

fn project(root: &Utf8Path, name: &str, base: Option<&[u8]>, manifest: Option<&str>) -> EnabledMod {
    let path = root.join(name);
    fs::create_dir_all(path.join("content/base/Test.wad.client")).unwrap();
    let config = ModProject {
        name: name.into(),
        display_name: name.into(),
        version: "1.0.0".into(),
        layers: vec![ModProjectLayer::base()],
        ..Default::default()
    };
    fs::write(
        path.join("mod.config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    if let Some(base) = base {
        fs::write(path.join("content/base/Test.wad.client/shared"), base).unwrap();
    }
    if let Some(manifest) = manifest {
        fs::write(path.join("content/base/game_data.json"), manifest).unwrap();
    }
    EnabledMod {
        id: name.into(),
        content: Box::new(FsModContent::new(path)),
        enabled_layers: None,
    }
}

fn chunk(path: &Utf8Path, name: &str) -> Vec<u8> {
    let mut wad = Wad::mount(Cursor::new(fs::read(path).unwrap())).unwrap();
    let entry = *wad.chunks().get(common::hash(name)).unwrap();
    wad.load_chunk_decompressed(&entry).unwrap().to_vec()
}

#[test]
fn declarations_use_the_highest_precedence_copy_even_when_it_matches_the_game() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let overlay = root.join("overlay");
    common::write_game_wad(&game.join(WAD), &[("shared", &bin(&["Game"]))]);
    let top = project(
        &root,
        "top",
        Some(&bin(&["Game"])),
        Some(r#"{"version":1,"modules":[{"target":"shared","links":["Added"]}]}"#),
    );
    let bottom = project(&root, "bottom", Some(&bin(&["Lower"])), None);
    let mut builder = OverlayBuilder::new(game, overlay.clone(), root.join("state"));
    builder.set_enabled_mods(vec![top, bottom]);
    builder.build().unwrap();
    assert_eq!(chunk(&overlay.join(WAD), "shared"), bin(&["Game", "Added"]));
}

#[test]
fn refused_programs_keep_sources_out_of_wad_and_raw_content() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let overlay = root.join("overlay");
    common::write_game_wad(
        &game.join(WAD),
        &[
            ("shared", &bin(&[])),
            ("source.json", b"game source"),
            ("raw-source.json", b"game raw"),
        ],
    );
    let mut enabled = project(
        &root,
        "mod",
        Some(&bin(&["ordinary"])),
        Some(
            r#"{"version":1,"modules":[
        {"target":"shared","unsupported":true},
        {"target":"shared","source":"Test.wad.client/source.json"},
        {"target":"shared","source":"raw/raw-source.json"}
    ]}"#,
        ),
    );
    let path = root.join("mod");
    fs::create_dir_all(path.join("content/base/raw")).unwrap();
    let source = r#"{"version":1,"links":["declared"]}"#;
    fs::write(
        path.join("content/base/Test.wad.client/source.json"),
        source,
    )
    .unwrap();
    fs::write(path.join("content/base/raw/raw-source.json"), source).unwrap();
    enabled.content = Box::new(FsModContent::new(path).with_raw_overrides());
    let mut builder = OverlayBuilder::new(game, overlay.clone(), root.join("state"));
    builder.set_enabled_mods(vec![enabled]);
    builder.build().unwrap();
    assert_eq!(builder.game_data_reports().len(), 1);
    assert_eq!(chunk(&overlay.join(WAD), "source.json"), b"game source");
    assert_eq!(chunk(&overlay.join(WAD), "raw-source.json"), b"game raw");
    assert_eq!(chunk(&overlay.join(WAD), "shared"), bin(&["ordinary"]));
}

#[test]
fn archives_patch_game_only_targets_and_preserve_reports_on_cached_builds() {
    use ltk_fantome::{FantomeInfo, FantomeLayerInfo, FantomeWriter};
    use ltk_modpkg::{
        Modpkg, ModpkgLayerMetadata, ModpkgMetadata,
        builder::{ModpkgBuilder, ModpkgLayerBuilder},
    };
    use ltk_overlay::{FantomeContent, ModContentProvider, ModpkgContent};
    for (format, target) in [
        ("modpkg", "shared".to_owned()),
        (
            "fantome",
            format!("{:016X}", ltk_game_data::path_hash("shared")),
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
        let game = root.join("game");
        let overlay = root.join("overlay");
        let state = root.join("state");
        common::write_game_wad(
            &game.join(WAD),
            &[
                ("shared", &bin(&[])),
                ("Added", &bin(&[])),
                ("invalid", b"PTCH"),
            ],
        );
        let program = ltk_game_data::compile(
            "game_data.json",
            &r#"{"version":1,"modules":[
            {"target":"TARGET","steps":[{"-links":["Absent"]},{"links":["Added"]}]},
            {"target":"missing","links":["Added"]},
            {"target":"invalid","links":["Added"]}
        ]}"#
            .replace("TARGET", &target),
            |_| unreachable!(),
        )
        .unwrap();
        let mut archive = Cursor::new(Vec::new());
        if format == "modpkg" {
            ModpkgBuilder::default()
                .with_layer(ModpkgLayerBuilder::base())
                .with_metadata(ModpkgMetadata {
                    name: "declarations".into(),
                    layers: vec![ModpkgLayerMetadata {
                        name: "base".into(),
                        priority: 0,
                        display_name: None,
                        description: None,
                        string_overrides: Default::default(),
                        game_data: Some(program.into()),
                    }],
                    ..Default::default()
                })
                .build_to_writer(&mut archive, |_| Ok(Vec::new()))
                .unwrap();
        } else {
            let mut writer = FantomeWriter::new(&mut archive);
            writer
                .write_info(&FantomeInfo {
                    name: "declarations".into(),
                    layers: [(
                        "base".into(),
                        FantomeLayerInfo {
                            name: "base".into(),
                            game_data: Some(program.into()),
                            ..Default::default()
                        },
                    )]
                    .into(),
                    ..Default::default()
                })
                .unwrap();
            writer.finish().unwrap();
        }
        let archive_path = root.join(format!("mod.{format}"));
        fs::write(&archive_path, archive.get_ref()).unwrap();
        archive.set_position(0);
        let content: Box<dyn ModContentProvider> = if format == "modpkg" {
            Box::new(
                ModpkgContent::new(Modpkg::mount_from_reader(archive).unwrap())
                    .with_archive_path(archive_path),
            )
        } else {
            Box::new(
                FantomeContent::new(archive)
                    .unwrap()
                    .with_archive_path(archive_path),
            )
        };
        let mut builder = OverlayBuilder::new(game, overlay.clone(), state.clone());
        builder.set_enabled_mods(vec![EnabledMod {
            id: "mod".into(),
            content,
            enabled_layers: None,
        }]);
        let first = builder.build().unwrap();
        assert_eq!(first.wads_built.len(), 1, "{format}");
        assert_eq!(chunk(&overlay.join(WAD), "shared"), bin(&["Added"]));
        let reports = builder.game_data_reports().to_vec();
        assert_eq!(reports.len(), 3, "{reports:?}");
        assert!(reports.iter().all(|report| report.mod_id == "mod"
            && report.layer == "base"
            && report.origin.is_some()));
        let cached = builder.build().unwrap();
        assert!(cached.wads_built.is_empty());
        assert_eq!(cached.wads_reused.len(), 1);
        assert_eq!(builder.game_data_reports(), reports);
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(state.join("overlay.json")).unwrap()).unwrap();
        assert_eq!(saved["gameDataReports"].as_array().unwrap().len(), 3);
    }
}

#[test]
fn linked_dependencies_use_literal_game_paths() {
    use ltk_wad::{WadBuilder, WadChunkBuilder};
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let path = game.join(WAD);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut wad = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(WadChunkBuilder::default().with_path("shared"))
        .with_chunk(WadChunkBuilder::default().with_path("0123456789abcdef"))
        .with_chunk(WadChunkBuilder::default().with_path("literal.ltk.bin"))
        .build_to_writer(&mut wad, |_, writer| {
            writer.write_all(&bin(&[]))?;
            Ok(())
        })
        .unwrap();
    fs::write(path, wad.into_inner()).unwrap();
    let enabled = project(
        &root,
        "mod",
        None,
        Some(
            r#"{"version":1,"modules":[{"target":"shared","links":["0123456789abcdef","literal.ltk.bin"]}]}"#,
        ),
    );
    let mut builder = OverlayBuilder::new(game, root.join("overlay"), root.join("state"));
    builder.set_enabled_mods(vec![enabled]);
    builder.build().unwrap();
    assert!(builder.take_linked_bin_offenders().is_empty());
}

#[test]
fn layer_and_mod_order_apply_steps_and_directory_edits_invalidate_output() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let overlay = root.join("overlay");
    common::write_game_wad(&game.join(WAD), &[("shared", &bin(&[]))]);
    let mut top = project(
        &root,
        "top",
        None,
        Some(
            r#"{"version":1,"modules":[{"target":"shared","-links":["lower"],"links":["base"]}]}"#,
        ),
    );
    let bottom = project(
        &root,
        "bottom",
        None,
        Some(r#"{"version":1,"modules":[{"target":"shared","links":["lower"]}]}"#),
    );
    let path = root.join("top");
    let mut config: ModProject =
        serde_json::from_slice(&fs::read(path.join("mod.config.json")).unwrap()).unwrap();
    config.layers.push(ModProjectLayer {
        name: "extras".into(),
        priority: 10,
        ..Default::default()
    });
    config.layers.push(ModProjectLayer {
        name: "disabled".into(),
        priority: 20,
        ..Default::default()
    });
    fs::write(
        path.join("mod.config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(path.join("content/extras")).unwrap();
    fs::create_dir_all(path.join("content/disabled")).unwrap();
    fs::write(path.join("content/extras/game_data.json"), r#"{"version":1,"modules":[{"target":"shared","steps":[{"-links":["base"],"links":["first"]},{"links":["second"]}]},{"target":"shared","-links":["first"]}]}"#).unwrap();
    fs::write(
        path.join("content/disabled/game_data.json"),
        "invalid ignored layer",
    )
    .unwrap();
    top.enabled_layers = Some(["extras".to_owned()].into());
    let mut builder = OverlayBuilder::new(game, overlay.clone(), root.join("state"));
    builder.set_enabled_mods(vec![top, bottom]);
    builder.build().unwrap();
    assert_eq!(chunk(&overlay.join(WAD), "shared"), bin(&["second"]));
    assert!(builder.game_data_reports().is_empty());
    let manifest = path.join("content/extras/game_data.json");
    let text = fs::read_to_string(&manifest)
        .unwrap()
        .replace("second", "edited");
    fs::write(manifest, text).unwrap();
    builder.build().unwrap();
    assert_eq!(chunk(&overlay.join(WAD), "shared"), bin(&["edited"]));
}
