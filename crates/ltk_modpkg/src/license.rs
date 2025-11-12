use serde::{Deserialize, Serialize};

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
        url: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn test_license_roundtrip(license: ModpkgLicense) {
            let encoded = rmp_serde::to_vec_named(&license).unwrap();
            let decoded: ModpkgLicense = rmp_serde::from_slice(&encoded).unwrap();
            prop_assert_eq!(license, decoded);
        }
    }
}
