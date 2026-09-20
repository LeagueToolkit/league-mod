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
fn refused_declarations_keep_sources_out_of_wad_and_raw_content() {
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
    let result = builder.build().unwrap();
    assert_eq!(result.game_data_diagnostics.len(), 1);
    assert_eq!(
        result.game_data_diagnostics[0].kind,
        ltk_overlay::game_data::GameDataDiagnosticKind::DeclarationsRejected
    );
    assert_eq!(chunk(&overlay.join(WAD), "source.json"), b"game source");
    assert_eq!(chunk(&overlay.join(WAD), "raw-source.json"), b"game raw");
    assert_eq!(chunk(&overlay.join(WAD), "shared"), bin(&["ordinary"]));
}

#[test]
fn archives_patch_game_only_targets_and_preserve_diagnostics_on_cached_builds() {
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
        let declarations = ltk_game_data::load_declarations(
            "game_data.json",
            &r#"{"version":1,"modules":[
            {"target":"TARGET","edits":[{"-links":["Absent"]},{"links":["Added"]}]},
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
                        game_data: Some(declarations.try_into().unwrap()),
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
                            game_data: Some(declarations.try_into().unwrap()),
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
        let reports = first.game_data_diagnostics;
        assert_eq!(reports.len(), 3, "{reports:?}");
        assert!(reports.iter().all(|report| report.mod_id == "mod"
            && report.layer == "base"
            && report.origin.is_some()));
        let cached = builder.build().unwrap();
        assert!(cached.wads_built.is_empty());
        assert_eq!(cached.wads_reused.len(), 1);
        assert_eq!(cached.game_data_diagnostics, reports);
        use ltk_overlay::game_data::GameDataDiagnosticKind;
        assert_eq!(
            reports
                .iter()
                .filter(|d| d.kind == GameDataDiagnosticKind::TargetSkipped)
                .count(),
            2
        );
        let removal = reports
            .iter()
            .find(|d| d.kind == GameDataDiagnosticKind::LinkRemovalUnmatched)
            .unwrap();
        assert_eq!(removal.edit_index, Some(0));
        assert_eq!(removal.chunk, Some(common::hash("shared")));
        let mut saved: serde_json::Value =
            serde_json::from_slice(&fs::read(state.join("overlay.json")).unwrap()).unwrap();
        assert_eq!(saved["gameDataReports"].as_array().unwrap().len(), 3);
        assert_eq!(
            saved["gameDataReports"][0]["origin"]["module"].as_u64(),
            Some(reports[0].origin.as_ref().unwrap().module_index as u64)
        );
        assert!(saved.get("gameDataDiagnostics").is_none());
        // Legacy and future categories retain diagnostic context on exact cache reuse.
        saved["gameDataReports"][0]
            .as_object_mut()
            .unwrap()
            .remove("kind");
        saved["gameDataReports"][1]["kind"] = serde_json::json!("futureCategory");
        fs::write(
            state.join("overlay.json"),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        let compatible = builder.build().unwrap();
        assert!(compatible.wads_built.is_empty());
        assert_eq!(compatible.wads_reused.len(), 1);
        let mut expected = reports;
        expected[0].kind = GameDataDiagnosticKind::Unknown;
        expected[1].kind = GameDataDiagnosticKind::Unknown;
        assert_eq!(compatible.game_data_diagnostics, expected);
        builder.set_enabled_mods(Vec::new());
        assert!(builder.build().unwrap().game_data_diagnostics.is_empty());
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
fn layer_and_mod_order_apply_edits_and_directory_edits_invalidate_output() {
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
    fs::write(path.join("content/extras/game_data.json"), r#"{"version":1,"modules":[{"target":"shared","edits":[{"-links":["base"],"links":["first"]},{"links":["second"]}]},{"target":"shared","-links":["first"]}]}"#).unwrap();
    fs::write(
        path.join("content/disabled/game_data.json"),
        "invalid ignored layer",
    )
    .unwrap();
    top.enabled_layers = Some(["extras".to_owned()].into());
    let mut builder = OverlayBuilder::new(game, overlay.clone(), root.join("state"));
    builder.set_enabled_mods(vec![top, bottom]);
    let result = builder.build().unwrap();
    assert_eq!(chunk(&overlay.join(WAD), "shared"), bin(&["second"]));
    assert!(result.game_data_diagnostics.is_empty());
    let manifest = path.join("content/extras/game_data.json");
    let text = fs::read_to_string(&manifest)
        .unwrap()
        .replace("second", "edited");
    fs::write(manifest, text).unwrap();
    builder.build().unwrap();
    assert_eq!(chunk(&overlay.join(WAD), "shared"), bin(&["edited"]));
}

/// A PROP v3 with `links` declaring `objects` as `(object, class)` pairs, each without properties.
fn bin_declaring(links: &[&str], objects: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = bin(links);
    bytes.truncate(bytes.len() - 4);
    bytes.extend_from_slice(&(objects.len() as u32).to_le_bytes());
    for &(_, class) in objects {
        bytes.extend_from_slice(&class.to_le_bytes());
    }
    for &(object, _) in objects {
        bytes.extend_from_slice(&6u32.to_le_bytes());
        bytes.extend_from_slice(&object.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
    }
    bytes
}

/// The builder with a progress callback recording every stage it reports, in order.
fn recording_stages(
    builder: OverlayBuilder,
) -> (
    OverlayBuilder,
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) {
    let stages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = stages.clone();
    let builder = builder.with_progress(move |progress| {
        let stage = serde_json::to_value(&progress.stage).unwrap();
        sink.lock()
            .unwrap()
            .push(stage.as_str().unwrap().to_owned());
    });
    (builder, stages)
}

#[test]
fn entries_edit_every_declaring_chunk_and_report_fan_out_and_unresolved_names() {
    use ltk_overlay::game_data::GameDataDiagnosticKind;
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let overlay = root.join("overlay");
    let state = root.join("state");
    let skin = ltk_game_data::BinHash::from("Characters/Teemo/Skins/Skin0").0;
    let other = ltk_game_data::BinHash::from("Characters/Ahri/Skins/Skin0").0;
    let one = bin_declaring(&["Game"], &[(skin, 1)]);
    let two = bin_declaring(&[], &[(other, 1), (skin, 1)]);
    let untouched = bin_declaring(&[], &[(other, 1)]);
    common::write_game_wad(
        &game.join(WAD),
        &[
            ("data/one.bin", &one),
            ("data/two.bin", &two),
            ("data/untouched.bin", &untouched),
            ("data/tex", b"not a bin"),
        ],
    );
    let top = project(
        &root,
        "top",
        None,
        Some(
            r#"{"version":1,"modules":[{"entries":{
                "Characters/TEEMO/Skins/Skin0":{"links":["Added"]},
                "Characters/Nobody/Skins/Skin0":{"links":["Nope"]}}}]}"#,
        ),
    );
    fs::create_dir_all(&state).unwrap();
    fs::write(state.join("object_index.bin"), b"unreadable").unwrap();
    let (mut builder, stages) =
        recording_stages(OverlayBuilder::new(game, overlay.clone(), state.clone()));
    builder.set_enabled_mods(vec![top]);
    let first = builder.build().unwrap();
    assert_eq!(first.wads_built.len(), 1);
    assert_ne!(
        fs::read(state.join("object_index.bin")).unwrap(),
        b"unreadable"
    );
    assert_eq!(
        stages
            .lock()
            .unwrap()
            .iter()
            .filter(|stage| *stage == "indexingObjects")
            .count(),
        1
    );
    assert_eq!(
        chunk(&overlay.join(WAD), "data/one.bin"),
        bin_declaring(&["Game", "Added"], &[(skin, 1)])
    );
    assert_eq!(
        chunk(&overlay.join(WAD), "data/two.bin"),
        bin_declaring(&["Added"], &[(other, 1), (skin, 1)])
    );
    assert_eq!(chunk(&overlay.join(WAD), "data/untouched.bin"), untouched);

    let reports = first.game_data_diagnostics;
    assert_eq!(reports.len(), 2, "{reports:?}");
    let fan_out = &reports[0];
    assert_eq!(fan_out.kind, GameDataDiagnosticKind::EntryFanOut);
    assert_eq!(
        fan_out.target.as_deref(),
        Some("Characters/TEEMO/Skins/Skin0")
    );
    assert_eq!(fan_out.origin.as_ref().unwrap().module_index, 0);
    assert_eq!(fan_out.chunk, None);
    for name in ["data/one.bin", "data/two.bin"] {
        let hash = format!("{:016x}", common::hash(name).0);
        assert!(fan_out.message.contains(&hash), "{}", fan_out.message);
    }
    let unresolved = &reports[1];
    assert_eq!(unresolved.kind, GameDataDiagnosticKind::EntryUnresolved);
    assert_eq!(
        unresolved.target.as_deref(),
        Some("Characters/Nobody/Skins/Skin0")
    );
    assert_eq!(unresolved.mod_id, "top");
    assert_eq!(unresolved.layer, "base");

    stages.lock().unwrap().clear();
    let cached = builder.build().unwrap();
    assert!(cached.wads_built.is_empty());
    assert_eq!(cached.wads_reused.len(), 1);
    assert_eq!(cached.game_data_diagnostics, reports);
    assert_eq!(
        stages
            .lock()
            .unwrap()
            .iter()
            .filter(|stage| *stage == "indexingObjects")
            .count(),
        1
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(state.join("overlay.json")).unwrap()).unwrap();
    assert_eq!(saved["gameDataReports"][0]["kind"], "entryFanOut");
    assert_eq!(saved["gameDataReports"][1]["kind"], "entryUnresolved");
}

#[test]
fn a_build_without_entries_never_opens_the_object_index() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let overlay = root.join("overlay");
    let state = root.join("state");
    common::write_game_wad(&game.join(WAD), &[("shared", &bin(&["Game"]))]);
    let top = project(
        &root,
        "top",
        None,
        Some(r#"{"version":1,"modules":[{"target":"shared","links":["Added"]}]}"#),
    );
    let (mut builder, stages) =
        recording_stages(OverlayBuilder::new(game, overlay.clone(), state.clone()));
    builder.set_enabled_mods(vec![top]);
    builder.build().unwrap();
    assert_eq!(chunk(&overlay.join(WAD), "shared"), bin(&["Game", "Added"]));
    assert!(!state.join("object_index.bin").exists());
    assert!(
        !stages
            .lock()
            .unwrap()
            .contains(&"indexingObjects".to_owned())
    );
    assert!(stages.lock().unwrap().contains(&"indexing".to_owned()));
}

#[test]
fn a_called_off_build_ends_without_writing_state() {
    use std::sync::{Arc, Mutex};
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let overlay = root.join("overlay");
    let state = root.join("state");
    let skin = ltk_game_data::BinHash::from("Characters/Teemo/Skins/Skin0").0;
    common::write_game_wad(
        &game.join(WAD),
        &[("data/one.bin", &bin_declaring(&[], &[(skin, 1)]))],
    );
    let manifest = r#"{"version":1,"modules":[{"entries":{"Characters/Teemo/Skins/Skin0":{"links":["Added"]}}}]}"#;

    let mut builder =
        OverlayBuilder::new(game.clone(), overlay.clone(), state.clone()).with_called_off(|| true);
    builder.set_enabled_mods(vec![project(&root, "first", None, Some(manifest))]);
    assert!(matches!(
        builder.build(),
        Err(ltk_overlay::Error::CalledOff)
    ));
    assert!(!state.join("overlay.json").exists());

    let stages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = stages.clone();
    let seen = stages.clone();
    let mut builder = OverlayBuilder::new(game, overlay, state.clone())
        .with_progress(move |progress| {
            let stage = serde_json::to_value(&progress.stage).unwrap();
            sink.lock()
                .unwrap()
                .push(stage.as_str().unwrap().to_owned());
        })
        .with_called_off(move || seen.lock().unwrap().contains(&"indexingObjects".to_owned()));
    builder.set_enabled_mods(vec![project(&root, "second", None, Some(manifest))]);
    assert!(matches!(
        builder.build(),
        Err(ltk_overlay::Error::CalledOff)
    ));
    assert_eq!(
        stages.lock().unwrap().last().map(String::as_str),
        Some("indexingObjects")
    );
    assert!(!state.join("object_index.bin").exists());
    assert!(!state.join("overlay.json").exists());
}

/// A PROP v3 with the dependency `shared` and one object `1` of class `2` whose `speed` is 1.0.
fn speed_bin() -> Vec<u8> {
    use ltk_meta::concrete::{Bin, BinObject, values};
    let bin = Bin::builder()
        .dependency("shared")
        .object(
            BinObject::builder(1u32, 2u32)
                .property(ltk_game_data::BinHash::from("speed"), values::F32::new(1.0))
                .build(),
        )
        .build();
    let mut cursor = Cursor::new(Vec::new());
    bin.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

/// A PTCH setting `speed` to 2.0 on object `1` and on the absent object `9`.
fn speed_ptch() -> Vec<u8> {
    speed_ptch_with(&[1, 9])
}

/// A PTCH setting `speed` to 2.0 on each of `objects`.
fn speed_ptch_with(objects: &[u32]) -> Vec<u8> {
    use ltk_meta::{concrete::values, path::PropertyPath};
    let mut patch = ltk_meta::concrete::BinOverride::builder();
    for &object in objects {
        patch = patch.set(
            object,
            PropertyPath::new("speed").unwrap(),
            values::F32::new(2.0),
        );
    }
    let patch = patch.build();
    let mut cursor = Cursor::new(Vec::new());
    patch.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

/// The `speed` of object `1` and the dependencies of a PROP.
fn speed_and_links(bytes: &[u8]) -> (f32, Vec<String>) {
    use ltk_meta::{concrete::Bin, path::PropertyPath};
    let bin = Bin::from_reader(&mut Cursor::new(bytes)).unwrap();
    let speed = match bin.objects[&ltk_game_data::BinHash(1)]
        .resolve(&PropertyPath::new("speed").unwrap())
    {
        Ok(ltk_meta::PropertyValueEnum::F32(value)) => value.value,
        other => panic!("unexpected speed: {other:?}"),
    };
    (speed, bin.dependencies)
}

fn skipped_record() -> ltk_game_data::SkippedRecord {
    ltk_game_data::SkippedRecord {
        index: 1,
        object: ltk_game_data::BinHash(9),
        property: "speed".into(),
        reason: ltk_game_data::RecordSkipReason::MissingObject,
    }
}

#[test]
fn declared_overrides_rewrite_game_bins_and_report_skipped_records() {
    use ltk_overlay::game_data::GameDataDiagnosticKind;
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let overlay = root.join("overlay");
    common::write_game_wad(&game.join(WAD), &[("shared", &speed_bin())]);
    let top = project(
        &root,
        "top",
        None,
        Some(r#"{"version":1,"modules":[{"target":"shared","source":"patches/skin.yaml"}]}"#),
    );
    let layer = root.join("top/content/base");
    fs::create_dir_all(layer.join("patches")).unwrap();
    fs::write(
        layer.join("patches/skin.yaml"),
        "version: 1\nedits:\n  - overrides: [../speed.ptch]\n    links: [Added]\n  - links: [Later]\n",
    )
    .unwrap();
    fs::write(layer.join("speed.ptch"), speed_ptch()).unwrap();
    let mut builder = OverlayBuilder::new(game, overlay.clone(), root.join("state"));
    builder.set_enabled_mods(vec![top]);
    let result = builder.build().unwrap();
    assert_eq!(
        speed_and_links(&chunk(&overlay.join(WAD), "shared")),
        (
            2.0,
            vec!["shared".to_owned(), "Added".into(), "Later".into()]
        )
    );
    assert_eq!(
        result.game_data_diagnostics.len(),
        1,
        "{:?}",
        result.game_data_diagnostics
    );
    let skipped = &result.game_data_diagnostics[0];
    assert_eq!(skipped.kind, GameDataDiagnosticKind::OverrideRecordSkipped);
    assert_eq!(skipped.chunk, Some(common::hash("shared")));
    assert_eq!(skipped.edit_index, Some(0));
    assert_eq!(skipped.record, Some(skipped_record()));
    assert_eq!(
        skipped
            .origin
            .as_ref()
            .map(|origin| origin.source.as_deref()),
        Some(Some("patches/skin.yaml"))
    );
    // The declared override file is a build resource, never WAD content.
    assert!(
        !overlay.join(WAD).exists() || {
            let wad = Wad::mount(Cursor::new(fs::read(overlay.join(WAD)).unwrap())).unwrap();
            wad.chunks().get(common::hash("speed.ptch")).is_none()
        }
    );
}

#[test]
fn a_clean_override_sets_the_property_and_reports_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let overlay = root.join("overlay");
    common::write_game_wad(&game.join(WAD), &[("shared", &speed_bin())]);
    let top = project(
        &root,
        "top",
        None,
        Some(r#"{"version":1,"modules":[{"target":"shared","overrides":["speed.ptch"]}]}"#),
    );
    fs::write(
        root.join("top/content/base/speed.ptch"),
        speed_ptch_with(&[1]),
    )
    .unwrap();
    let mut builder = OverlayBuilder::new(game, overlay.clone(), root.join("state"));
    builder.set_enabled_mods(vec![top]);
    let result = builder.build().unwrap();
    assert!(
        result.game_data_diagnostics.is_empty(),
        "{:?}",
        result.game_data_diagnostics
    );
    assert_eq!(
        speed_and_links(&chunk(&overlay.join(WAD), "shared")),
        (2.0, vec!["shared".to_owned()])
    );
}

/// An application that returns `Ok` has decoded the base, not landed an edit. A target whose
/// every record skipped is left as the game ships it, and the author is told so. Writing it
/// into the overlay would claim a change that is not there and report nothing.
#[test]
fn a_target_whose_every_edit_skipped_is_not_written_and_says_so() {
    use ltk_overlay::game_data::GameDataDiagnosticKind;

    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let overlay = root.join("overlay");
    common::write_game_wad(&game.join(WAD), &[("shared", &speed_bin())]);
    let top = project(
        &root,
        "top",
        None,
        Some(r#"{"version":1,"modules":[{"target":"shared","overrides":["speed.ptch"]}]}"#),
    );
    // Object 9 is not in the base, so the file's one record skips and nothing lands.
    fs::write(
        root.join("top/content/base/speed.ptch"),
        speed_ptch_with(&[9]),
    )
    .unwrap();
    let mut builder = OverlayBuilder::new(game, overlay.clone(), root.join("state"));
    builder.set_enabled_mods(vec![top]);
    let result = builder.build().unwrap();

    let kinds: Vec<_> = result
        .game_data_diagnostics
        .iter()
        .map(|d| d.kind)
        .collect();
    assert!(
        kinds.contains(&GameDataDiagnosticKind::OverrideRecordSkipped),
        "{:?}",
        result.game_data_diagnostics
    );
    assert!(
        kinds.contains(&GameDataDiagnosticKind::NoEffect),
        "{:?}",
        result.game_data_diagnostics
    );

    let written = overlay.join(WAD);
    let empty = !written.exists() || {
        let wad = Wad::mount(Cursor::new(fs::read(&written).unwrap())).unwrap();
        wad.chunks().get(common::hash("shared")).is_none()
    };
    assert!(empty, "the unchanged chunk was written into the overlay");
}

#[test]
fn archives_apply_packed_overrides_and_report_unreadable_and_invalid_files() {
    use ltk_fantome::{FantomeInfo, FantomeLayerInfo, FantomeWriter};
    use ltk_modpkg::{
        Modpkg, ModpkgLayerMetadata, ModpkgMetadata,
        builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder},
    };
    use ltk_overlay::game_data::GameDataDiagnosticKind;
    use ltk_overlay::{FantomeContent, ModContentProvider, ModpkgContent};
    for format in ["modpkg", "fantome"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
        let game = root.join("game");
        let overlay = root.join("overlay");
        let state = root.join("state");
        common::write_game_wad(&game.join(WAD), &[("shared", &speed_bin())]);
        let declarations = ltk_game_data::load_declarations(
            "game_data.json",
            r#"{"version":1,"modules":[{"target":"shared","overrides":["patches/speed.ptch","prop.ptch","missing.ptch"],"links":["Added"]}]}"#,
            |_| unreachable!(),
        )
        .unwrap();
        let mut archive = Cursor::new(Vec::new());
        if format == "modpkg" {
            ModpkgBuilder::default()
                .with_layer(ModpkgLayerBuilder::base())
                .with_chunk(
                    ModpkgChunkBuilder::new()
                        .with_path("patches/speed.ptch")
                        .with_layer("base"),
                )
                .with_chunk(
                    ModpkgChunkBuilder::new()
                        .with_path("prop.ptch")
                        .with_layer("base"),
                )
                .with_metadata(ModpkgMetadata {
                    name: "overrides".into(),
                    layers: vec![ModpkgLayerMetadata {
                        name: "base".into(),
                        priority: 0,
                        display_name: None,
                        description: None,
                        string_overrides: Default::default(),
                        game_data: Some(declarations.try_into().unwrap()),
                    }],
                    ..Default::default()
                })
                .build_to_writer(&mut archive, |chunk| {
                    Ok(match chunk.path() {
                        "patches/speed.ptch" => speed_ptch(),
                        _ => speed_bin(),
                    })
                })
                .unwrap();
        } else {
            let mut writer = FantomeWriter::new(&mut archive);
            writer
                .write_info(&FantomeInfo {
                    name: "overrides".into(),
                    layers: [(
                        "base".into(),
                        FantomeLayerInfo {
                            name: "base".into(),
                            game_data: Some(declarations.try_into().unwrap()),
                            ..Default::default()
                        },
                    )]
                    .into(),
                    ..Default::default()
                })
                .unwrap();
            writer
                .write_game_data_resource(
                    "base",
                    "patches/speed.ptch",
                    &mut speed_ptch().as_slice(),
                )
                .unwrap();
            writer
                .write_game_data_resource("base", "prop.ptch", &mut speed_bin().as_slice())
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
        assert_eq!(
            speed_and_links(&chunk(&overlay.join(WAD), "shared")),
            (2.0, vec!["shared".to_owned(), "Added".into()]),
            "{format}"
        );
        let reports = first.game_data_diagnostics;
        let kinds: Vec<_> = reports
            .iter()
            .map(|d| (d.kind, d.edit_index, d.record.clone()))
            .collect();
        assert_eq!(
            kinds,
            [
                (
                    GameDataDiagnosticKind::OverrideRecordSkipped,
                    Some(0),
                    Some(skipped_record())
                ),
                (GameDataDiagnosticKind::OverrideInvalid, Some(0), None),
                (GameDataDiagnosticKind::OverrideUnreadable, Some(0), None),
            ],
            "{format}"
        );
        assert!(
            reports
                .iter()
                .all(|d| d.chunk == Some(common::hash("shared")))
        );
        let cached = builder.build().unwrap();
        assert!(cached.wads_built.is_empty(), "{format}");
        assert_eq!(cached.game_data_diagnostics, reports, "{format}");
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(state.join("overlay.json")).unwrap()).unwrap();
        assert_eq!(
            saved["gameDataReports"][0]["record"]["reason"], "missingObject",
            "{format}"
        );
        assert!(
            saved["gameDataReports"][1].get("record").is_none(),
            "{format}"
        );
    }
}

/// A schema typing `speed` as `f32` and `iconAvatar` as `file` on class `2`.
struct TestSchema;

impl ltk_game_data::Schema for TestSchema {
    fn expected(
        &self,
        class: ltk_game_data::BinHash,
        field: ltk_game_data::BinHash,
    ) -> Option<ltk_game_data::Shape> {
        use ltk_game_data::{BinHash, PropertyKind, Shape};
        if class != BinHash(2) {
            return None;
        }
        if field == BinHash::from("speed") {
            Some(Shape::bare(PropertyKind::F32))
        } else if field == BinHash::from("iconAvatar") {
            Some(Shape::bare(PropertyKind::WadChunkLink))
        } else {
            None
        }
    }

    fn has_class(&self, class: ltk_game_data::BinHash) -> bool {
        class == ltk_game_data::BinHash(2)
    }
}

#[test]
fn declared_property_edits_apply_with_and_without_a_schema() {
    use ltk_game_data::{PropertySkipReason, SkippedProperty};
    use ltk_overlay::game_data::GameDataDiagnosticKind;
    // The signed edit sits in its own batch: a report on a key skips the whole key.
    let manifest = "version: 1\nmodules:\n  - target: shared\n    edits:\n      - '0x00000001':\n          speed: 2.5\n          iconAvatar: assets/a.tex\n      - '0x00000001':\n          +speed: [1]\n";
    for with_schema in [true, false] {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
        let game = root.join("game");
        let overlay = root.join("overlay");
        let state = root.join("state");
        common::write_game_wad(&game.join(WAD), &[("shared", &speed_bin())]);
        let top = project(&root, "top", None, None);
        fs::write(root.join("top/content/base/game_data.yaml"), manifest).unwrap();
        let mut builder = OverlayBuilder::new(game, overlay.clone(), state.clone());
        if with_schema {
            builder = builder.with_game_data_schema(TestSchema);
        }
        builder.set_enabled_mods(vec![top]);
        let result = builder.build().unwrap();
        let (speed, _) = speed_and_links(&chunk(&overlay.join(WAD), "shared"));
        assert_eq!(speed, 2.5, "with_schema={with_schema}");
        let kinds: Vec<_> = result
            .game_data_diagnostics
            .iter()
            .map(|d| (d.kind, d.message.clone()))
            .collect();
        let sign = result
            .game_data_diagnostics
            .iter()
            .find(|d| {
                d.kind == GameDataDiagnosticKind::PropertyEditSkipped
                    && d.message.contains("+speed")
            })
            .unwrap_or_else(|| panic!("{kinds:?}"));
        assert_eq!(sign.chunk, Some(common::hash("shared")));
        assert_eq!(sign.edit_index, Some(1));
        assert_eq!(sign.origin.as_ref().unwrap().module_index, 0);
        assert_eq!(
            sign.property,
            Some(SkippedProperty {
                entry: "0x00000001".try_into().unwrap(),
                reason: PropertySkipReason::SignOnScalar,
            })
        );
        let others: Vec<_> = result
            .game_data_diagnostics
            .iter()
            .filter(|d| !std::ptr::eq(*d, sign))
            .map(|d| (d.kind, d.property.as_ref().map(|p| p.reason)))
            .collect();
        if with_schema {
            assert!(others.is_empty(), "{others:?}");
        } else {
            assert_eq!(
                others,
                [
                    (GameDataDiagnosticKind::SchemaFallback, None),
                    (
                        GameDataDiagnosticKind::PropertyEditSkipped,
                        Some(PropertySkipReason::Untypable)
                    ),
                    (GameDataDiagnosticKind::SchemaFallback, None),
                ]
            );
        }
        // A cached build replays the property diagnostics with their typed fields.
        let cached = builder.build().unwrap();
        assert!(cached.wads_built.is_empty());
        assert_eq!(cached.game_data_diagnostics, result.game_data_diagnostics);
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(state.join("overlay.json")).unwrap()).unwrap();
        let reports = saved["gameDataReports"].as_array().unwrap();
        assert!(
            reports.iter().any(|r| r["kind"] == "propertyEditSkipped"
                && r["property"]["reason"] == "signOnScalar"
                && r["property"]["entry"] == "0x00000001"),
            "{reports:?}"
        );
    }
}

#[test]
fn entries_property_edits_reach_every_declaring_chunk() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
    let game = root.join("game");
    let overlay = root.join("overlay");
    let state = root.join("state");
    let skin = ltk_game_data::BinHash::from("Characters/Teemo/Skins/Skin0").0;
    let one = bin_declaring(&[], &[(skin, 2)]);
    let two = bin_declaring(&[], &[(7, 2), (skin, 2)]);
    common::write_game_wad(
        &game.join(WAD),
        &[("data/one.bin", &one), ("data/two.bin", &two)],
    );
    let top = project(&root, "top", None, None);
    fs::write(
        root.join("top/content/base/game_data.yaml"),
        "version: 1\nmodules:\n  - entries:\n      Characters/Teemo/Skins/Skin0:\n        speed: 4\n        links: [Added]\n",
    )
    .unwrap();
    let mut builder =
        OverlayBuilder::new(game, overlay.clone(), state).with_game_data_schema(TestSchema);
    builder.set_enabled_mods(vec![top]);
    let result = builder.build().unwrap();
    use ltk_overlay::game_data::GameDataDiagnosticKind;
    assert!(
        result
            .game_data_diagnostics
            .iter()
            .all(|d| d.kind == GameDataDiagnosticKind::EntryFanOut),
        "{:?}",
        result.game_data_diagnostics
    );
    for name in ["data/one.bin", "data/two.bin"] {
        use ltk_meta::{concrete::Bin, path::PropertyPath};
        let bin = Bin::from_reader(&mut Cursor::new(chunk(&overlay.join(WAD), name))).unwrap();
        let speed = bin.objects[&ltk_game_data::BinHash(skin)]
            .resolve(&PropertyPath::new("speed").unwrap())
            .unwrap();
        assert_eq!(
            *speed,
            ltk_meta::concrete::values::F32::new(4.0).into(),
            "{name}"
        );
        assert_eq!(bin.dependencies, ["Added"], "{name}");
    }
}
