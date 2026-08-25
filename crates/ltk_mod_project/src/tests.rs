use super::*;

fn create_example_project() -> ModProject {
    ModProject {
        name: "old-summoners-rift".to_string(),
        display_name: "Old Summoners Rift".to_string(),
        version: "0.1.0-beta.5".to_string(),
        description: "A mod for League of Legends that changes the map to the old Summoners Rift"
            .to_string(),
        authors: vec![
            ModProjectAuthor::Name("TheKillerey".to_string()),
            ModProjectAuthor::Role {
                name: "Crauzer".to_string(),
                role: "Contributor".to_string(),
            },
        ],
        license: Some(ModProjectLicense::Spdx("MIT".to_string())),
        tags: vec![ModTag::Known(WellKnownModTag::MapSkin)],
        champions: vec![],
        maps: vec![ModMap::Known(WellKnownMap::SummonersRift)],
        transformers: vec![FileTransformer {
            name: "tex-converter".to_string(),
            patterns: vec!["**/*.dds".to_string(), "**/*.png".to_string()],
            files: vec![],
            options: None,
        }],
        layers: vec![
            ModProjectLayer {
                name: "base".to_string(),
                display_name: None,
                priority: 0,
                description: Some("Base layer of the mod".to_string()),
                string_overrides: IndexMap::new(),
            },
            ModProjectLayer {
                name: "chroma1".to_string(),
                display_name: Some("Chroma 1".to_string()),
                priority: 20,
                description: Some("Chroma 1".to_string()),
                string_overrides: IndexMap::new(),
            },
        ],
        thumbnail: None,
    }
}

#[test]
fn test_json_parsing() {
    let project: ModProject =
        serde_json::from_str(include_str!("../test-data/mod.config.json")).unwrap();

    assert_eq!(project, create_example_project());
}

#[test]
fn test_toml_parsing() {
    let project: ModProject = toml::from_str(include_str!("../test-data/mod.config.toml")).unwrap();

    assert_eq!(project, create_example_project());
}

#[test]
fn test_thumbnail_optional() {
    // Test that thumbnail is None when not specified
    let config_without_thumbnail = r#"
        {
            "name": "test-mod",
            "display_name": "Test Mod",
            "version": "1.0.0",
            "description": "A test mod",
            "authors": ["Test Author"]
        }
        "#;

    let project: ModProject = serde_json::from_str(config_without_thumbnail).unwrap();
    assert_eq!(project.thumbnail, None);

    // Test that custom thumbnail path is preserved
    let config_with_thumbnail = r#"
        {
            "name": "test-mod",
            "display_name": "Test Mod",
            "version": "1.0.0",
            "description": "A test mod",
            "authors": ["Test Author"],
            "thumbnail": "custom/path.png"
        }
        "#;

    let project: ModProject = serde_json::from_str(config_with_thumbnail).unwrap();
    assert_eq!(project.thumbnail, Some("custom/path.png".to_string()));
}

#[test]
fn test_custom_license_url_optional() {
    let with_url = r#"
        {
            "name": "test-mod",
            "display_name": "Test Mod",
            "version": "1.0.0",
            "description": "A test mod",
            "authors": ["Test Author"],
            "license": { "name": "My License", "url": "https://example.com/terms" }
        }
        "#;

    let project: ModProject = serde_json::from_str(with_url).unwrap();
    assert_eq!(
        project.license,
        Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: Some("https://example.com/terms".to_string()),
        })
    );

    let without_url = r#"
        {
            "name": "test-mod",
            "display_name": "Test Mod",
            "version": "1.0.0",
            "description": "A test mod",
            "authors": ["Test Author"],
            "license": { "name": "My License" }
        }
        "#;

    let project: ModProject = serde_json::from_str(without_url).unwrap();
    assert_eq!(
        project.license,
        Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: None,
        })
    );

    // A URL-less custom license must not emit a null `url` key.
    let json = serde_json::to_value(project.license.as_ref().unwrap()).unwrap();
    assert_eq!(json, serde_json::json!({ "name": "My License" }));
}

