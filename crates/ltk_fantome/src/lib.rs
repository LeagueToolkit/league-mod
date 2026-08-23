//! Reading and writing the legacy Fantome (`.fantome`) archive format.
//!
//! This crate handles the format itself: the metadata shapes stored in
//! `META/info.json`, [`FantomeWriter`] for producing archives, and
//! [`FantomeReader`] for consuming them. Turning a mod *project* into an
//! archive (and back) lives in `ltk_mod_project`'s `fantome` module, which
//! composes the primitives here.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod error;
mod hashtable;
mod reader;
mod writer;

pub use error::{FantomeExtractError, FantomeWriteError, WadHashtableError};
pub use hashtable::{WadHashtable, format_chunk_path_hash};
pub use reader::FantomeReader;
pub use writer::FantomeWriter;

/// Fantome metadata structure that goes into info.json
#[derive(Serialize, Deserialize, Debug)]
pub struct FantomeInfo {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Author")]
    pub author: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Description")]
    pub description: String,
    /// The license the mod is distributed under.
    ///
    /// Independent of the `META/LICENSE` archive entry: this names the terms,
    /// that entry carries their text. Either, both, or neither may be present.
    #[serde(rename = "License", default, skip_serializing_if = "Option::is_none")]
    pub license: Option<FantomeLicense>,
    /// Tags/categories for the mod (e.g., "champion-skin", "sfx").
    #[serde(rename = "Tags", default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Champions this mod targets (e.g., "Aatrox", "Ahri").
    #[serde(rename = "Champions", default, skip_serializing_if = "Vec::is_empty")]
    pub champions: Vec<String>,
    /// Maps this mod targets (e.g., "Summoner's Rift", "Howling Abyss").
    #[serde(rename = "Maps", default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<String>,
    /// Per-layer metadata including string overrides.
    #[serde(rename = "Layers", default, skip_serializing_if = "HashMap::is_empty")]
    pub layers: HashMap<String, FantomeLayerInfo>,
}

/// The license declaration in a Fantome info.json.
///
/// Either a bare SPDX identifier (`"License": "MIT"`) or an object naming the
/// license with an optional link (`"License": {"Name": "...", "Url": "..."}`).
/// Note the PascalCase inner keys: this is a distinct shape from
/// `ltk_mod_project`'s license object, whose keys are `{name, url}`.
///
/// Unknown fields are rejected: with `Url` optional, `Name` is the only
/// required key, so an object with a misspelled `Url` would otherwise still
/// match `Custom` and quietly lose the link.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum FantomeLicense {
    Spdx(String),
    Custom {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "Url", default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
}

/// Per-layer metadata in a Fantome info.json.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FantomeLayerInfo {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(
        rename = "DisplayName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub display_name: Option<String>,
    #[serde(rename = "Priority")]
    pub priority: i32,
    /// String overrides for this layer, organized by locale.
    /// Outer key: locale (e.g., "en_us", "ko_kr", or "default")
    /// Inner map: field name -> replacement string
    #[serde(
        rename = "StringOverrides",
        default,
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub string_overrides: IndexMap<String, IndexMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info_json(license: Option<FantomeLicense>) -> serde_json::Value {
        let info = FantomeInfo {
            name: "Test Mod".to_string(),
            author: "Alice".to_string(),
            version: "1.0.0".to_string(),
            description: "A test mod".to_string(),
            license,
            tags: vec![],
            champions: vec![],
            maps: vec![],
            layers: HashMap::new(),
        };
        serde_json::to_value(&info).unwrap()
    }

    #[test]
    fn info_json_emits_spdx_license() {
        let json = info_json(Some(FantomeLicense::Spdx("MIT".to_string())));
        assert_eq!(json["License"], serde_json::json!("MIT"));
    }

    #[test]
    fn info_json_emits_custom_license() {
        let json = info_json(Some(FantomeLicense::Custom {
            name: "My License".to_string(),
            url: Some("https://example.com/terms".to_string()),
        }));
        assert_eq!(
            json["License"],
            serde_json::json!({ "Name": "My License", "Url": "https://example.com/terms" })
        );

        let json = info_json(Some(FantomeLicense::Custom {
            name: "My License".to_string(),
            url: None,
        }));
        assert_eq!(json["License"], serde_json::json!({ "Name": "My License" }));
    }

    #[test]
    fn info_json_omits_absent_license() {
        let json = info_json(None);
        assert!(
            json.get("License").is_none(),
            "License key must be omitted entirely, got: {json}"
        );
    }

    #[test]
    fn legacy_info_json_without_license_still_parses() {
        let legacy = r#"{
            "Name": "Old Mod",
            "Author": "Someone",
            "Version": "1.0.0",
            "Description": "Packed before licenses existed"
        }"#;

        let info: FantomeInfo = serde_json::from_str(legacy).unwrap();

        assert_eq!(info.name, "Old Mod");
        assert_eq!(info.license, None);
    }

    #[test]
    fn custom_license_rejects_unknown_field() {
        let typoed = r#"{
            "Name": "Test",
            "Author": "Test",
            "Version": "1.0.0",
            "Description": "Test",
            "License": { "Name": "My License", "Ur1": "https://example.com/terms" }
        }"#;

        assert!(
            serde_json::from_str::<FantomeInfo>(typoed).is_err(),
            "a misspelled license key must not parse as a URL-less license"
        );
    }
}
