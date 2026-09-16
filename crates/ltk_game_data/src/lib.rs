//! Ordered game-data declarations shared by projects, archives, and overlay consumers.
//!
//! A layer's [`Declarations`] are modules in execution order. A [`Module`] carries one
//! [`Selector`], which names what is edited and holds the edits, and its [`Origin`]. An
//! [`Edit`] is one batch of phased bindings on a chunk; an [`EntryEdit`] is the bindings of one
//! bin entry, applied in every chunk declaring it. [`apply`] runs edits over a `PROP`; an edit's
//! override files ([`OverridePath`]) are read through a caller-supplied reader.
//!
//! The `manifest` module loads the file a layer is edited through and its sources; the
//! `document` module carries the serialized [`DeclarationDocument`] an archive stores.

mod apply;
mod discovery;
mod document;
mod manifest;

pub use apply::{ApplyDiagnostic, ApplyDiagnosticKind, ApplyResult, apply};
pub use apply::{RecordSkipReason, SkippedRecord};
pub use document::DeclarationDocument;
pub use indexmap::IndexMap;
pub use ltk_hash::BinHash;
pub use manifest::{MANIFEST_NAMES, ReferencedInputs, load_declarations};

use std::fmt;

use serde::{Deserialize, Serialize};

/// A declaration that cannot be loaded or applied.
#[derive(Debug, thiserror::Error)]
#[error("{location}: {message}")]
pub struct Error {
    pub location: String,
    pub message: String,
}

impl Error {
    pub fn new(location: impl Into<String>, message: impl ToString) -> Self {
        Self {
            location: location.into(),
            message: message.to_string(),
        }
    }
}

/// A layer's modules in execution order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declarations {
    pub version: u32,
    pub modules: Vec<Module>,
}

impl Declarations {
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::new("game_data", "unsupported declarations version"));
        }
        Ok(())
    }

    /// Writes the declarations as a direct JSON manifest.
    ///
    /// The manifest names no source. Every target module carries an `edits` list; every edit
    /// carries `links`, empty or not.
    pub fn manifest_json(&self) -> Result<String, Error> {
        self.validate()?;
        serde_json::to_string_pretty(&manifest::Manifest::from(self))
            .map_err(|error| Error::new("game_data", error))
    }
}

/// One selector with its edits and its origin.
///
/// The serialized shape is `target` with `edits`, or `entries`, beside `origin`. A module
/// with both selector keys, or neither, does not deserialize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "document::Module", into = "document::Module")]
pub struct Module {
    pub selector: Selector,
    pub origin: Origin,
}

/// What a module edits, with the edits.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Selector {
    /// One chunk and the edits applied to it, in order.
    Target { target: Target, edits: Vec<Edit> },
    /// Entry names in mapping order, each with its edits. Every declaring chunk of an entry
    /// is edited.
    Entries(IndexMap<EntryName, EntryEdit>),
}

/// Where a module is declared. Indices are zero-based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub manifest: String,
    pub source: Option<String>,
    #[serde(rename = "module")]
    pub module_index: usize,
}

/// One edit of a chunk: every binding of one batch, applied phase by phase in field order.
///
/// Each edit of a target reads the result of the preceding one. The serialized form is the
/// compact binding body: `overrides`, `links` (`+links` accepted on input) and `-links`, each
/// present only when nonempty. A serialized override path is layer-relative.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "document::Bindings", into = "document::Bindings")]
#[non_exhaustive]
pub struct Edit {
    /// Override files applied in listed order, the first phase.
    pub overrides: Vec<OverridePath>,
    /// Dependency-list edits, the last phase.
    pub links: LinkEdit,
}

/// The bindings of one bin entry, applied in every chunk declaring it.
///
/// The serialized form is the compact binding body of [`Edit`] without `overrides`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "document::Bindings", into = "document::Bindings")]
#[non_exhaustive]
pub struct EntryEdit {
    /// Dependency-list edits of the declaring chunk.
    pub links: LinkEdit,
}

/// Link removals followed by link additions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkEdit {
    pub add: Vec<LinkPath>,
    pub remove: Vec<LinkPath>,
}

impl LinkEdit {
    /// Whether the edit adds or removes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.remove.is_empty()
    }
}

/// A nonempty game lookup path or bare 16-digit hexadecimal chunk hash.
/// Construction classifies the spelling and preserves it verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Target(TargetKind);

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetKind {
    Path(String),
    Hash(String),
}

impl TryFrom<String> for Target {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty() {
            return Err(Error::new("target", "expected a nonempty target string"));
        }
        Ok(Self(
            if value.len() == 16 && value.bytes().all(|c| c.is_ascii_hexdigit()) {
                TargetKind::Hash(value)
            } else {
                TargetKind::Path(value)
            },
        ))
    }
}

impl TryFrom<&str> for Target {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        Self::try_from(value.to_owned())
    }
}