#[test]
fn test_custom_license_rejects_unknown_field() {
    let typoed_url = r#"
        {
            "name": "test-mod",
            "display_name": "Test Mod",
            "version": "1.0.0",
            "description": "A test mod",
            "authors": ["Test Author"],
            "license": { "name": "My License", "ur1": "https://example.com/terms" }
        }
        "#;

    // Without deny_unknown_fields this parses as a URL-less custom license
    // and the author's link is silently dropped.
    assert!(
        serde_json::from_str::<ModProject>(typoed_url).is_err(),
        "a misspelled license key must not parse as a URL-less license"
    );
}

#[test]
fn test_custom_license_url_optional_toml() {
    let toml_config = r#"
name = "test-mod"
display_name = "Test Mod"
version = "1.0.0"
description = "A test mod"
authors = ["Test Author"]

[license]
name = "My License"
"#;

    let project: ModProject = toml::from_str(toml_config).unwrap();
    assert_eq!(
        project.license,
        Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: None,
        })
    );

    let toml_config = r#"
name = "test-mod"
display_name = "Test Mod"
version = "1.0.0"
description = "A test mod"
authors = ["Test Author"]

[license]
name = "My License"
url = "https://example.com/terms"
"#;

    let project: ModProject = toml::from_str(toml_config).unwrap();
    assert_eq!(
        project.license,
        Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: Some("https://example.com/terms".to_string()),
        })
    );
}

#[test]
fn test_tags_serialization() {
    let tags = vec![
        ModTag::Known(WellKnownModTag::ChampionSkin),
        ModTag::Known(WellKnownModTag::Sfx),
        ModTag::Custom("my-custom-tag".to_string()),
    ];

    let json = serde_json::to_string(&tags).unwrap();
    assert_eq!(json, r#"["champion-skin","sfx","my-custom-tag"]"#);

    let deserialized: Vec<ModTag> = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, tags);
}

#[test]
fn test_tags_default_empty() {
    let config = r#"
        {
            "name": "test-mod",
            "display_name": "Test Mod",
            "version": "1.0.0",
            "description": "A test mod",
            "authors": ["Test Author"]
        }
        "#;

    let project: ModProject = serde_json::from_str(config).unwrap();
    assert!(project.tags.is_empty());
}

#[test]
fn test_mod_tag_display() {
    assert_eq!(
        ModTag::Known(WellKnownModTag::ChampionSkin).to_string(),
        "champion-skin"
    );
    assert_eq!(
        ModTag::Known(WellKnownModTag::MapSkin).to_string(),
        "map-skin"
    );
    assert_eq!(ModTag::Custom("my-tag".to_string()).to_string(), "my-tag");
}

#[test]
fn test_mod_tag_from_string() {
    assert_eq!(
        ModTag::from("champion-skin".to_string()),
        ModTag::Known(WellKnownModTag::ChampionSkin)
    );
    assert_eq!(
        ModTag::from("sfx".to_string()),
        ModTag::Known(WellKnownModTag::Sfx)
    );
    assert_eq!(
        ModTag::from("my-custom".to_string()),
        ModTag::Custom("my-custom".to_string())
    );
}

#[test]
fn test_mod_map_serialization() {
    let maps = vec![
        ModMap::Known(WellKnownMap::SummonersRift),
        ModMap::Known(WellKnownMap::Aram),
        ModMap::Custom("my-custom-map".to_string()),
    ];

    let json = serde_json::to_string(&maps).unwrap();
    assert_eq!(json, r#"["summoners-rift","aram","my-custom-map"]"#);

    let deserialized: Vec<ModMap> = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, maps);
}

#[test]
fn test_mod_map_display() {
    assert_eq!(
        ModMap::Known(WellKnownMap::SummonersRift).to_string(),
        "summoners-rift"
    );
    assert_eq!(ModMap::Known(WellKnownMap::Arena).to_string(), "arena");
    assert_eq!(ModMap::Custom("my-map".to_string()).to_string(), "my-map");
}

