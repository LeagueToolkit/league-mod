//! [`Category`]: the standard's open category registry.
//!
//! Both registries are open: a spelling a tool does not recognize is carried
//! as `Unknown` verbatim, never dropped. The wire form of each is a bare
//! lowercase string in every container, so the serde impls live on the domain
//! types rather than in the containers - [`Category`]'s here, and
//! [`Algorithm`]'s beside [`Key`](crate::Key) in the key module, where the
//! hashing it names lives.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Algorithm, KeyWidth};

/// The lookup domain a table's names belong to.
///
/// An unknown category means the entry is ignored for lookup, and preserved
/// untouched on a rewrite or repack.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Category {
    /// WAD chunk paths and `File` property values, in the xxh64 hash space.
    Game,
    /// BIN object path names (entries).
    BinEntries,
    /// FNV-1a string hashes appearing as `Hash`-typed values.
    BinHashes,
    /// A category this tool does not recognize, spelling kept verbatim.
    Unknown(String),
}

impl Category {
    /// The wire spelling.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Game => "game",
            Self::BinEntries => "binentries",
            Self::BinHashes => "binhashes",
            Self::Unknown(spelling) => spelling,
        }
    }

    /// The algorithm and width the standard's registry currently lists for
    /// this category, for declaring a new manifest entry.
    ///
    /// `None` for an unknown category. An existing manifest entry always
    /// declares its own shape - this never overrides one.
    pub fn default_shape(&self) -> Option<(Algorithm, KeyWidth)> {
        let (algorithm, bits) = match self {
            Self::Game => (Algorithm::Xxh64, 64),
            Self::BinEntries | Self::BinHashes => (Algorithm::Fnv1a32, 32),
            Self::Unknown(_) => return None,
        };
        Some((algorithm, KeyWidth::new(bits)?))
    }

    /// Parse a wire spelling; anything unrecognized is [`Category::Unknown`].
    pub fn from_wire(spelling: &str) -> Self {
        match spelling {
            "game" => Self::Game,
            "binentries" => Self::BinEntries,
            "binhashes" => Self::BinHashes,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Category {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Category {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_wire(&String::deserialize(deserializer)?))
    }
}
