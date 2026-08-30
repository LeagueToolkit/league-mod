//! Reading and writing the legacy Fantome (`.fantome`) archive format.
//!
//! This crate handles the format itself: the metadata shapes stored in
//! `META/info.json`, [`FantomeWriter`] for producing archives, and
//! [`FantomeReader`] for consuming them. Turning a mod *project* into an
//! archive (and back) lives in `ltk_mod_project`'s `fantome` module, which
//! composes the primitives here.
//!
//! Three operations rewrite an archive in place of reading it:
//! [`add_hashtables`] merges harvested names into one, [`normalize_archive`]
//! holds its packed WADs stored so a reader can seek to them, and
//! [`apply_delta`] repairs its content - chunks inside a packed WAD, whole
//! entries, or both - without repacking the mod. All three raw-copy everything
//! they do not themselves replace, and all three belong to a caller working on
//! a copy it owns.
//!
//! What normalizing buys is spent twice over. [`FantomeReader::mount_packed_wad`]
//! reads a packed WAD an archive stores chunk by chunk where it lies, so looking
//! inside one costs its TOC and the chunks asked for rather than the whole
//! archive unpacked to a directory; and [`apply_delta`] rewrites that WAD's
//! tail in place of rebuilding it, so a repair costs what changed rather than
//! what the mod holds.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod delta;
pub mod error;
mod normalize;
mod packed;
mod reader;
mod rewrite;
mod writer;

pub use delta::{
    ArchiveDelta, DeltaProgress, DeltaReport, DeltaStep, FantomeDeltaError, apply_delta,
};
pub use error::{FantomeExtractError, FantomeWriteError};
/// Re-exported because this crate's own signatures name them.
///
/// [`WadExtractOptions`] takes the first three: a caller names chunks by
/// implementing [`PathResolver`] over whatever it holds, or leaves the default
/// [`NoResolver`] in place and leaves the naming to the archive's own bins,
/// and [`NamingPolicy`] decides what becomes of a chunk two paths claim.
/// [`Wad`] is what [`FantomeReader::mount_packed_wad`] answers with.
pub use ltk_wad::{NamingPolicy, NoResolver, PathResolver, Wad};
pub use normalize::{
    FantomeNormalizeError, NormalizeOutcome, normalize_archive, store_packed_wads,
};
pub use packed::PackedWadSource;
pub use reader::{FantomeEntry, FantomeReader, WadExtractOptions, WadProgress, classify_entry};
pub use rewrite::{FantomeRewriteError, RewriteOutcome, add_hashtables, replace_entries};
pub use writer::FantomeWriter;

/// Fantome metadata structure that goes into info.json
///
/// `Default` is for struct update syntax and for tests building one field at a
/// time (`FantomeInfo { name, ..Default::default() }`); a default archive's
/// metadata is empty, not valid.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct FantomeInfo {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Author")]
    pub author: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Description")]
    pub description: String,
    /// The license the mod is distributed under.
    ///
    /// Independent of the `META/LICENSE` archive entry: this names the terms,
    /// that entry carries their text. Either, both, or neither may be present.
    #[serde(rename = "License", default, skip_serializing_if = "Option::is_none")]
    pub license: Option<FantomeLicense>,
    /// Tags/categories for the mod (e.g., "champion-skin", "sfx").
    #[serde(rename = "Tags", default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Champions this mod targets (e.g., "Aatrox", "Ahri").
    #[serde(rename = "Champions", default, skip_serializing_if = "Vec::is_empty")]
    pub champions: Vec<String>,
    /// Maps this mod targets (e.g., "Summoner's Rift", "Howling Abyss").
    #[serde(rename = "Maps", default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<String>,
    /// Per-layer metadata including string overrides.
    #[serde(rename = "Layers", default, skip_serializing_if = "HashMap::is_empty")]
    pub layers: HashMap<String, FantomeLayerInfo>,
    /// The embedded hashtables the archive declares.
    ///
    /// The manifest is authoritative: a `META/hashes/` entry no manifest
    /// entry declares does not exist for lookup. Absent from archives written
    /// before the standard, and omitted when empty so an archive without
    /// tables serializes byte-identically to one written before this field.
    #[serde(rename = "Hashtables", default, skip_serializing_if = "Vec::is_empty")]
    pub hashtables: Vec<FantomeHashtable>,
    /// Fields this crate does not know, carried verbatim.
    ///
    /// `info.json` is shared ground: other tools extend it, and the archive
    /// rewrite reserializes it. Dropping what we cannot name would make this
    /// crate the older tool that silently strips a newer one's data.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// One `Hashtables` manifest entry in a Fantome info.json.
///
/// The fantome spelling of `ltk_hashtable`'s `HashtableEntry`: PascalCase
/// keys, `Path` relative to the archive root. Convert with
/// [`FantomeHashtable::to_entry`] and [`FantomeHashtable::from_entry`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FantomeHashtable {
    /// Where the table file lives, relative to the archive root.
    #[serde(rename = "Path")]
    pub path: String,
    /// The lookup domain of the table's names.
    #[serde(rename = "Category")]
    pub category: ltk_hashtable::Category,
    /// The hash function keying the table's names.
    #[serde(rename = "Algorithm")]
    pub algorithm: ltk_hashtable::Algorithm,
    /// The declared key width in bits.
    #[serde(rename = "Bits")]
    pub bits: u8,
}

impl FantomeHashtable {
    /// The entry as `ltk_hashtable`'s domain type.
    ///
    /// `None` when `Bits` declares a width no key can have; the standard
    /// requires `1..=64`.
    pub fn to_entry(&self) -> Option<ltk_hashtable::HashtableEntry> {
        let width = ltk_hashtable::KeyWidth::new(self.bits)?;
        Some(ltk_hashtable::HashtableEntry::new(
            self.path.as_str(),
            self.category.clone(),
            self.algorithm.clone(),
            width,
        ))
    }

    /// Spell a domain entry the fantome way.
    pub fn from_entry(entry: &ltk_hashtable::HashtableEntry) -> Self {
        Self {
            path: entry.path().to_string(),
            category: entry.category().clone(),
            algorithm: entry.algorithm().clone(),
            bits: entry.width().bits(),
        }
    }
}

/// The license declaration in a Fantome info.json.
///
/// Either a bare SPDX identifier (`"License": "MIT"`) or an object naming the
/// license with an optional link (`"License": {"Name": "...", "Url": "..."}`).
/// Note the PascalCase inner keys: this is a distinct shape from
/// `ltk_mod_project`'s license object, whose keys are `{name, url}`.
///
/// Unknown fields are rejected: with `Url` optional, `Name` is the only
/// required key, so an object with a misspelled `Url` would otherwise still
/// match `Custom` and quietly lose the link.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum FantomeLicense {
    Spdx(String),
    Custom {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "Url", default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
}

/// Per-layer metadata in a Fantome info.json.
///
/// As with [`FantomeInfo`], `Default` is for struct update syntax; it names no
/// layer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct FantomeLayerInfo {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(
        rename = "DisplayName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub display_name: Option<String>,
    #[serde(rename = "Priority")]
    pub priority: i32,
    /// String overrides for this layer, organized by locale.
    /// Outer key: locale (e.g., "en_us", "ko_kr", or "default")
    /// Inner map: field name -> replacement string
    #[serde(
        rename = "StringOverrides",
        default,
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub string_overrides: IndexMap<String, IndexMap<String, String>>,
}

#[cfg(test)]
mod tests;