#[test]
fn test_mod_map_from_string() {
    assert_eq!(
        ModMap::from("summoners-rift".to_string()),
        ModMap::Known(WellKnownMap::SummonersRift)
    );
    assert_eq!(
        ModMap::from("arena".to_string()),
        ModMap::Known(WellKnownMap::Arena)
    );
    assert_eq!(
        ModMap::from("custom-map".to_string()),
        ModMap::Custom("custom-map".to_string())
    );
}

/// `as_str` hand-writes what `rename_all = "kebab-case"` derives. The two
/// can drift apart silently, so pin them together: a tag whose `as_str`
/// disagrees with its serialized form would round-trip through a config
/// file as a `Custom` tag with the same text.
#[test]
fn as_str_matches_serde() {
    for tag in WellKnownModTag::ALL {
        assert_eq!(
            serde_json::to_value(tag).unwrap(),
            serde_json::Value::String(tag.as_str().to_string()),
            "{tag:?}"
        );
        assert_eq!(WellKnownModTag::from_name(tag.as_str()), Some(tag));
    }

    for map in WellKnownMap::ALL {
        assert_eq!(
            serde_json::to_value(map).unwrap(),
            serde_json::Value::String(map.as_str().to_string()),
            "{map:?}"
        );
        assert_eq!(WellKnownMap::from_name(map.as_str()), Some(map));
    }
}

/// `ALL` is hand-maintained, so an added variant that is left out of it
/// would be missed by `as_str_matches_serde` above.
#[test]
fn all_covers_every_variant() {
    // Exhaustive matches: adding a variant fails to compile until it is
    // listed here, at which point the length assertions catch `ALL`.
    fn tag_is_listed(tag: WellKnownModTag) -> bool {
        match tag {
            WellKnownModTag::LeagueOfLegends
            | WellKnownModTag::Tft
            | WellKnownModTag::ChampionSkin
            | WellKnownModTag::MapSkin
            | WellKnownModTag::WardSkin
            | WellKnownModTag::Emote
            | WellKnownModTag::SummonerIcon
            | WellKnownModTag::Companion
            | WellKnownModTag::Ui
            | WellKnownModTag::Hud
            | WellKnownModTag::Font
            | WellKnownModTag::Sfx
            | WellKnownModTag::Announcer
            | WellKnownModTag::Structure
            | WellKnownModTag::Minion
            | WellKnownModTag::JungleMonster
            | WellKnownModTag::Misc => true,
        }
    }
    fn map_is_listed(map: WellKnownMap) -> bool {
        match map {
            WellKnownMap::SummonersRift
            | WellKnownMap::Aram
            | WellKnownMap::TeamfightTactics
            | WellKnownMap::Arena
            | WellKnownMap::Swarm => true,
        }
    }

    assert!(WellKnownModTag::ALL.into_iter().all(tag_is_listed));
    assert!(WellKnownMap::ALL.into_iter().all(map_is_listed));

    // Distinct spellings, so no variant is listed twice in place of another.
    let tags: std::collections::HashSet<_> =
        WellKnownModTag::ALL.iter().map(|t| t.as_str()).collect();
    assert_eq!(tags.len(), WellKnownModTag::ALL.len());

    let maps: std::collections::HashSet<_> = WellKnownMap::ALL.iter().map(|m| m.as_str()).collect();
    assert_eq!(maps.len(), WellKnownMap::ALL.len());
}

/// Promoting a tag from `Custom` to `Known` must not change what lands on
/// disk, or existing config files and modpkgs would rewrite themselves.
#[test]
fn promoted_tags_keep_their_serialized_form() {
    for name in ["emote", "summoner-icon", "companion"] {
        let tag = ModTag::from(name);

        assert!(matches!(tag, ModTag::Known(_)), "{name} should be known");
        assert_eq!(serde_json::to_value(&tag).unwrap(), serde_json::json!(name));
        assert_eq!(tag.to_string(), name);
    }
}

fn temp_root(tmp: &tempfile::TempDir) -> camino::Utf8PathBuf {
    camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap()
}

#[test]
fn save_load_round_trips_in_both_formats() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    let project = create_example_project();

    for format in ConfigFormat::ALL {
        let path = root.join(format.file_name());
        project.save(&path).unwrap();

        assert_eq!(
            ModProject::load_from_file(&path).unwrap(),
            project,
            "{format}"
        );
    }
}

