use crate::{error::ModpkgError, hashtable::ModpkgHashtable, license::ModpkgLicense, Modpkg};
use indexmap::IndexMap;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Seek, Write};

/// The path to the info.msgpack chunk.
pub const METADATA_CHUNK_PATH: &str = "_meta_/info.msgpack";

impl<TSource: Read + Seek> Modpkg<TSource> {
    /// Load the metadata chunk from the mod package.
    pub fn load_metadata(&mut self) -> Result<ModpkgMetadata, ModpkgError> {
        let chunk = *self.chunk(METADATA_CHUNK_PATH, None)?;

        if chunk.layer().is_some() || chunk.wad().is_some() {
            return Err(ModpkgError::InvalidMetaChunk);
        }

        ModpkgMetadata::read(&mut Cursor::new(
            self.decoder().load_chunk_decompressed(&chunk)?,
        ))
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
///
/// Added in schema version 2: the `string_overrides` field allows mods to
/// customise in-game text without shipping the entire `lol.stringtable` file.
///
/// # Example
///
/// ```
/// use indexmap::IndexMap;
/// use ltk_modpkg::ModpkgLayerMetadata;
///
/// let mut en_us_overrides = IndexMap::new();
/// en_us_overrides.insert("game_character_displayname_Ahri".to_string(), "Fox Spirit".to_string());
///
/// let layer = ModpkgLayerMetadata {
///     name: "base".to_string(),
///     display_name: None,
///     priority: 0,
///     description: Some("Base layer".to_string()),
///     string_overrides: IndexMap::from([
///         ("en_us".to_string(), en_us_overrides),
///     ]),
/// };
///
/// assert_eq!(layer.string_overrides.len(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ModpkgLayerMetadata {
    /// The name of the layer (e.g. "base", "chroma1").
    pub name: String,
    /// Optional human-readable display name for the layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The priority of the layer as stored in the modpkg header.
    pub priority: i32,
    /// Optional human-readable description of the layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// String overrides for this layer (added in schema v2), organized by locale.
    ///
    /// Outer key: locale (e.g., "en_us", "ko_kr", "zh_cn", or "default" for all locales)
    /// Inner map: field name (from `data/menu/{locale}/lol.stringtable`) -> replacement string
    ///
    /// Only the overrides are stored, not the full stringtable, so the
    /// mod stays compatible across game patches.
    /// Empty maps are omitted during serialization.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[cfg_attr(test, proptest(strategy = "string_overrides_strategy()"))]
    pub string_overrides: IndexMap<String, IndexMap<String, String>>,
}

/// Proptest strategy for [`ModpkgLayerMetadata::string_overrides`].
#[cfg(test)]
fn string_overrides_strategy(
) -> impl proptest::strategy::Strategy<Value = IndexMap<String, IndexMap<String, String>>> {
    use proptest::strategy::Strategy;

    let bucket = proptest::collection::vec(("[a-z_]{1,30}", "[a-zA-Z0-9 ]{0,50}"), 0..3)
        .prop_map(|entries| entries.into_iter().collect::<IndexMap<_, _>>());
    proptest::collection::vec(("[a-z]{2}_[a-z]{2}", bucket), 0..2)
        .prop_map(|buckets| buckets.into_iter().collect())
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
    #[cfg_attr(
        test,
        proptest(
            strategy = "proptest::collection::vec(proptest::prelude::any::<ModpkgAuthor>(), 0..3)"
        )
    )]
    pub authors: Vec<ModpkgAuthor>,
    pub license: ModpkgLicense,

    /// Tags/categories for the mod (e.g., "champion-skin", "sfx").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(
        test,
        proptest(strategy = "proptest::collection::vec(\"[a-z][a-z-]{0,20}\", 0..3)")
    )]
    pub tags: Vec<String>,

    /// Champions this mod targets (e.g., "Aatrox", "Ahri").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(
        test,
        proptest(strategy = "proptest::collection::vec(\"[A-Z][a-z]{2,10}\", 0..3)")
    )]
    pub champions: Vec<String>,

    /// Maps this mod targets (e.g., "Summoner's Rift", "Howling Abyss").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(
        test,
        proptest(strategy = "proptest::collection::vec(\"[A-Z][a-z]{2,10}\", 0..3)")
    )]
    pub maps: Vec<String>,

    /// This is purely informational and does not affect how the modpkg loader
    /// resolves layers; the canonical source of truth for layer priority is
    /// still the modpkg header.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(
        test,
        proptest(
            strategy = "proptest::collection::vec(proptest::prelude::any::<ModpkgLayerMetadata>(), 0..3)"
        )
    )]
    pub layers: Vec<ModpkgLayerMetadata>,

    /// The embedded hashtables the package declares (added in schema v3).
    ///
    /// The manifest is authoritative: a chunk under `_meta_/hashes/` that no
    /// entry here declares does not exist for lookup. Absent from packages
    /// written before schema v3, and omitted when empty so a package without
    /// tables serializes byte-identically to one written before this field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, proptest(strategy = "hashtables_strategy()"))]
    pub hashtables: Vec<ModpkgHashtable>,
}

