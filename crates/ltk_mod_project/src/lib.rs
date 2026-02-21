use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

fn serde_fmt<T: Serialize>(value: &T, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let json = serde_json::to_string(value).map_err(|_| fmt::Error)?;
    let s: String = serde_json::from_str(&json).map_err(|_| fmt::Error)?;
    f.write_str(&s)
}

/// Well-known mod tags for common mod categories.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum WellKnownModTag {
    LeagueOfLegends,
    Tft,
    ChampionSkin,
    MapSkin,
    WardSkin,
    Ui,
    Hud,
    Font,
    Sfx,
    Announcer,
    Structure,
    Minion,
    JungleMonster,
    Misc,
}

/// A mod tag, either a well-known category or a custom string.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum ModTag {
    Known(WellKnownModTag),
    Custom(String),
}

impl fmt::Display for ModTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModTag::Known(tag) => serde_fmt(tag, f),
            ModTag::Custom(s) => f.write_str(s),
        }
    }
}

impl From<String> for ModTag {
    fn from(s: String) -> Self {
        serde_json::from_value(serde_json::Value::String(s.clone())).unwrap_or(ModTag::Custom(s))
    }
}

/// Well-known game maps.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum WellKnownMap {
    SummonersRift,
    Aram,
    TeamfightTactics,
    Arena,
    Swarm,
}

/// A map identifier, either a well-known map or a custom string.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum ModMap {
    Known(WellKnownMap),
    Custom(String),
}

impl fmt::Display for ModMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModMap::Known(map) => serde_fmt(map, f),
            ModMap::Custom(s) => f.write_str(s),
        }
    }
}

impl From<String> for ModMap {
    fn from(s: String) -> Self {
        serde_json::from_value(serde_json::Value::String(s.clone())).unwrap_or(ModMap::Custom(s))
    }
}

/// Describes a mod project configuration file
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ModProject {
    /// The name of the mod
    /// Must not contain spaces or special characters except for underscores and hyphens
    ///
    /// Example: `my_mod`
    pub name: String,

    /// The display name of the mod.
    ///
    /// Example: `My Mod`
    pub display_name: String,

    /// The version of the mod
    ///
    /// Example: `1.0.0`
    pub version: String,

    /// The description of the mod
    ///
    /// Example: `This is a mod for my game`
    pub description: String,

    /// The authors of the mod
    pub authors: Vec<ModProjectAuthor>,

    /// The license of the mod
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<ModProjectLicense>,

    /// Tags/categories for the mod (e.g., "champion-skin", "sfx")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<ModTag>,

    /// Champions this mod targets (e.g., "Aatrox", "Ahri")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub champions: Vec<String>,

    /// Maps this mod targets (e.g., "summoners-rift", "howling-abyss")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<ModMap>,

    /// File transformers to be applied during the build process
    /// Optional field - if not provided, no transformers will be applied
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transformers: Vec<FileTransformer>,

    /// Layers of the mod project
    /// Layers are loaded in order of priority (highest priority last)
    /// If not specified, a default "base" layer with priority 0 is assumed
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<ModProjectLayer>,

    /// The thumbnail file path relative to the mod project folder
    /// Optional field - if not specified, default thumbnail will be used
    ///
    /// Example: `thumbnail.webp`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
}

/// Represents a layer in a mod project
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ModProjectLayer {
    /// The name of the layer
    /// Must not contain spaces or special characters except for underscores and hyphens
    ///
    /// Example: `base`, `high_res_textures`, `gameplay_overhaul`
    pub name: String,

    /// The priority of the layer
    /// Higher priority layers override lower priority layers when they modify the same files
    /// Default is 0 for the base layer
    pub priority: i32,

    /// Optional description of the layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// String overrides for this layer, organized by locale.
    /// Outer key: locale (e.g., "en_us", "ko_kr", "zh_cn", or "default" for all locales)
    /// Inner map: field name (from lol.stringtable) -> new string value
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub string_overrides: HashMap<String, HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(untagged)]
pub enum ModProjectAuthor {
    Name(String),
    Role { name: String, role: String },
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(untagged)]
pub enum ModProjectLicense {
    Spdx(String),
    Custom { name: String, url: String },
}

/// Represents a file transformer that can be applied to files during the build process
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct FileTransformer {
    /// The name of the transformer to use.
    pub name: String,

    /// File patterns to apply this transformer to.
    /// At least one of `patterns` or `files` must be provided
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,

    /// Specific files to apply this transformer to.
    /// At least one of `patterns` or `files` must be provided
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,

    /// Transformer-specific configuration
    /// This is an optional field that can be used to configure the transformer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<FileTransformerOptions>,
}

pub type FileTransformerOptions = HashMap<String, serde_json::Value>;

impl ModProjectLayer {
    /// Returns the default base layer
    pub fn base() -> Self {
        Self {
            name: "base".to_string(),
            priority: 0,
            description: Some("Base layer of the mod".to_string()),
            string_overrides: HashMap::new(),
        }
    }
}

/// Returns the default layers for a mod project
pub fn default_layers() -> Vec<ModProjectLayer> {
    vec![ModProjectLayer {
        name: "base".to_string(),
        priority: 0,
        description: Some("Base layer of the mod".to_string()),
        string_overrides: HashMap::new(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_example_project() -> ModProject {
        ModProject {
            name: "old-summoners-rift".to_string(),
            display_name: "Old Summoners Rift".to_string(),
            version: "0.1.0-beta.5".to_string(),
            description:
                "A mod for League of Legends that changes the map to the old Summoners Rift"
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
                    priority: 0,
                    description: Some("Base layer of the mod".to_string()),
                    string_overrides: HashMap::new(),
                },
                ModProjectLayer {
                    name: "chroma1".to_string(),
                    priority: 20,
                    description: Some("Chroma 1".to_string()),
                    string_overrides: HashMap::new(),
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
        let project: ModProject =
            toml::from_str(include_str!("../test-data/mod.config.toml")).unwrap();

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
}
