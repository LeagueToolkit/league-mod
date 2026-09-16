//! Ordered game-data declarations shared by projects, archives, and overlay consumers.
//!
//! A layer's [`Declarations`] are modules in execution order. A [`Module`] carries one
//! [`Selector`], which names what is edited and holds the edits, and its [`Origin`]. An
//! [`Edit`] is one batch of phased bindings on a chunk; an [`EntryEdit`] is the bindings of one
//! bin entry, applied in every chunk declaring it. [`apply`] runs edits over a `PROP`.

mod apply;
mod authoring;
mod discovery;

pub use apply::{ApplyDiagnostic, ApplyDiagnosticKind, ApplyResult, apply};
pub use authoring::{MANIFEST_NAMES, load_declarations, referenced_sources};
pub use indexmap::IndexMap;
pub use ltk_hash::BinHash;

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

/// An archive's versioned declarations, including fields an older consumer cannot execute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeclarationDocument(serde_json::Value);

impl DeclarationDocument {
    /// Validates the complete layer declarations before execution.
    pub fn parse(&self) -> Result<Declarations, Error> {
        let declarations: Declarations = serde_json::from_value(self.0.clone())
            .map_err(|error| Error::new("game_data", error))?;
        declarations.validate()?;
        Ok(declarations)
    }
}

impl From<Declarations> for DeclarationDocument {
    fn from(declarations: Declarations) -> Self {
        Self(
            serde_json::to_value(declarations)
                .expect("declarations contain JSON-compatible fields"),
        )
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

    /// Reconstructs a direct JSON authoring manifest.
    pub fn manifest_json(&self) -> Result<String, Error> {
        self.validate()?;
        let modules: Vec<_> = self
            .modules
            .iter()
            .map(|module| match &module.selector {
                Selector::Target { target, edits } => {
                    let steps: Vec<_> = edits.iter().map(|edit| body_json(&edit.links)).collect();
                    serde_json::json!({"target": target, "steps": steps})
                }
                Selector::Entries(entries) => {
                    let entries: IndexMap<&str, _> = entries
                        .iter()
                        .map(|(name, edit)| (name.as_str(), body_json(&edit.links)))
                        .collect();
                    serde_json::json!({"entries": entries})
                }
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({"version": 1, "modules": modules}))
            .map_err(|error| Error::new("game_data", error))
    }
}

/// The authoring body of one edit. `links` is always written; an authored step carries at
/// least one binding.
fn body_json(links: &LinkEdit) -> serde_json::Value {
    let mut body = serde_json::json!({"links": links.add});
    if !links.remove.is_empty() {
        body["-links"] = serde_json::json!(links.remove);
    }
    body
}

/// One selector with its edits and its authored origin.
///
/// The serialized shape is `target` with `steps`, or `entries`, beside `origin`. A module
/// with both selector keys, or neither, does not deserialize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ModuleWire", into = "ModuleWire")]
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

/// The serialized form of [`Module`]. The selector keys match the JSON manifest.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<Target>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    steps: Option<Vec<Edit>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entries: Option<IndexMap<EntryName, EntryEdit>>,
    origin: Origin,
}

impl TryFrom<ModuleWire> for Module {
    type Error = Error;

    fn try_from(wire: ModuleWire) -> Result<Self, Error> {
        let at = format!(
            "{}: module {}",
            wire.origin.manifest, wire.origin.module_index
        );
        let selector = match (one_selector(&at, wire.target, wire.entries)?, wire.steps) {
            (SelectorKey::Target(target), Some(edits)) => Selector::Target { target, edits },
            (SelectorKey::Target(_), None) => {
                return Err(Error::new(at, "target requires steps"));
            }
            (SelectorKey::Entries(entries), None) => Selector::Entries(entries),
            (SelectorKey::Entries(_), Some(_)) => {
                return Err(Error::new(at, "entries and steps are mutually exclusive"));
            }
        };
        Ok(Self {
            selector,
            origin: wire.origin,
        })
    }
}

impl From<Module> for ModuleWire {
    fn from(module: Module) -> Self {
        let (target, steps, entries) = match module.selector {
            Selector::Target { target, edits } => (Some(target), Some(edits), None),
            Selector::Entries(entries) => (None, None, Some(entries)),
        };
        Self {
            target,
            steps,
            entries,
            origin: module.origin,
        }
    }
}

/// The one selector key a module carries.
pub(crate) enum SelectorKey<T, E> {
    Target(T),
    Entries(E),
}

/// Exactly one of `target` and `entries`, or the error for a module `at`.
pub(crate) fn one_selector<T, E>(
    at: &str,
    target: Option<T>,
    entries: Option<E>,
) -> Result<SelectorKey<T, E>, Error> {
    match (target, entries) {
        (Some(target), None) => Ok(SelectorKey::Target(target)),
        (None, Some(entries)) => Ok(SelectorKey::Entries(entries)),
        (Some(_), Some(_)) => Err(Error::new(at, "target and entries are mutually exclusive")),
        (None, None) => Err(Error::new(at, "module requires target or entries")),
    }
}

/// Where a module was authored. Indices are zero-based.
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
/// compact binding body: `links` (`+links` accepted on input) and `-links`, each present
/// only when nonempty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "BindingsWire", into = "BindingsWire")]
#[non_exhaustive]
pub struct Edit {
    /// Dependency-list edits, the last phase.
    pub links: LinkEdit,
}

/// The bindings of one bin entry, applied in every chunk declaring it.
///
/// The serialized form is the same compact binding body as [`Edit`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "BindingsWire", into = "BindingsWire")]
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

/// The compact binding body shared by [`Edit`] and [`EntryEdit`].
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindingsWire {
    #[serde(
        default,
        rename = "links",
        alias = "+links",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) add_links: Option<Vec<LinkPath>>,
    #[serde(default, rename = "-links", skip_serializing_if = "Option::is_none")]
    pub(crate) remove_links: Option<Vec<LinkPath>>,
}

impl BindingsWire {
    /// Whether any binding key is written, empty or not.
    pub(crate) fn is_present(&self) -> bool {
        self.add_links.is_some() || self.remove_links.is_some()
    }

    fn into_links(self) -> LinkEdit {
        LinkEdit {
            add: self.add_links.unwrap_or_default(),
            remove: self.remove_links.unwrap_or_default(),
        }
    }

    fn from_links(links: LinkEdit) -> Self {
        Self {
            add_links: (!links.add.is_empty()).then_some(links.add),
            remove_links: (!links.remove.is_empty()).then_some(links.remove),
        }
    }
}

impl From<BindingsWire> for Edit {
    fn from(wire: BindingsWire) -> Self {
        Self {
            links: wire.into_links(),
        }
    }
}

impl From<Edit> for BindingsWire {
    fn from(edit: Edit) -> Self {
        Self::from_links(edit.links)
    }
}

impl From<BindingsWire> for EntryEdit {
    fn from(wire: BindingsWire) -> Self {
        Self {
            links: wire.into_links(),
        }
    }
}

impl From<EntryEdit> for BindingsWire {
    fn from(edit: EntryEdit) -> Self {
        Self::from_links(edit.links)
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

    /// The identifier's authored spelling.
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
    /// The name's authored spelling.
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

/// An authored dependency path containing 1 to 65535 UTF-8 bytes.
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
    /// The path's authored spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The game's chunk-path hash. File suffixes and separators are preserved.
pub fn path_hash(path: &str) -> u64 {
    xxhash_rust::xxh64::xxh64(path.to_ascii_lowercase().as_bytes(), 0)
}
