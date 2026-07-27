use camino::Utf8Path;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

mod config_format;
pub mod error;
mod license_file;

pub use config_format::ConfigFormat;
pub use error::{ModProjectError, SerializeError};
pub use license_file::{canonical_license_file_name, find_license_file, LICENSE_FILE_NAMES};

/// Well-known mod tags for common mod categories.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Hash)]
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

impl WellKnownModTag {
    /// Every well-known tag, so a caller can list or validate against them.
    pub const ALL: [WellKnownModTag; 14] = [
        WellKnownModTag::LeagueOfLegends,
        WellKnownModTag::Tft,
        WellKnownModTag::ChampionSkin,
        WellKnownModTag::MapSkin,
        WellKnownModTag::WardSkin,
        WellKnownModTag::Ui,
        WellKnownModTag::Hud,
        WellKnownModTag::Font,
        WellKnownModTag::Sfx,
        WellKnownModTag::Announcer,
        WellKnownModTag::Structure,
        WellKnownModTag::Minion,
        WellKnownModTag::JungleMonster,
        WellKnownModTag::Misc,
    ];

    /// The tag's spelling in a config file.
    ///
    /// Must stay in step with the `rename_all` attribute above;
    /// `as_str_matches_serde` holds it there.
    pub fn as_str(self) -> &'static str {
        match self {
            WellKnownModTag::LeagueOfLegends => "league-of-legends",
            WellKnownModTag::Tft => "tft",
            WellKnownModTag::ChampionSkin => "champion-skin",
            WellKnownModTag::MapSkin => "map-skin",
            WellKnownModTag::WardSkin => "ward-skin",
            WellKnownModTag::Ui => "ui",
            WellKnownModTag::Hud => "hud",
            WellKnownModTag::Font => "font",
            WellKnownModTag::Sfx => "sfx",
            WellKnownModTag::Announcer => "announcer",
            WellKnownModTag::Structure => "structure",
            WellKnownModTag::Minion => "minion",
            WellKnownModTag::JungleMonster => "jungle-monster",
            WellKnownModTag::Misc => "misc",
        }
    }

    /// The tag a config file spelling names, if it names one.
    ///
    /// Not `FromStr`: an unrecognized spelling is not an error here, it is a
    /// [`ModTag::Custom`], so the caller wants an `Option` rather than a
    /// `Result` it has to discard.
    pub fn from_name(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tag| tag.as_str() == value)
    }
}

impl fmt::Display for WellKnownModTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A mod tag, either a well-known category or a custom string.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Hash)]
#[serde(untagged)]
pub enum ModTag {
    Known(WellKnownModTag),
    Custom(String),
}

impl fmt::Display for ModTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModTag::Known(tag) => f.write_str(tag.as_str()),
            ModTag::Custom(s) => f.write_str(s),
        }
    }
}

impl From<&str> for ModTag {
    fn from(s: &str) -> Self {
        match WellKnownModTag::from_name(s) {
            Some(tag) => ModTag::Known(tag),
            None => ModTag::Custom(s.to_owned()),
        }
    }
}

impl From<String> for ModTag {
    fn from(s: String) -> Self {
        // Matches on the borrowed form so a well-known tag does not keep the
        // allocation the caller handed over.
        match WellKnownModTag::from_name(&s) {
            Some(tag) => ModTag::Known(tag),
            None => ModTag::Custom(s),
        }
    }
}

/// Well-known game maps.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum WellKnownMap {
    SummonersRift,
    Aram,
    TeamfightTactics,
    Arena,
    Swarm,
}

impl WellKnownMap {
    /// Every well-known map, so a caller can list or validate against them.
    pub const ALL: [WellKnownMap; 5] = [
        WellKnownMap::SummonersRift,
        WellKnownMap::Aram,
        WellKnownMap::TeamfightTactics,
        WellKnownMap::Arena,
        WellKnownMap::Swarm,
    ];

    /// The map's spelling in a config file.
    ///
    /// Must stay in step with the `rename_all` attribute above;
    /// `as_str_matches_serde` holds it there.
    pub fn as_str(self) -> &'static str {
        match self {
            WellKnownMap::SummonersRift => "summoners-rift",
            WellKnownMap::Aram => "aram",
            WellKnownMap::TeamfightTactics => "teamfight-tactics",
            WellKnownMap::Arena => "arena",
            WellKnownMap::Swarm => "swarm",
        }
    }

    /// The map a config file spelling names, if it names one.
    ///
    /// Not `FromStr`, for the same reason as
    /// [`WellKnownModTag::from_name`].
    pub fn from_name(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|map| map.as_str() == value)
    }
}

impl fmt::Display for WellKnownMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A map identifier, either a well-known map or a custom string.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Hash)]
#[serde(untagged)]
pub enum ModMap {
    Known(WellKnownMap),
    Custom(String),
}

impl fmt::Display for ModMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModMap::Known(map) => f.write_str(map.as_str()),
            ModMap::Custom(s) => f.write_str(s),
        }
    }
}

impl From<&str> for ModMap {
    fn from(s: &str) -> Self {
        match WellKnownMap::from_name(s) {
            Some(map) => ModMap::Known(map),
            None => ModMap::Custom(s.to_owned()),
        }
    }
}

impl From<String> for ModMap {
    fn from(s: String) -> Self {
        match WellKnownMap::from_name(&s) {
            Some(map) => ModMap::Known(map),
            None => ModMap::Custom(s),
        }
    }
}