/// Proptest strategy for [`ModpkgMetadata::hashtables`].
#[cfg(test)]
fn hashtables_strategy() -> impl proptest::strategy::Strategy<Value = Vec<ModpkgHashtable>> {
    use proptest::strategy::Strategy;

    proptest::collection::vec(
        ("[a-z0-9.-]{1,20}", 1u8..=64).prop_map(|(name, bits)| ModpkgHashtable {
            path: format!("_meta_/hashes/{name}"),
            category: ltk_hashtable::Category::Game,
            algorithm: ltk_hashtable::Algorithm::Xxh64,
            bits,
        }),
        0..3,
    )
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
            tags: Vec::new(),
            champions: Vec::new(),
            maps: Vec::new(),
            layers: Vec::new(),
            hashtables: Vec::new(),
        }
    }
}

/// Current metadata schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 3;

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
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

    /// Get the tags/categories of the mod package.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }
    /// Get the champions this mod targets.
    pub fn champions(&self) -> &[String] {
        &self.champions
    }
    /// Get the maps this mod targets.
    pub fn maps(&self) -> &[String] {
        &self.maps
    }

    /// Get the per-layer metadata entries, if any.
    pub fn layers(&self) -> &[ModpkgLayerMetadata] {
        &self.layers
    }

    /// Get the embedded hashtables the package declares.
    pub fn hashtables(&self) -> &[ModpkgHashtable] {
        &self.hashtables
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
        // Reduce test cases for CI performance (8 instead of default 256)
        // The nested map structure makes this test slow
        #![proptest_config(ProptestConfig::with_cases(8))]

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
            tags: vec![],
            champions: vec![],
            maps: vec![],
            layers: vec![],
            hashtables: vec![],
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
            tags: vec![],
            champions: vec![],
            maps: vec![],
            layers: vec![],
            hashtables: vec![],
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
            url: Some("https://example.com".to_string()),
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

    #[test]
    fn test_layer_string_overrides_roundtrip() {
        let layer = ModpkgLayerMetadata {
            name: "base".to_string(),
            display_name: None,
            priority: 0,
            description: Some("Base layer".to_string()),
            string_overrides: IndexMap::from([(
                "en_us".to_string(),
                IndexMap::from([
                    ("field_a".to_string(), "New Value A".to_string()),
                    ("field_b".to_string(), "New Value B".to_string()),
                ]),
            )]),
        };

        let encoded = rmp_serde::to_vec_named(&layer).unwrap();
        let decoded: ModpkgLayerMetadata = rmp_serde::from_slice(&encoded).unwrap();
        assert_eq!(layer, decoded);
    }

    #[test]
    fn test_layer_empty_overrides_skipped_in_serialization() {
        let layer = ModpkgLayerMetadata {
            name: "base".to_string(),
            display_name: None,
            priority: 0,
            description: None,
            string_overrides: IndexMap::new(),
        };

        let encoded = rmp_serde::to_vec_named(&layer).unwrap();
        // Empty string_overrides should not appear in encoded bytes
        let as_str = String::from_utf8_lossy(&encoded);
        assert!(!as_str.contains("string_overrides"));

        // Should still decode correctly
        let decoded: ModpkgLayerMetadata = rmp_serde::from_slice(&encoded).unwrap();
        assert_eq!(layer, decoded);
    }

    /// A metadata map with no `schema_version` key reads as CURRENT. Every
    /// writer, v1 onward, writes the key explicitly, so this only decides
    /// hand-built msgpack - and the default deliberately tracks
    /// [`CURRENT_SCHEMA_VERSION`], the crate's standing pattern. This test
    /// exists so the next version bump makes the same move consciously.
    #[test]
    fn a_missing_schema_version_key_reads_as_current() {
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        struct VersionlessWriter {
            name: String,
            display_name: String,
            description: Option<String>,
            version: Version,
            distributor: Option<DistributorInfo>,
            authors: Vec<ModpkgAuthor>,
            license: ModpkgLicense,
        }

        let encoded = rmp_serde::to_vec_named(&VersionlessWriter {
            name: "keyless".to_string(),
            display_name: "Keyless".to_string(),
            description: None,
            version: Version::new(1, 0, 0),
            distributor: None,
            authors: vec![],
            license: ModpkgLicense::None,
        })
        .unwrap();

        let decoded = ModpkgMetadata::read(&mut Cursor::new(encoded)).unwrap();
        assert_eq!(decoded.schema_version, CURRENT_SCHEMA_VERSION);
    }

    /// Schema v3 is additive: a v3 package must read on a v2 reader, with the
    /// hashtables ignored - the correct outcome for a reader that could not
    /// have used them.
    #[test]
    fn v3_metadata_decodes_with_a_v2_reader() {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct V2Metadata {
            name: String,
        }

        let metadata = ModpkgMetadata {
            name: "v3-mod".to_string(),
            hashtables: vec![ModpkgHashtable {
                path: "_meta_/hashes/game.hashes.txt".to_string(),
                category: ltk_hashtable::Category::Game,
                algorithm: ltk_hashtable::Algorithm::Xxh64,
                bits: 64,
            }],
            ..ModpkgMetadata::default()
        };

        let mut cursor = Cursor::new(Vec::new());
        metadata.write(&mut cursor).unwrap();

        let decoded: V2Metadata = rmp_serde::from_slice(cursor.get_ref()).unwrap();
        assert_eq!(decoded.name, "v3-mod");
    }

    /// A v2 package carries no `hashtables` key; the v3 reader must decode it
    /// with the field empty rather than failing the whole metadata chunk.
    #[test]
    fn v2_metadata_decodes_with_no_hashtables() {
        let v2 = ModpkgMetadata {
            schema_version: 2,
            name: "old-mod".to_string(),
            ..ModpkgMetadata::default()
        };
        let encoded = rmp_serde::to_vec_named(&v2).unwrap();
        assert!(!String::from_utf8_lossy(&encoded).contains("hashtables"));

        let decoded = ModpkgMetadata::read(&mut Cursor::new(encoded)).unwrap();
        assert!(decoded.hashtables.is_empty());
    }

    /// A package without tables serializes byte-identically to one written
    /// before the field existed.
    #[test]
    fn empty_hashtables_are_omitted_from_the_wire() {
        let encoded = rmp_serde::to_vec_named(&ModpkgMetadata::default()).unwrap();
        assert!(!String::from_utf8_lossy(&encoded).contains("hashtables"));
    }

    #[test]
    fn hashtables_roundtrip_through_the_metadata_chunk() {
        let manifest = ModpkgHashtable {
            path: "_meta_/hashes/game.imported.hashes.txt".to_string(),
            category: ltk_hashtable::Category::Unknown("wadnames".to_string()),
            algorithm: ltk_hashtable::Algorithm::Unknown("crc32".to_string()),
            bits: 32,
        };
        let metadata = ModpkgMetadata {
            hashtables: vec![manifest.clone()],
            ..ModpkgMetadata::default()
        };

        let mut cursor = Cursor::new(Vec::new());
        metadata.write(&mut cursor).unwrap();
        cursor.set_position(0);

        let read = ModpkgMetadata::read(&mut cursor).unwrap();
        // Unknown categories and algorithms round-trip verbatim: the open
        // registries keep their spelling.
        assert_eq!(read.hashtables, [manifest]);
    }

    #[test]
    fn test_v1_metadata_backward_compat() {
        // Simulate a v1 metadata without string_overrides on layers
        let v1_metadata = ModpkgMetadata {
            schema_version: 1,
            name: "old-mod".to_string(),
            display_name: "Old Mod".to_string(),
            description: None,
            version: Version::parse("1.0.0").unwrap(),
            distributor: None,
            authors: vec![],
            license: ModpkgLicense::None,
            tags: vec![],
            champions: vec![],
            maps: vec![],
            layers: vec![ModpkgLayerMetadata {
                name: "base".to_string(),
                display_name: None,
                priority: 0,
                description: None,
                string_overrides: IndexMap::new(),
            }],
            hashtables: vec![],
        };

        let mut cursor = Cursor::new(Vec::new());
        v1_metadata.write(&mut cursor).unwrap();
        cursor.set_position(0);

        let read = ModpkgMetadata::read(&mut cursor).unwrap();
        assert_eq!(v1_metadata, read);
        assert!(read.layers[0].string_overrides.is_empty());
    }

    #[test]
    fn test_v2_metadata_with_string_overrides() {
        let metadata = ModpkgMetadata {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: "test-mod".to_string(),
            display_name: "Test Mod".to_string(),
            description: Some("A mod with string overrides".to_string()),
            version: Version::parse("2.0.0").unwrap(),
            distributor: None,
            authors: vec![ModpkgAuthor {
                name: "Author".to_string(),
                role: None,
            }],
            license: ModpkgLicense::None,
            tags: vec![],
            champions: vec![],
            maps: vec![],
            layers: vec![
                ModpkgLayerMetadata {
                    name: "base".to_string(),
                    display_name: None,
                    priority: 0,
                    description: None,
                    string_overrides: IndexMap::from([(
                        "en_us".to_string(),
                        IndexMap::from([("game_stat_name".to_string(), "Custom Stat".to_string())]),
                    )]),
                },
                ModpkgLayerMetadata {
                    name: "chroma1".to_string(),
                    display_name: Some("Pink chroma".to_string()),
                    priority: 10,
                    description: Some("Pink chroma".to_string()),
                    string_overrides: IndexMap::from([(
                        "en_us".to_string(),
                        IndexMap::from([
                            ("champion_name".to_string(), "Custom Name".to_string()),
                            ("ability_desc".to_string(), "Custom Description".to_string()),
                        ]),
                    )]),
                },
            ],
            hashtables: vec![],
        };

        let mut cursor = Cursor::new(Vec::new());
        metadata.write(&mut cursor).unwrap();
        cursor.set_position(0);

        let read = ModpkgMetadata::read(&mut cursor).unwrap();
        assert_eq!(metadata, read);
        assert_eq!(read.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(read.layers[0].string_overrides.len(), 1); // 1 locale
        assert_eq!(read.layers[1].string_overrides.len(), 1); // 1 locale
        assert_eq!(
            read.layers[0]
                .string_overrides
                .get("en_us")
                .and_then(|m| m.get("game_stat_name")),
            Some(&"Custom Stat".to_string())
        );
    }
}
