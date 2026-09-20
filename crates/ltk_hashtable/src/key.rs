//! [`Key`]: a name's truncated hash, the unit every comparison runs on.

use std::fmt;

use ltk_hash::Hash as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A hashing algorithm the registry knows.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Algorithm {
    /// 64-bit xxHash, seed 0 - the `game` hash space.
    Xxh64,
    /// 32-bit FNV-1a - the `binentries` and `binhashes` hash spaces.
    Fnv1a32,
    /// An algorithm this tool does not recognize, spelling kept verbatim.
    ///
    /// Its keys cannot be computed, so a table declaring one is skipped for
    /// lookup - and preserved untouched on a rewrite or repack.
    Unknown(String),
}

impl Algorithm {
    /// The wire spelling.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Xxh64 => "xxh64",
            Self::Fnv1a32 => "fnv1a_32",
            Self::Unknown(spelling) => spelling,
        }
    }

    /// Parse a wire spelling; anything unrecognized is [`Algorithm::Unknown`].
    pub fn from_wire(spelling: &str) -> Self {
        match spelling {
            "xxh64" => Self::Xxh64,
            "fnv1a_32" => Self::Fnv1a32,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Algorithm {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Algorithm {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_wire(&String::deserialize(deserializer)?))
    }
}

/// A validated key width, in bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyWidth(u8);

impl KeyWidth {
    /// Validate a declared bit count; `1..=64` is representable.
    pub fn new(bits: u8) -> Option<Self> {
        (1..=64).contains(&bits).then_some(Self(bits))
    }

    /// The width in bits.
    pub fn bits(self) -> u8 {
        self.0
    }

    /// The mask keeping a hash's low `bits`: `(1 << bits) - 1`.
    fn mask(self) -> u64 {
        u64::MAX >> (64 - u32::from(self.0))
    }
}

/// A name's hash truncated to a declared width.
///
/// The only way to make one is [`Key::of`], which canonicalizes, hashes and
/// truncates in one motion - a full hash never reaches a caller. `Display`
/// renders fixed-width lowercase hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    value: u64,
    width: KeyWidth,
}

impl Key {
    /// Canonicalize `name`, hash it with `algorithm` and truncate to `width`.
    ///
    /// Returns `None` when the algorithm cannot be computed.
    pub fn of(name: &str, algorithm: &Algorithm, width: KeyWidth) -> Option<Self> {
        let canonical = name.to_ascii_lowercase();
        let hash = match algorithm {
            Algorithm::Xxh64 => ltk_hash::WadHash::hash_str(&canonical).0,
            Algorithm::Fnv1a32 => u64::from(ltk_hash::BinHash::hash_str(&canonical).0),
            Algorithm::Unknown(_) => return None,
        };
        Some(Self {
            value: hash & width.mask(),
            width,
        })
    }

    /// The key a hash value another crate holds truncates to.
    ///
    /// The inverse direction of [`Key::of`]: a caller holding a hash rather
    /// than a name - a WAD chunk's path hash, say - truncates it here to look
    /// it up. `value` is masked to the width's low bits, so a full hash and
    /// an already-truncated one produce the same key.
    pub fn from_value(value: u64, width: KeyWidth) -> Self {
        Self {
            value: value & width.mask(),
            width,
        }
    }

    /// The truncated value, for interop with hash types other crates hold.
    ///
    /// At a 64-bit width this is the full hash; at any narrower width it is
    /// the masked low bits, never the untruncated hash.
    pub fn value(self) -> u64 {
        self.value
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let digits = usize::from(self.width.bits().div_ceil(4));
        write!(f, "{:0digits$x}", self.value)
    }
}
