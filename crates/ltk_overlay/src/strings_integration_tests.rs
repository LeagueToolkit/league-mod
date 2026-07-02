//! End-to-end tests for string override application through the full
//! `OverlayBuilder::build` pipeline, against a synthetic game directory.

use crate::content::FsModContent;
use crate::fantome_content::FantomeContent;
use crate::strings::{stringtable_chunk_hash, StringOverrideMode};
use crate::{EnabledMod, OverlayBuilder, OverlayStage};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_wad::{Wad, WadBuilder, WadChunkBuilder, WadChunkCompression};
use std::fs;
use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};

fn make_stringtable(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut table = ltk_rst::Stringtable::new();
    for (key, value) in entries {
        table.insert_str(*key, *value);
    }
    let mut out = Vec::new();
    table.to_writer(&mut out).unwrap();
    out
}

/// Write `Localized/Global.{locale}.wad.client` into the game dir, containing a
/// single `data/menu/{locale_lower}/lol.stringtable` chunk.
fn write_game_wad(game_dir: &Utf8Path, locale: &str, table_bytes: Vec<u8>) {
    let localized_dir = game_dir.join("DATA").join("FINAL").join("Localized");
    fs::create_dir_all(localized_dir.as_std_path()).unwrap();

    let chunk_path = format!("data/menu/{}/lol.stringtable", locale.to_ascii_lowercase());
    let mut cursor = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(
            WadChunkBuilder::default()
                .with_path(chunk_path)
                .with_force_compression(WadChunkCompression::None),
        )
        .build_to_writer(&mut cursor, move |_hash, writer| {
            writer.write_all(&table_bytes)?;
            Ok(())
        })
        .unwrap();

    let wad_path = localized_dir.join(format!("Global.{locale}.wad.client"));
    fs::write(wad_path.as_std_path(), cursor.into_inner()).unwrap();
}

fn overlay_stringtable_path(overlay_root: &Utf8Path, locale: &str) -> Utf8PathBuf {
    overlay_root
        .join("DATA")
        .join("FINAL")
        .join("Localized")
        .join(format!("Global.{locale}.wad.client"))
}

/// Mount the patched overlay WAD for `locale` and parse its stringtable chunk.
fn read_overlay_table(overlay_root: &Utf8Path, locale: &str) -> ltk_rst::Stringtable {
    let wad_path = overlay_stringtable_path(overlay_root, locale);
    let file = fs::File::open(wad_path.as_std_path()).unwrap();
    let mut wad = Wad::mount(file).unwrap();
    let chunk = *wad
        .chunks()
        .get(stringtable_chunk_hash(&locale.to_ascii_lowercase()))
        .expect("patched WAD must contain the stringtable chunk");
    let bytes = wad.load_chunk_decompressed(&chunk).unwrap().to_vec();
    ltk_rst::Stringtable::from_reader(&mut Cursor::new(&bytes[..])).unwrap()
}

fn string_layer(buckets: &[(&str, &[(&str, &str)])]) -> ModProjectLayer {
    ModProjectLayer {
        name: "base".to_string(),
        display_name: None,
        priority: 0,
        description: None,
        string_overrides: buckets
            .iter()
            .map(|(locale, entries)| {
                (
                    locale.to_string(),
                    entries
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                )
            })
            .collect(),
    }
}

fn write_mod_dir(root: &Utf8Path, name: &str, layers: Vec<ModProjectLayer>) -> Utf8PathBuf {
    let mod_dir = root.join(name);
    fs::create_dir_all(mod_dir.join("content").join("base").as_std_path()).unwrap();
    let project = ModProject {
        name: name.to_string(),
        display_name: name.to_string(),
        version: "1.0.0".to_string(),
        description: String::new(),
        authors: vec![],
        license: None,
        tags: vec![],
        champions: vec![],
        maps: vec![],
        transformers: vec![],
        layers,
        thumbnail: None,
    };
    fs::write(
        mod_dir.join("mod.config.json").as_std_path(),
        serde_json::to_string_pretty(&project).unwrap(),
    )
    .unwrap();
    mod_dir
}

