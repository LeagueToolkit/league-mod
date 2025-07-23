use binrw::binrw;

use crate::{
    license::ModpkgLicense,
    utils::{optional_string_len, optional_string_read, optional_string_write},
};
use serde::{Deserialize, Serialize};

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ModpkgMetadata {
    #[br(temp)]
    #[bw(calc = name.len() as u32)]
    name_len: u32,
    #[br(count = name_len, try_map = String::from_utf8)]
    #[bw(map = |s| s.as_bytes().to_vec())]
    pub name: String,

    #[br(temp)]
    #[bw(calc = display_name.len() as u32)]
    display_name_len: u32,
    #[br(count = display_name_len, try_map = String::from_utf8)]
    #[bw(map = |s| s.as_bytes().to_vec())]
    pub display_name: String,

    #[br(temp)]
    #[bw(calc = optional_string_len(description) as u32)]
    description_len: u32,
    #[brw(if(description_len > 0))]
    #[bw(map = optional_string_write)]
    #[br(count = description_len, try_map = optional_string_read)]
    pub description: Option<String>,

    #[br(temp)]
    #[bw(calc = version.len() as u32)]
    version_len: u32,
    #[br(count = version_len, try_map = String::from_utf8)]
    #[bw(map = |s| s.as_bytes().to_vec())]
    pub version: String,

    #[br(temp)]
    #[bw(calc = optional_string_len(distributor) as u32)]
    distributor_len: u32,
    #[brw(if(distributor_len> 0))]
    #[bw(map = optional_string_write)]
    #[br(count = distributor_len, try_map = optional_string_read)]
    pub distributor: Option<String>,

    #[br(temp)]
    #[bw(calc = (authors.len()) as u32)]
    author_count: u32,
    #[br(count = author_count)]
    pub authors: Vec<ModpkgAuthor>,

    pub license: ModpkgLicense,
}

impl ModpkgMetadata {
    /// The total size of the metadata when written to bytes.
    pub fn size(&self) -> usize {
        (self.name.len() + size_of::<u32>())
            + (self.display_name.len() + size_of::<u32>())
            + (optional_string_len(&self.description) + size_of::<u32>())
            + (self.version.len() + size_of::<u32>())
            + (optional_string_len(&self.distributor) + size_of::<u32>())
            + (self.authors.iter().map(|a| a.size()).sum::<usize>() + size_of::<u32>())
            + self.license().size()
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

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ModpkgAuthor {
    #[br(temp)]
    #[bw( calc = name.len() as u32 )]
    name_len: u32,
    #[br(count = name_len, try_map = String::from_utf8)]
    #[bw( map = |s| s.as_bytes().to_vec() )]
    pub name: String,

    #[br(temp)]
    #[bw( calc = role.as_ref().map(|n| n.len() as u32).unwrap_or_default() )]
    role_len: u32,

    #[brw(if(role_len > 0))]
    #[bw( map = |s| s.as_ref().map(|s| s.as_bytes().to_vec()) )]
    #[br(count = role_len, try_map = |s| String::from_utf8(s).map(Some))]
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

impl ModpkgAuthor {
    /// The total size of the author when written to bytes.
    pub fn size(&self) -> usize {
        (self.name.len() + size_of::<u32>()) + (optional_string_len(&self.role) + size_of::<u32>())
    }
}

#[cfg(test)]
mod tests {
    use crate::utils::test;
    use binrw::{BinRead, BinWrite};
    use std::io::Cursor;

    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn test_metadata_size(metadata: ModpkgMetadata) {
            test::written_size(&metadata, metadata.size());
        }
        #[test]
        fn test_author_size(author: ModpkgAuthor) {
            test::written_size(&author, author.size());
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