impl From<Target> for String {
    fn from(target: Target) -> Self {
        match target.0 {
            TargetKind::Path(value) | TargetKind::Hash(value) => value,
        }
    }
}

impl Target {
    /// The game's WAD chunk identifier.
    pub fn chunk_hash(&self) -> u64 {
        match &self.0 {
            TargetKind::Path(path) => path_hash(path),
            TargetKind::Hash(hash) => u64::from_str_radix(hash, 16).expect("validated hex"),
        }
    }

    /// The identifier's spelling.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            TargetKind::Path(value) | TargetKind::Hash(value) => value,
        }
    }
}

/// A nonempty bin object path, or its hash as `0x` and exactly 8 hexadecimal digits.
/// Construction classifies the spelling and preserves it verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EntryName(EntryNameKind);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum EntryNameKind {
    Path(String),
    Hash(String),
}

impl TryFrom<String> for EntryName {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty() {
            return Err(Error::new("entries", "expected a nonempty entry name"));
        }
        let hash_form = value.strip_prefix("0x").is_some_and(|digits| {
            digits.len() == 8 && digits.bytes().all(|c| c.is_ascii_hexdigit())
        });
        Ok(Self(if hash_form {
            EntryNameKind::Hash(value)
        } else {
            EntryNameKind::Path(value)
        }))
    }
}

impl TryFrom<&str> for EntryName {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        Self::try_from(value.to_owned())
    }
}

impl From<EntryName> for String {
    fn from(name: EntryName) -> Self {
        match name.0 {
            EntryNameKind::Path(value) | EntryNameKind::Hash(value) => value,
        }
    }
}

impl fmt::Display for EntryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl EntryName {
    /// The name's spelling.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            EntryNameKind::Path(value) | EntryNameKind::Hash(value) => value,
        }
    }

    /// The bin object hash: FNV-1a over the ASCII-lowercased path, or the spelled hash.
    pub fn object_hash(&self) -> BinHash {
        match &self.0 {
            EntryNameKind::Path(path) => BinHash::from(path.as_str()),
            EntryNameKind::Hash(hash) => {
                BinHash(u32::from_str_radix(&hash[2..], 16).expect("validated hex"))
            }
        }
    }
}

/// A dependency path containing 1 to 65535 UTF-8 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LinkPath(String);

impl TryFrom<String> for LinkPath {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty() || value.len() > u16::MAX as usize {
            return Err(Error::new(
                "links",
                "link paths require 1 to 65535 UTF-8 bytes",
            ));
        }
        Ok(Self(value))
    }
}

impl TryFrom<&str> for LinkPath {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        Self::try_from(value.to_owned())
    }
}

impl From<LinkPath> for String {
    fn from(path: LinkPath) -> Self {
        path.0
    }
}

impl LinkPath {
    /// The path's spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The layer-relative, forward-slash path of a `.ptch` override file.
///
/// Construction enforces the spelling and preserves it verbatim: nonempty, relative, no
/// backslash, no empty, `.`, or `..` segment, and a `.ptch` extension compared ASCII
/// case-insensitively. A `.rito` path is refused with an error naming the extension.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OverridePath(String);

impl TryFrom<String> for OverridePath {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty() {
            return Err(Error::new("overrides", "expected a nonempty override path"));
        }
        if value.contains('\\') {
            return Err(Error::new(
                "overrides",
                "override paths use forward slashes",
            ));
        }
        if value.starts_with('/') {
            return Err(Error::new(
                "overrides",
                "override paths are relative to the layer",
            ));
        }
        if value
            .split('/')
            .any(|segment| matches!(segment, "" | "." | ".."))
        {
            return Err(Error::new(
                "overrides",
                "override paths contain no empty, `.`, or `..` segment",
            ));
        }
        let file_name = value.rsplit('/').next().unwrap_or(&value);
        match file_name.rsplit_once('.') {
            Some((stem, extension))
                if !stem.is_empty() && extension.eq_ignore_ascii_case("ptch") =>
            {
                Ok(Self(value))
            }
            Some((_, extension)) if extension.eq_ignore_ascii_case("rito") => Err(Error::new(
                "overrides",
                "`.rito` override files are unsupported; convert the file to `.ptch`",
            )),
            _ => Err(Error::new(
                "overrides",
                "override files require the `.ptch` extension",
            )),
        }
    }
}

impl TryFrom<&str> for OverridePath {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        Self::try_from(value.to_owned())
    }
}

impl From<OverridePath> for String {
    fn from(path: OverridePath) -> Self {
        path.0
    }
}

impl fmt::Display for OverridePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl OverridePath {
    /// The path's layer-relative spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The game's chunk-path hash. File suffixes and separators are preserved.
pub fn path_hash(path: &str) -> u64 {
    xxhash_rust::xxh64::xxh64(path.to_ascii_lowercase().as_bytes(), 0)
}