fn fs_mod(id: &str, mod_dir: Utf8PathBuf) -> EnabledMod {
    EnabledMod {
        id: id.to_string(),
        content: Box::new(FsModContent::new(mod_dir)),
        enabled_layers: None,
    }
}

struct TestEnv {
    _tmp: tempfile::TempDir,
    root: Utf8PathBuf,
    game_dir: Utf8PathBuf,
    profile_dir: Utf8PathBuf,
    overlay_root: Utf8PathBuf,
}

fn test_env() -> TestEnv {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game_dir = root.join("Game");
    fs::create_dir_all(game_dir.join("DATA").join("FINAL").as_std_path()).unwrap();
    let profile_dir = root.join("profile");
    let overlay_root = profile_dir.join("overlay");
    TestEnv {
        _tmp: tmp,
        root,
        game_dir,
        profile_dir,
        overlay_root,
    }
}

#[test]
fn fs_mod_overrides_patch_game_stringtable() {
    let env = test_env();
    write_game_wad(
        &env.game_dir,
        "en_US",
        make_stringtable(&[("game_client_quit", "Quit"), ("untouched", "Original")]),
    );

    let mod_dir = write_mod_dir(
        &env.root,
        "strings-mod",
        vec![string_layer(&[
            ("default", &[("game_client_quit", "Bye")]),
            ("en_us", &[("locale_only", "Locale")]),
        ])],
    );

    let stages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let stages_sink = Arc::clone(&stages);
    let mut builder = OverlayBuilder::new(
        env.game_dir.clone(),
        env.overlay_root.clone(),
        env.profile_dir.clone(),
    )
    .with_string_overrides(StringOverrideMode::Locales(vec!["en_US".to_string()]))
    .with_progress(move |progress| {
        stages_sink
            .lock()
            .unwrap()
            .push(format!("{:?}", progress.stage));
    });
    builder.set_enabled_mods(vec![fs_mod("strings-mod", mod_dir)]);

    let result = builder.build().unwrap();
    assert_eq!(result.wads_built.len(), 1);
    assert!(result.wads_built[0]
        .as_str()
        .ends_with("Global.en_US.wad.client"));
    assert!(stages
        .lock()
        .unwrap()
        .iter()
        .any(|s| s == &format!("{:?}", OverlayStage::ApplyingStringOverrides)));

    let table = read_overlay_table(&env.overlay_root, "en_US");
    assert_eq!(table.get_key("game_client_quit"), Some("Bye"));
    assert_eq!(table.get_key("locale_only"), Some("Locale"));
    assert_eq!(table.get_key("untouched"), Some("Original"));

    // Unchanged config -> exact-match skip reuses the patched WAD.
    let rerun = builder.build().unwrap();
    assert!(rerun.wads_built.is_empty());
    assert_eq!(rerun.wads_reused.len(), 1);

    // Disabling string overrides invalidates the skip and drops the WAD.
    builder = builder.with_string_overrides(StringOverrideMode::Disabled);
    let disabled = builder.build().unwrap();
    assert!(disabled.wads_built.is_empty());
    assert!(!overlay_stringtable_path(&env.overlay_root, "en_US")
        .as_std_path()
        .exists());
}