/// Describes a mod project configuration file
///
/// The `Default` impl exists for struct update syntax
/// (`ModProject { name, ..Default::default() }`), so that a field added here
/// does not break every caller that builds one. A default-constructed project
/// is empty rather than valid, and is not meant to be written out as-is.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
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

impl ModProject {
    /// Load a mod project from a project directory.
    ///
    /// Searches for `mod.config.json` (preferred) then `mod.config.toml`, and
    /// parses the first one found.
    pub fn load(project_dir: &Utf8Path) -> Result<Self, ModProjectError> {
        for format in ConfigFormat::ALL {
            let path = project_dir.join(format.file_name());
            if path.exists() {
                return Self::load_from_file_as(&path, format);
            }
        }

        Err(ModProjectError::ConfigNotFound(project_dir.to_owned()))
    }

    /// Load a mod project from a config file, taking the format from its
    /// extension.
    pub fn load_from_file(path: &Utf8Path) -> Result<Self, ModProjectError> {
        let format = ConfigFormat::from_path(path).ok_or_else(|| {
            ModProjectError::UnsupportedExtension(path.extension().unwrap_or_default().to_owned())
        })?;

        Self::load_from_file_as(path, format)
    }

    /// Load a mod project from a config file parsed as `format`, whatever the
    /// file is named.
    ///
    /// Use this for a config whose name does not carry a usable extension.
    pub fn load_from_file_as(
        path: &Utf8Path,
        format: ConfigFormat,
    ) -> Result<Self, ModProjectError> {
        let content =
            std::fs::read_to_string(path).map_err(|source| ModProjectError::io(path, source))?;

        match format {
            ConfigFormat::Json => {
                serde_json::from_str(&content).map_err(|source| ModProjectError::Json {
                    path: path.to_owned(),
                    source,
                })
            }
            ConfigFormat::Toml => {
                toml::from_str(&content).map_err(|source| ModProjectError::Toml {
                    path: path.to_owned(),
                    source: Box::new(source),
                })
            }
        }
    }

    /// Write the project to a config file, taking the format from its
    /// extension.
    pub fn save(&self, path: &Utf8Path) -> Result<(), ModProjectError> {
        let format = ConfigFormat::from_path(path).ok_or_else(|| {
            ModProjectError::UnsupportedExtension(path.extension().unwrap_or_default().to_owned())
        })?;

        self.save_as(path, format)
    }

    /// Write the project to `path` in `format`, whatever the file is named.
    pub fn save_as(&self, path: &Utf8Path, format: ConfigFormat) -> Result<(), ModProjectError> {
        let content = self.to_config_string(format)?;

        std::fs::write(path, content).map_err(|source| ModProjectError::io(path, source))
    }

    /// Render the project as the text of a config file.
    ///
    /// JSON is pretty-printed, since the result is a file a mod author edits by
    /// hand rather than something only a machine reads.
    pub fn to_config_string(&self, format: ConfigFormat) -> Result<String, ModProjectError> {
        let result = match format {
            ConfigFormat::Json => serde_json::to_string_pretty(self).map_err(SerializeError::Json),
            ConfigFormat::Toml => toml::to_string_pretty(self).map_err(SerializeError::Toml),
        };

        result.map_err(|source| ModProjectError::Serialize { format, source })
    }
}

/// Represents a layer in a mod project
///
/// As with [`ModProject`], `Default` is here for struct update syntax rather
/// than to describe a usable layer: it has no name. Use
/// [`base`](ModProjectLayer::base) for the layer every project starts with.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct ModProjectLayer {
    /// The name of the layer
    /// Must not contain spaces or special characters except for underscores and hyphens
    ///
    /// Example: `base`, `high_res_textures`, `gameplay_overhaul`
    pub name: String,

    /// Optional human-readable display name for the layer
    ///
    /// Example: `Base`, `High Res Textures`, `Gameplay Overhaul`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

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
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub string_overrides: IndexMap<String, IndexMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum ModProjectAuthor {
    Name(String),
    Role { name: String, role: String },
}

/// How a project declares its license terms.
///
/// `url` being optional leaves `name` as the only required field, so without
/// `deny_unknown_fields` a misspelled key would make the object still match
/// `Custom`: `{"name": "X", "ur1": "..."}` would silently parse as a license
/// with no URL instead of failing. Rejecting unknown fields restores the
/// structural check that the required `url` used to provide.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged, deny_unknown_fields)]
pub enum ModProjectLicense {
    Spdx(String),
    Custom {
        name: String,
        /// Optional link to the full terms. A project may name a license and
        /// ship its text in a `LICENSE` file without pointing anywhere.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
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
    /// The name every project's lowest-priority layer carries.
    pub const BASE_NAME: &'static str = "base";

    /// Returns the default base layer
    pub fn base() -> Self {
        Self {
            name: Self::BASE_NAME.to_string(),
            description: Some("Base layer of the mod".to_string()),
            ..Default::default()
        }
    }

    /// Whether this is the base layer, which every project has and which the
    /// Fantome format is limited to.
    pub fn is_base(&self) -> bool {
        self.name == Self::BASE_NAME
    }
}

/// Returns the default layers for a mod project
pub fn default_layers() -> Vec<ModProjectLayer> {
    vec![ModProjectLayer::base()]
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

        let maps: std::collections::HashSet<_> =
            WellKnownMap::ALL.iter().map(|m| m.as_str()).collect();
        assert_eq!(maps.len(), WellKnownMap::ALL.len());
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
}
