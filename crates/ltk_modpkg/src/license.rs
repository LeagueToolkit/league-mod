use binrw::binrw;

use crate::utils::{nullstr_read, nullstr_write};

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum ModpkgLicense {
    #[default]
    #[brw(magic = 0u8)]
    None,
    #[brw(magic = 1u8)]
    Spdx {
        #[bw(map = nullstr_write)]
        #[br(try_map = nullstr_read)]
        spdx_id: String,
    },
    #[brw(magic = 2u8)]
    Custom {
        #[bw(map = nullstr_write)]
        #[br(try_map = nullstr_read)]
        name: String,
        #[bw(map = nullstr_write)]
        #[br(try_map = nullstr_read)]
        url: String,
    },
}

impl ModpkgLicense {
    /// The total size of the license when written to bytes.
    #[inline]
    pub fn size(&self) -> usize {
        1 + match self {
            Self::None => 0,
            // null terminators not included in len()
            Self::Spdx { spdx_id } => 1 + spdx_id.len(),
            Self::Custom { name, url } => 2 + name.len() + url.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn test_license_size(license: ModpkgLicense) {
            test::written_size(&license, license.size());
        }
    }
}
