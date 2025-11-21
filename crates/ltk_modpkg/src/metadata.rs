use crate::{
    chunk::{NO_LAYER_INDEX, NO_WAD_INDEX},
    error::ModpkgError,
    license::ModpkgLicense,
    Modpkg,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Seek, Write};

/// The path to the info.msgpack chunk.
pub const METADATA_CHUNK_PATH: &str = "_meta_/info.msgpack";

impl<TSource: Read + Seek> Modpkg<TSource> {
    /// Load the metadata chunk from the mod package.
    pub fn load_metadata(&mut self) -> Result<ModpkgMetadata, ModpkgError> {
        let chunk = *self.get_chunk(METADATA_CHUNK_PATH, None)?;

        if chunk.layer_index != NO_LAYER_INDEX || chunk.wad_index != NO_WAD_INDEX {
            return Err(ModpkgError::InvalidMetaChunk);
        }

        ModpkgMetadata::read(&mut Cursor::new(self.load_chunk_decompressed(&chunk)?))
    }
}

/// Information about the distributor site and mod ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct DistributorInfo {
    /// The identifier of the distributor site (e.g., "runeforge").
    pub site_id: String,
    /// The display name of the distributor site (e.g., "Runeforge").
    pub site_name: String,
    /// The base URL of the distributor site (e.g., "https://runeforge.dev").
    pub site_url: String,
    /// The mod ID on the distributor site.
    pub mod_id: String,
}

impl DistributorInfo {
    /// Create a new distributor info.
    pub fn new(site_id: String, site_name: String, site_url: String, mod_id: String) -> Self {
        Self {
            site_id,
            site_name,
            site_url,
            mod_id,
        }
    }

    /// Get the distributor site ID.
    pub fn site_id(&self) -> &str {
        &self.site_id
    }

    /// Get the display name of the distributor site.
    pub fn site_name(&self) -> &str {
        &self.site_name
    }

    /// Get the base URL of the distributor site.
    pub fn site_url(&self) -> &str {
        &self.site_url
    }

    /// Get the mod ID on the distributor site.
    pub fn mod_id(&self) -> &str {
        &self.mod_id
    }
}

/// Per-layer metadata that can be stored inside the mod package metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ModpkgLayerMetadata {
    /// The name of the layer (e.g. "base", "chroma1").
    pub name: String,
    /// The priority of the layer as stored in the modpkg header.
    pub priority: i32,
    /// Optional human-readable description of the layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The metadata of a mod package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ModpkgMetadata {
    /// The schema version of this metadata structure.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    #[cfg_attr(test, proptest(value = "Version::new(0, 1, 0)"))]
    pub version: Version,
    pub distributor: Option<DistributorInfo>,
    pub authors: Vec<ModpkgAuthor>,
    pub license: ModpkgLicense,

    /// This is purely informational and does not affect how the modpkg loader
    /// resolves layers; the canonical source of truth for layer priority is
    /// still the modpkg header.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<ModpkgLayerMetadata>,
}

impl Default for ModpkgMetadata {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            name: String::new(),
            display_name: String::new(),
            description: None,
            version: Version::new(0, 0, 0),
            distributor: None,
            authors: Vec::new(),
            license: ModpkgLicense::None,
            layers: Vec::new(),
        }
    }
}

fn default_schema_version() -> u32 {
    1
}

impl ModpkgMetadata {
    /// Get the path to the metadata chunk.
    pub fn path(&self) -> &str {
        METADATA_CHUNK_PATH
    }
}

impl ModpkgMetadata {
    /// Read metadata from a reader using msgpack encoding.
    pub fn read<R: Read>(reader: &mut R) -> Result<Self, crate::error::ModpkgError> {
        rmp_serde::from_read(reader).map_err(crate::error::ModpkgError::from)
    }

    /// Write metadata to a writer using msgpack encoding.
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<(), crate::error::ModpkgError> {
        let encoded = rmp_serde::to_vec_named(self).map_err(crate::error::ModpkgError::from)?;
        writer
            .write_all(&encoded)
            .map_err(crate::error::ModpkgError::from)?;
        Ok(())
    }

    pub fn size(&self) -> usize {
        rmp_serde::to_vec_named(self).map(|v| v.len()).unwrap_or(0)
    }
}

