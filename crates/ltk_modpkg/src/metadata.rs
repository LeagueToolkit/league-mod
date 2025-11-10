use crate::license::ModpkgLicense;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ModpkgMetadata {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub version: String,
    pub distributor: Option<String>,
    pub authors: Vec<ModpkgAuthor>,
    pub license: ModpkgLicense,
}

impl ModpkgMetadata {
    /// Read metadata from a reader using msgpack encoding.
    pub fn read<R: Read>(reader: &mut R) -> Result<Self, crate::error::ModpkgError> {
        rmp_serde::from_read(reader).map_err(Into::into)
    }

    /// Write metadata to a writer using msgpack encoding.
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<(), crate::error::ModpkgError> {
        rmp_serde::encode::write(writer, self).map_err(Into::into)
    }

    pub fn size(&self) -> usize {
        rmp_serde::to_vec(self).map(|v| v.len()).unwrap_or(0)
    }
}

impl ModpkgMetadata {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn distributor(&self) -> Option<&str> {
        self.distributor.as_deref()
    }
    pub fn authors(&self) -> &[ModpkgAuthor] {
        &self.authors
    }
    pub fn license(&self) -> &ModpkgLicense {
        &self.license
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
            let encoded = rmp_serde::to_vec(&author).unwrap();
            let decoded: ModpkgAuthor = rmp_serde::from_slice(&encoded).unwrap();
            prop_assert_eq!(author, decoded);
        }
    }

    #[test]
    fn test_modpkg_metadata_read() {
        let metadata = ModpkgMetadata {
            name: "test".to_string(),
            display_name: "test".to_string(),
            description: Some("test".to_string()),
            version: "1.0.0".to_string(),
            distributor: Some("test".to_string()),
            authors: vec![ModpkgAuthor {
                name: "test".to_string(),
                role: Some("test".to_string()),
            }],
            license: ModpkgLicense::Spdx {
                spdx_id: "MIT".to_string(),
            },
        };
        let mut cursor = Cursor::new(Vec::new());
        metadata.write(&mut cursor).unwrap();

        cursor.set_position(0);
        let read_metadata = ModpkgMetadata::read(&mut cursor).unwrap();
        assert_eq!(metadata, read_metadata);
    }
}