#[test]
fn all_installed_mode_patches_every_locale() {
    let env = test_env();
    write_game_wad(
        &env.game_dir,
        "en_US",
        make_stringtable(&[("shared_key", "English")]),
    );
    write_game_wad(
        &env.game_dir,
        "ko_KR",
        make_stringtable(&[("shared_key", "Korean")]),
    );

    let mod_dir = write_mod_dir(
        &env.root,
        "strings-mod",
        vec![string_layer(&[
            ("default", &[("shared_key", "Everywhere")]),
            ("ko_kr", &[("korean_only", "KR")]),
        ])],
    );

    let mut builder = OverlayBuilder::new(
        env.game_dir.clone(),
        env.overlay_root.clone(),
        env.profile_dir.clone(),
    )
    .with_string_overrides(StringOverrideMode::AllInstalled);
    builder.set_enabled_mods(vec![fs_mod("strings-mod", mod_dir)]);

    let result = builder.build().unwrap();
    assert_eq!(result.wads_built.len(), 2);

    let en = read_overlay_table(&env.overlay_root, "en_US");
    assert_eq!(en.get_key("shared_key"), Some("Everywhere"));
    assert_eq!(en.get_key("korean_only"), None);

    let ko = read_overlay_table(&env.overlay_root, "ko_KR");
    assert_eq!(ko.get_key("shared_key"), Some("Everywhere"));
    assert_eq!(ko.get_key("korean_only"), Some("KR"));
}

#[test]
fn mod_shipped_stringtable_becomes_patch_base() {
    let env = test_env();
    write_game_wad(
        &env.game_dir,
        "en_US",
        make_stringtable(&[("game_client_quit", "Quit"), ("game_only", "FromGame")]),
    );

    let mod_dir = write_mod_dir(
        &env.root,
        "strings-mod",
        vec![string_layer(&[(
            "default",
            &[("game_client_quit", "Final")],
        )])],
    );
    // The mod also ships a whole replacement stringtable; key-level overrides
    // must apply on top of it, not the game's copy.
    let shipped_dir = mod_dir
        .join("content")
        .join("base")
        .join("Global.en_US.wad.client")
        .join("data")
        .join("menu")
        .join("en_us");
    fs::create_dir_all(shipped_dir.as_std_path()).unwrap();
    fs::write(
        shipped_dir.join("lol.stringtable").as_std_path(),
        make_stringtable(&[("game_client_quit", "ModQuit"), ("mod_only", "FromMod")]),
    )
    .unwrap();

    let mut builder = OverlayBuilder::new(
        env.game_dir.clone(),
        env.overlay_root.clone(),
        env.profile_dir.clone(),
    )
    .with_string_overrides(StringOverrideMode::Locales(vec!["en_us".to_string()]));
    builder.set_enabled_mods(vec![fs_mod("strings-mod", mod_dir)]);

    builder.build().unwrap();

    let table = read_overlay_table(&env.overlay_root, "en_US");
    assert_eq!(table.get_key("game_client_quit"), Some("Final"));
    assert_eq!(table.get_key("mod_only"), Some("FromMod"));
    // The shipped table replaced the game's copy entirely (D4 semantics).
    assert_eq!(table.get_key("game_only"), None);
}