impl ModpkgMetadata {
    /// Get the name of the mod package.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Get the display name of the mod package.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    /// Get the description of the mod package.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    /// Get the version of the mod package.
    pub fn version(&self) -> &Version {
        &self.version
    }
    /// Get the distributor info of the mod package.
    pub fn distributor(&self) -> Option<&DistributorInfo> {
        self.distributor.as_ref()
    }
    /// Get the authors of the mod package.
    pub fn authors(&self) -> &[ModpkgAuthor] {
        &self.authors
    }
    /// Get the license of the mod package.
    pub fn license(&self) -> &ModpkgLicense {
        &self.license
    }

    /// Get the per-layer metadata entries, if any.
    pub fn layers(&self) -> &[ModpkgLayerMetadata] {
        &self.layers
    }
}

/// The author of a mod package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ModpkgAuthor {
    pub name: String,
    pub role: Option<String>,
}

impl ModpkgAuthor {
    pub fn new(name: String, role: Option<String>) -> Self {
        Self { name, role }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::io::Cursor;

    proptest! {
        #[test]
        fn test_metadata_roundtrip(metadata: ModpkgMetadata) {
            let mut cursor = Cursor::new(Vec::new());
            metadata.write(&mut cursor).unwrap();

            cursor.set_position(0);
            let read_metadata = ModpkgMetadata::read(&mut cursor).unwrap();
            prop_assert_eq!(metadata, read_metadata);
        }

        #[test]
        fn test_author_roundtrip(author: ModpkgAuthor) {
            let encoded = rmp_serde::to_vec_named(&author).unwrap();
            let decoded: ModpkgAuthor = rmp_serde::from_slice(&encoded).unwrap();
            prop_assert_eq!(author, decoded);
        }
    }

    #[test]
    fn test_modpkg_metadata_read() {
        let metadata = ModpkgMetadata {
            schema_version: 1,
            name: "test".to_string(),
            display_name: "test".to_string(),
            description: Some("test".to_string()),
            version: Version::parse("1.0.0").unwrap(),
            distributor: Some(DistributorInfo {
                site_id: "test_site".to_string(),
                site_name: "Test Site".to_string(),
                site_url: "https://test-site.com".to_string(),
                mod_id: "12345".to_string(),
            }),
            authors: vec![ModpkgAuthor {
                name: "test".to_string(),
                role: Some("test".to_string()),
            }],
            license: ModpkgLicense::Spdx {
                spdx_id: "MIT".to_string(),
            },
            layers: vec![],
        };
        let mut cursor = Cursor::new(Vec::new());
        metadata.write(&mut cursor).unwrap();

        cursor.set_position(0);
        let read_metadata = ModpkgMetadata::read(&mut cursor).unwrap();
        assert_eq!(metadata, read_metadata);
    }

    #[test]
    fn test_msgpack_format_visualization() {
        // This test shows what the msgpack encoding looks like with named fields (maps)
        let metadata = ModpkgMetadata {
            schema_version: 1,
            name: "TestMod".to_string(),
            display_name: "Test Mod".to_string(),
            description: Some("A test mod".to_string()),
            version: Version::parse("1.0.0").unwrap(),
            distributor: Some(DistributorInfo {
                site_id: "nexus".to_string(),
                site_name: "Nexus Mods".to_string(),
                site_url: "https://www.nexusmods.com".to_string(),
                mod_id: "12345".to_string(),
            }),
            authors: vec![ModpkgAuthor {
                name: "Author1".to_string(),
                role: Some("Developer".to_string()),
            }],
            license: ModpkgLicense::Spdx {
                spdx_id: "MIT".to_string(),
            },
            layers: vec![],
        };

        let encoded = rmp_serde::to_vec_named(&metadata).unwrap();
        println!("\nMsgpack encoded bytes (hex): {:02x?}", encoded);
        println!("Size: {} bytes", encoded.len());

        // Test all license variants
        let license_none = ModpkgLicense::None;
        let license_spdx = ModpkgLicense::Spdx {
            spdx_id: "MIT".to_string(),
        };
        let license_custom = ModpkgLicense::Custom {
            name: "MyLicense".to_string(),
            url: "https://example.com".to_string(),
        };

        println!(
            "\nLicense::None: {:02x?}",
            rmp_serde::to_vec_named(&license_none).unwrap()
        );
        println!(
            "License::Spdx: {:02x?}",
            rmp_serde::to_vec_named(&license_spdx).unwrap()
        );
        println!(
            "License::Custom: {:02x?}",
            rmp_serde::to_vec_named(&license_custom).unwrap()
        );
    }
}
