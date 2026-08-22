//! Typed indices into the tables a modpkg stores alongside its chunks.
//!
//! Layers and WADs are addressed by `u32` table positions on disk. Each gets
//! its own type so that a `(wad, layer)` position pair cannot be built with
//! its halves swapped.

use std::fmt;

use binrw::binrw;

/// A position in the package's layer table.
///
/// Valid positions come from [`Modpkg::layer_index`](crate::Modpkg::layer_index)
/// or a chunk record; [`NONE`](Self::NONE) marks chunks that belong to no layer
/// (meta chunks).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerIndex(u32);

impl LayerIndex {
    /// The position of chunks that belong to no layer (meta chunks).
    pub const NONE: Self = Self(u32::MAX);

    /// Wrap a raw table position.
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// The raw table position.
    pub const fn value(self) -> u32 {
        self.0
    }
}

impl fmt::Display for LayerIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A position in the package's WAD table.
///
/// Valid positions come from [`Modpkg::wad_index`](crate::Modpkg::wad_index)
/// or a chunk record; [`NONE`](Self::NONE) marks chunks that belong to no WAD
/// (meta chunks).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WadIndex(u32);

impl WadIndex {
    /// The position of chunks that belong to no WAD (meta chunks).
    pub const NONE: Self = Self(u32::MAX);

    /// Wrap a raw table position.
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// The raw table position.
    pub const fn value(self) -> u32 {
        self.0
    }
}

impl fmt::Display for WadIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