#[test]
fn fantome_string_overrides_are_applied() {
    let env = test_env();
    write_game_wad(
        &env.game_dir,
        "en_US",
        make_stringtable(&[("game_client_quit", "Quit")]),
    );

    let info = serde_json::json!({
        "Name": "Fantome Strings",
        "Author": "Author",
        "Version": "1.0.0",
        "Description": "Desc",
        "Layers": {
            "base": {
                "Name": "base",
                "Priority": 0,
                "StringOverrides": {
                    "default": { "game_client_quit": "FantomeBye" }
                }
            }
        }
    });

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("META/info.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(info.to_string().as_bytes()).unwrap();
    let mut cursor = zip.finish().unwrap();
    cursor.set_position(0);

    let mut builder = OverlayBuilder::new(
        env.game_dir.clone(),
        env.overlay_root.clone(),
        env.profile_dir.clone(),
    )
    .with_string_overrides(StringOverrideMode::Locales(vec!["en_us".to_string()]));
    builder.set_enabled_mods(vec![EnabledMod {
        id: "fantome-strings".to_string(),
        content: Box::new(FantomeContent::new(cursor).unwrap()),
        enabled_layers: None,
    }]);

    builder.build().unwrap();

    let table = read_overlay_table(&env.overlay_root, "en_US");
    assert_eq!(table.get_key("game_client_quit"), Some("FantomeBye"));
}

#[test]
fn missing_locale_wad_is_skipped_without_failing() {
    let env = test_env();
    write_game_wad(
        &env.game_dir,
        "en_US",
        make_stringtable(&[("game_client_quit", "Quit")]),
    );

    let mod_dir = write_mod_dir(
        &env.root,
        "strings-mod",
        vec![string_layer(&[("default", &[("game_client_quit", "Bye")])])],
    );

    let mut builder = OverlayBuilder::new(
        env.game_dir.clone(),
        env.overlay_root.clone(),
        env.profile_dir.clone(),
    )
    .with_string_overrides(StringOverrideMode::Locales(vec![
        "en_us".to_string(),
        "zz_zz".to_string(), // not installed
    ]));
    builder.set_enabled_mods(vec![fs_mod("strings-mod", mod_dir)]);

    let result = builder.build().unwrap();
    assert_eq!(result.wads_built.len(), 1);
    assert_eq!(
        read_overlay_table(&env.overlay_root, "en_US").get_key("game_client_quit"),
        Some("Bye")
    );
}

#[test]
fn higher_priority_mod_wins_string_conflicts() {
    let env = test_env();
    write_game_wad(
        &env.game_dir,
        "en_US",
        make_stringtable(&[("contested", "Original")]),
    );

    let front = write_mod_dir(
        &env.root,
        "front-mod",
        vec![string_layer(&[("default", &[("contested", "Front")])])],
    );
    let back = write_mod_dir(
        &env.root,
        "back-mod",
        vec![string_layer(&[("en_us", &[("contested", "Back")])])],
    );

    let mut builder = OverlayBuilder::new(
        env.game_dir.clone(),
        env.overlay_root.clone(),
        env.profile_dir.clone(),
    )
    .with_string_overrides(StringOverrideMode::Locales(vec!["en_us".to_string()]));
    builder.set_enabled_mods(vec![fs_mod("front-mod", front), fs_mod("back-mod", back)]);

    builder.build().unwrap();

    // Position 0 has the highest priority, even against a locale-specific
    // bucket further down the list — mirroring chunk conflict resolution.
    assert_eq!(
        read_overlay_table(&env.overlay_root, "en_US").get_key("contested"),
        Some("Front")
    );
}

#[test]
fn editing_overrides_rebuilds_only_localized_wad() {
    let env = test_env();
    write_game_wad(
        &env.game_dir,
        "en_US",
        make_stringtable(&[("game_client_quit", "Quit")]),
    );

    let mod_dir = write_mod_dir(
        &env.root,
        "strings-mod",
        vec![string_layer(&[(
            "default",
            &[("game_client_quit", "First")],
        )])],
    );

    let mut builder = OverlayBuilder::new(
        env.game_dir.clone(),
        env.overlay_root.clone(),
        env.profile_dir.clone(),
    )
    .with_string_overrides(StringOverrideMode::Locales(vec!["en_us".to_string()]));
    builder.set_enabled_mods(vec![fs_mod("strings-mod", mod_dir.clone())]);
    builder.build().unwrap();

    // Edit the override value; the state's mod list still matches, but the
    // per-WAD fingerprint (derived from the merged override map) changed.
    write_mod_dir(
        &env.root,
        "strings-mod",
        vec![string_layer(&[(
            "default",
            &[("game_client_quit", "Second")],
        )])],
    );

    // The exact-match skip keys on the enabled-mod id list, so an in-place
    // config edit needs a changed id (or a forced rebuild) to get past it —
    // same as any other in-place content edit. With the skip bypassed, the
    // per-WAD fingerprint (derived from the merged override map) must pick up
    // the change and rebuild exactly the localized WAD.
    builder.set_enabled_mods(vec![fs_mod("strings-mod-v2", mod_dir)]);
    let result = builder.build().unwrap();
    assert_eq!(result.wads_built.len(), 1);
    assert_eq!(
        read_overlay_table(&env.overlay_root, "en_US").get_key("game_client_quit"),
        Some("Second")
    );
}