#[test]
fn load_prefers_json_over_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let mut json_project = create_example_project();
    json_project.name = "from-json".to_string();
    json_project.save(&root.join("mod.config.json")).unwrap();

    let mut toml_project = create_example_project();
    toml_project.name = "from-toml".to_string();
    toml_project.save(&root.join("mod.config.toml")).unwrap();

    assert_eq!(ModProject::load(&root).unwrap().name, "from-json");
}

#[test]
fn load_reports_the_directory_it_searched() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    match ModProject::load(&root) {
        Err(ModProjectError::ConfigNotFound(dir)) => assert_eq!(dir, root),
        other => panic!("expected ConfigNotFound, got {other:?}"),
    }
}

/// A parse failure has to name the file. A project can hold both a JSON and
/// a TOML config, and "expected `,` at line 4" alone does not say which one
/// to open.
#[test]
fn parse_failure_names_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    let path = root.join("mod.config.json");
    std::fs::write(&path, "{ not json").unwrap();

    match ModProject::load_from_file(&path) {
        Err(ModProjectError::Json { path: failed, .. }) => assert_eq!(failed, path),
        other => panic!("expected Json, got {other:?}"),
    }
}

#[test]
fn missing_file_reports_the_path() {
    let tmp = tempfile::tempdir().unwrap();
    let path = temp_root(&tmp).join("mod.config.json");

    match ModProject::load_from_file(&path) {
        Err(ModProjectError::Io {
            path: failed,
            source,
        }) => {
            assert_eq!(failed, path);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected Io, got {other:?}"),
    }
}

/// The error's own message must not repeat what its source says, or an
/// error chain prints the same sentence twice.
#[test]
fn error_display_does_not_embed_its_source() {
    let tmp = tempfile::tempdir().unwrap();
    let path = temp_root(&tmp).join("mod.config.json");
    let err = ModProject::load_from_file(&path).unwrap_err();

    let source = std::error::Error::source(&err).unwrap().to_string();
    assert!(
        !err.to_string().contains(&source),
        "`{err}` already contains its source `{source}`"
    );
}

#[test]
fn unsupported_extension_names_it() {
    let path = camino::Utf8PathBuf::from("mod.config.yaml");

    match ModProject::load_from_file(&path) {
        Err(ModProjectError::UnsupportedExtension(ext)) => assert_eq!(ext, "yaml"),
        other => panic!("expected UnsupportedExtension, got {other:?}"),
    }
}

#[test]
fn default_supports_struct_update() {
    let project = ModProject {
        name: "only-field-i-care-about".to_string(),
        ..Default::default()
    };

    assert_eq!(project.name, "only-field-i-care-about");
    assert!(project.layers.is_empty());
}

#[test]
fn base_layer_is_recognized() {
    assert!(ModProjectLayer::base().is_base());
    assert_eq!(default_layers(), vec![ModProjectLayer::base()]);

    let other = ModProjectLayer {
        name: "high_res".to_string(),
        ..Default::default()
    };
    assert!(!other.is_base());
}

#[test]
fn non_base_layers_excludes_only_base() {
    let project = create_example_project();

    let names: Vec<&str> = project
        .non_base_layers()
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    assert_eq!(names, ["chroma1"]);
}

#[test]
fn package_file_name_per_format() {
    let project = ModProject {
        name: "test-mod".to_string(),
        version: "1.0.0".to_string(),
        ..Default::default()
    };

    assert_eq!(
        project.package_file_name(None, PackageFormat::Modpkg),
        "test-mod_1.0.0.modpkg"
    );
    assert_eq!(
        project.package_file_name(Some("custom".to_string()), PackageFormat::Modpkg),
        "custom.modpkg"
    );
    assert_eq!(
        project.package_file_name(Some("custom.modpkg".to_string()), PackageFormat::Modpkg),
        "custom.modpkg"
    );
    assert_eq!(
        project.package_file_name(None, PackageFormat::Fantome),
        "test-mod_1.0.0.fantome"
    );
}
