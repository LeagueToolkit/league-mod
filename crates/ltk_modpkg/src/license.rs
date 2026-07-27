use std::io::{Read, Seek};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    chunk::{NO_LAYER_INDEX, NO_WAD_INDEX},
    error::ModpkgError,
    Modpkg,
};

/// The path to the license text chunk.
pub const LICENSE_CHUNK_PATH: &str = "_meta_/license";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum ModpkgLicense {
    #[default]
    None,
    Spdx {
        spdx_id: String,
    },
    Custom {
        name: String,
        /// Optional link to the full terms. A package may name a license and
        /// ship its text in the [`LICENSE_CHUNK_PATH`] chunk without pointing
        /// anywhere.
        ///
        /// Always present on the wire (see [`serialize_url`]) so that readers
        /// predating the optional URL can still decode the metadata chunk.
        /// `None` and `Some("")` are the same value and both encode as `""`.
        #[serde(
            default,
            serialize_with = "serialize_url",
            deserialize_with = "deserialize_url"
        )]
        #[cfg_attr(test, proptest(strategy = "tests::arbitrary_url()"))]
        url: Option<String>,
    },
}

/// Serialize an absent URL as an empty string rather than omitting the key.
///
/// The license lives inside the msgpack metadata chunk, which readers decode as
/// a single unit. `url` used to be a required `String`, so omitting it does not
/// merely drop the URL for an older reader: it fails the whole metadata decode
/// and the package reads as corrupt. Writing `""` keeps those readers working.
fn serialize_url<S: Serializer>(url: &Option<String>, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(url.as_deref().unwrap_or(""))
}

/// Read a URL back, mapping the empty-string sentinel (and a missing key, via
/// `serde(default)`) to `None`.
fn deserialize_url<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.filter(|url| !url.is_empty()))
}

impl<TSource: Read + Seek> Modpkg<TSource> {
    /// Load the license text chunk from the mod package.
    ///
    /// Returns [`ModpkgError::MissingChunk`] if the package ships no license text.
    pub fn load_license_text(&mut self) -> Result<Vec<u8>, ModpkgError> {
        let chunk = *self.get_chunk(LICENSE_CHUNK_PATH, None)?;

        if chunk.layer_index != NO_LAYER_INDEX || chunk.wad_index != NO_WAD_INDEX {
            return Err(ModpkgError::InvalidMetaChunk);
        }

        let data = self.load_chunk_decompressed(&chunk)?;

        Ok(data.into_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Generates any URL except `Some("")`.
    ///
    /// An empty URL and no URL are the same value and both decode as `None`, so
    /// generating one would make every round-trip property fail on a difference
    /// that carries no information.
    pub(super) fn arbitrary_url() -> impl Strategy<Value = Option<String>> {
        proptest::option::of("\\PC{1,32}")
    }

    proptest! {
        #[test]
        fn test_license_roundtrip(license: ModpkgLicense) {
            let encoded = rmp_serde::to_vec_named(&license).unwrap();
            let decoded: ModpkgLicense = rmp_serde::from_slice(&encoded).unwrap();
            prop_assert_eq!(license, decoded);
        }
    }

    /// The empty-string sentinel is not a distinct value: an author writing
    /// `"url": ""` gets the same license as one who omitted the key.
    #[test]
    fn test_empty_url_decodes_as_none() {
        let encoded = rmp_serde::to_vec_named(&ModpkgLicense::Custom {
            name: "My License".to_string(),
            url: Some(String::new()),
        })
        .unwrap();

        let decoded: ModpkgLicense = rmp_serde::from_slice(&encoded).unwrap();

        assert_eq!(
            decoded,
            ModpkgLicense::Custom {
                name: "My License".to_string(),
                url: None,
            }
        );
    }

    #[test]
    fn test_custom_license_without_url_roundtrip() {
        let license = ModpkgLicense::Custom {
            name: "My License".to_string(),
            url: None,
        };

        let encoded = rmp_serde::to_vec_named(&license).unwrap();
        let decoded: ModpkgLicense = rmp_serde::from_slice(&encoded).unwrap();

        assert_eq!(license, decoded);
    }

    /// The license is embedded in the metadata chunk, so a missing `url` key
    /// would fail the *entire* metadata decode for readers that predate the
    /// optional URL. Those readers must keep working.
    #[test]
    fn test_url_less_custom_license_decodes_with_a_legacy_reader() {
        #[derive(Debug, Deserialize, PartialEq)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum LegacyLicense {
            None,
            Spdx { spdx_id: String },
            Custom { name: String, url: String },
        }

        let encoded = rmp_serde::to_vec_named(&ModpkgLicense::Custom {
            name: "My License".to_string(),
            url: None,
        })
        .unwrap();

        let decoded: LegacyLicense = rmp_serde::from_slice(&encoded).unwrap();

        assert_eq!(
            decoded,
            LegacyLicense::Custom {
                name: "My License".to_string(),
                url: String::new(),
            }
        );
    }

    /// A key omitted entirely still reads as `None`, so packages written while
    /// the field was `skip_serializing_if` remain readable.
    #[test]
    fn test_missing_url_key_decodes_as_none() {
        #[derive(Serialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum UrlLessWriter {
            Custom { name: String },
        }

        let encoded = rmp_serde::to_vec_named(&UrlLessWriter::Custom {
            name: "My License".to_string(),
        })
        .unwrap();

        let decoded: ModpkgLicense = rmp_serde::from_slice(&encoded).unwrap();

        assert_eq!(
            decoded,
            ModpkgLicense::Custom {
                name: "My License".to_string(),
                url: None,
            }
        );
    }
}
