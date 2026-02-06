use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

    /// String overrides for this layer.
    /// Maps field names (from lol.stringtable) to new string values.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub string_overrides: HashMap<String, String>,
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
}
