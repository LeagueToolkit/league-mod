//! Typed hashes for the name spaces a modpkg addresses chunks by.
//!
//! Paths, layers, and WADs are all identified by 64-bit hashes on disk. Each
//! gets its own type so that a hash from one name space cannot be passed
//! where another is expected, and so that `(u64, u64)` keys cannot be built
//! with their halves swapped.

use std::fmt;

use binrw::binrw;
use xxhash_rust::xxh3::xxh3_64;

/// The xxhash64 of a chunk's canonical path.
///
/// Produced by [`ChunkPath::hash`](crate::ChunkPath::hash), parsed from a hex
/// chunk name with [`from_hex_name`](Self::from_hex_name), or wrapped from a
/// raw value with [`new`](Self::new) when the hash comes from an external
/// hash list.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PathHash(u64);

impl PathHash {
    /// Wrap a raw hash value.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Parse a hex chunk name into the path hash it encodes.
    ///
    /// The base name (everything before the first `.`) must be exactly 16
    /// hexadecimal digits with no `0x` prefix; extensions after it are
    /// ignored. Returns `None` for any other shape.
    ///
    /// ```
    /// use ltk_modpkg::PathHash;
    ///
    /// let hash = PathHash::from_hex_name("abcdef1234567890.dds").unwrap();
    /// assert_eq!(hash.value(), 0xabcdef1234567890);
    /// assert_eq!(PathHash::from_hex_name("not_hex.bin"), None);
    /// ```
    pub fn from_hex_name(name: &str) -> Option<Self> {
        let base = name.split('.').next().unwrap_or(name);
        if base.len() != 16 || !base.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u64::from_str_radix(base, 16).ok().map(Self)
    }

    /// The raw hash value.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PathHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// The xxhash3 of a lowercased layer name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerHash(u64);

impl LayerHash {
    /// The layer hash of chunks that belong to no layer (meta chunks).
    pub const NONE: Self = Self(u64::MAX);

    /// Hash a layer name.
    ///
    /// The name is ASCII-lowercased first, so the hash is case-insensitive.
    pub fn from_name(name: &str) -> Self {
        Self(xxh3_64(name.to_ascii_lowercase().as_bytes()))
    }

    /// Wrap a raw hash value.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw hash value.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for LayerHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// The xxhash3 of a lowercased WAD name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WadNameHash(u64);

impl WadNameHash {
    /// The WAD name hash of chunks that belong to no WAD (meta chunks).
    pub const NONE: Self = Self(u64::MAX);

    /// Hash a WAD name.
    ///
    /// The name is ASCII-lowercased first, so the hash is case-insensitive.
    pub fn from_name(name: &str) -> Self {
        Self(xxh3_64(name.to_ascii_lowercase().as_bytes()))
    }

    /// Wrap a raw hash value.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw hash value.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for WadNameHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// The identity of a chunk: its path and the layer it belongs to.
///
/// WAD membership is not part of chunk identity - a chunk registered under
/// several WADs has one key, and [`Modpkg::chunks_for_wad_layer`] lists the
/// keys each WAD holds. Meta chunks use [`LayerHash::NONE`].
///
/// [`Modpkg::chunks_for_wad_layer`]: crate::Modpkg::chunks_for_wad_layer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkKey {
    pub path: PathHash,
    pub layer: LayerHash,
}

impl ChunkKey {
    /// Create a key from its parts.
    pub const fn new(path: PathHash, layer: LayerHash) -> Self {
        Self { path, layer }
    }
}
