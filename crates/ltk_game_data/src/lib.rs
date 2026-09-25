//! Ordered game-data declarations shared by projects, archives, and overlay consumers.
//!
//! A layer's [`Declarations`] are modules in execution order. A [`Module`] carries one
//! [`Selector`], which names what is edited and holds the edits, an optional [`ModuleName`],
//! and its [`Origin`]. An [`Edit`] is one batch of phased bindings on a chunk; an
//! [`EntryEdit`] is the bindings of one bin entry, applied in every chunk declaring it. A
//! [`PropertyEdit`] is one signed property path with its [`Value`]. [`apply`] runs edits over
//! a `PROP`; an edit's override files ([`OverridePath`]) are read through a caller-supplied
//! reader.
//!
//! The `manifest` module loads the file a layer is edited through and its sources; the
//! `document` module carries the serialized [`DeclarationDocument`] an archive stores.

mod apply;
mod discovery;
mod document;
mod error;
mod manifest;
mod property;
mod reference;
mod render;
mod schema;
mod value;
mod yaml;

pub use apply::{Applied, ApplyDiagnostic, ApplyDiagnosticKind, ApplyResult, apply};
pub use apply::{
    ObjectSkipReason, PropertySkipReason, RecordSkipReason, SkippedObject, SkippedProperty,
    SkippedRecord,
};
pub use document::DeclarationDocument;
pub use error::{Error, ErrorKind, Location, Span};
pub use indexmap::IndexMap;
pub use ltk_hash::BinHash;
pub use ltk_meta::{
    BinObject, PropertyKind,
    path::{FieldNames, PropertyPath},
};
pub use manifest::{MANIFEST_NAMES, ReferencedInputs, load_declarations};
pub use property::{PropertyEdit, Sign};
pub use reference::Reference;
pub use render::Names;
pub use schema::{NoSchema, Schema, Shape};
pub use value::{Value, kind_named, name_of};

use std::fmt;

use serde::{Deserialize, Serialize};

/// A layer's modules in execution order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declarations {
    pub version: u32,
    pub modules: Vec<Module>,
}

impl Declarations {
    /// Checks the declaration version and every module's selector.
    ///
    /// An `entries` module with no entry is valid and applies nothing.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::UnsupportedVersion`] for a version this crate does not execute, and
    /// [`ErrorKind::EditsEmpty`] for a target module with no edit. The error names the
    /// module index. A manifest refuses an empty `edits`, so declarations holding one write a
    /// manifest that does not load.
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::new(ErrorKind::UnsupportedVersion));
        }
        for (index, module) in self.modules.iter().enumerate() {
            if let Selector::Target { edits, .. } = &module.selector
                && edits.is_empty()
            {
                return Err(Error::new(ErrorKind::EditsEmpty)
                    .document(module.origin.manifest.clone())
                    .module(index));
            }
        }
        Ok(())
    }

    /// Writes the declarations as a direct JSON manifest.
    ///
    /// The manifest names no source. Every target module carries an `edits` list; every edit
    /// carries `links`, empty or not.
    ///
    /// # Errors
    ///
    /// Whatever [`validate`](Self::validate) refuses, and [`ErrorKind::Serialize`] for
    /// declarations the manifest form cannot hold ([`DeclarationDocument::try_from`]).
    pub fn manifest_json(&self) -> Result<String, Error> {
        self.validate()?;
        let manifest = manifest::Manifest::try_from(self)?;
        serde_json::to_string_pretty(&manifest).map_err(|error| {
            Error::new(ErrorKind::Serialize {
                detail: error.to_string(),
            })
        })
    }
}

/// One selector with its edits, its optional name, and its origin.
///
/// The serialized shape is an optional `name`, then `target` with `edits`, or `entries`,
/// beside `origin`. A module with both selector keys, or neither, does not deserialize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "document::Module", into = "document::Module")]
pub struct Module {
    /// The author's label for the module. Several modules of a layer may hold one name.
    pub name: Option<ModuleName>,
    pub selector: Selector,
    pub origin: Origin,
}

/// What a module edits, with the edits.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Selector {
    /// One chunk and the edits applied to it, in order.
    Target { target: Target, edits: Vec<Edit> },
    /// Entry names in mapping order, each with its edits. Every declaring chunk of an entry
    /// is edited.
    Entries(IndexMap<EntryName, EntryEdit>),
}

impl Module {
    /// Every reference the module's edits hold, in spelled order, duplicates included.
    ///
    /// A build resolves a reference against the installed game, which costs an object index
    /// and a chunk read. A consumer asks this to learn whether a module owes that cost
    /// before paying it.
    #[must_use]
    pub fn references(&self) -> Vec<Reference> {
        match &self.selector {
            Selector::Target { edits, .. } => edits.iter().flat_map(Edit::references).collect(),
            Selector::Entries(entries) => entries
                .values()
                .flat_map(|edit| edit.properties.iter())
                .flat_map(|property| property.value.references())
                .collect(),
        }
    }
}

impl Edit {
    /// Every reference the edit's property edits hold, in spelled order: the `set` of each
    /// object, then each entry.
    #[must_use]
    pub fn references(&self) -> Vec<Reference> {
        self.objects
            .values()
            .flat_map(ObjectEdit::properties)
            .chain(self.entries.values().flatten())
            .flat_map(|property| property.value.references())
            .collect()
    }
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

/// One edit of a chunk: every binding of one batch, applied phase by phase.
///
/// The phases are the override files, the object creations, the entry edits, the object
/// removals, and the dependency-list edits. Each edit of a target reads the result of the
/// preceding one. The serialized form is the compact binding body: `overrides`, `objects`,
/// one key per entry name holding its property edits, `links` (`+links` accepted on input)
/// and `-links`, each present only when nonempty. A serialized override path is
/// layer-relative.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(try_from = "document::Bindings")]
#[non_exhaustive]
pub struct Edit {
    /// Override files applied in listed order, the first phase.
    pub overrides: Vec<OverridePath>,
    /// Objects created or removed in the chunk, in mapping order. Creations are the second
    /// phase and removals the fourth.
    pub objects: IndexMap<EntryName, ObjectEdit>,
    /// The property edits of each entry of the chunk, in mapping order, the third phase.
    pub entries: IndexMap<EntryName, Vec<PropertyEdit>>,
    /// Dependency-list edits, the last phase.
    pub links: LinkEdit,
}

/// A new object of a chunk, or the removal of one.
///
/// The serialized form is an object body: `clone` or `class` beside an optional `set`, or
/// `remove: true`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ObjectEdit {
    /// A copy of the entry `source` holds at the start of the creation phase, with
    /// `properties` applied.
    Clone {
        source: EntryName,
        properties: Vec<PropertyEdit>,
    },
    /// An object of `class` holding no property but `properties`.
    Construct {
        class: ClassName,
        properties: Vec<PropertyEdit>,
    },
    /// The removal of the object.
    Remove,
}

impl ObjectEdit {
    /// The property edits of a creation's `set`, empty for a removal.
    #[must_use]
    pub fn properties(&self) -> &[PropertyEdit] {
        match self {
            Self::Clone { properties, .. } | Self::Construct { properties, .. } => properties,
            Self::Remove => &[],
        }
    }
}

/// A bin class: a name, or its hash as `0x` and 8 hexadecimal digits.
///
/// The class hash of a name is the FNV-1a of its ASCII-lowercased spelling.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ClassName(String);

impl TryFrom<String> for ClassName {
    type Error = Error;

    /// # Errors
    ///
    /// [`ErrorKind::EmptyClassName`] for an empty spelling.
    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty() {
            return Err(Error::at_key(ErrorKind::EmptyClassName, "class"));
        }
        Ok(Self(value))
    }
}

impl TryFrom<&str> for ClassName {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        Self::try_from(value.to_owned())
    }
}

impl From<ClassName> for String {
    fn from(name: ClassName) -> Self {
        name.0
    }
}

impl fmt::Display for ClassName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ClassName {
    /// The name's spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The class hash: the spelled hash, or FNV-1a over the ASCII-lowercased name.
    #[must_use]
    pub fn class_hash(&self) -> BinHash {
        apply::hash32_of(&self.0)
    }
}

/// The bindings of one bin entry, applied in every chunk declaring it.
///
/// The serialized form is an entry body: signed property keys beside `links` and `-links`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(try_from = "document::Bindings")]
#[non_exhaustive]
pub struct EntryEdit {
    /// The entry's property edits, in mapping order.
    pub properties: Vec<PropertyEdit>,
    /// Dependency-list edits of the declaring chunk.
    pub links: LinkEdit,
}

impl Serialize for Edit {
    /// # Errors
    ///
    /// The body mapping an edit writes holds one value per key. An entry name spelling a
    /// binding keyword, and one entry holding one signed key twice, are errors.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        document::Bindings::try_from(self.clone())
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl Serialize for EntryEdit {
    /// # Errors
    ///
    /// The body mapping an entry edit writes holds one value per key. A property path
    /// spelling a binding keyword, and one signed key held twice, are errors.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        document::Bindings::try_from(self.clone())
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
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
            return Err(Error::at_key(ErrorKind::EmptyTarget, "target"));
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
/// Construction classifies the spelling and preserves it as written.
///
/// A binding keyword is not one. A target body carries its entry names in the same mapping
/// as its bindings, so the serialized form drops an entry named `links` together with every
/// property edit under it. Construction refuses the spelling, not only the writer of the
/// serialized form. A consumer that builds [`Declarations`] by hand then hears about it at
/// the name it wrote, not at the pack that comes later.
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

    /// # Errors
    ///
    /// [`ErrorKind::EmptyEntryName`] for an empty spelling, and
    /// [`ErrorKind::ReservedBindingKey`] for a binding keyword.
    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty() {
            return Err(Error::at_key(ErrorKind::EmptyEntryName, "entries"));
        }
        if document::BindingKeyword::of(&value).is_some() {
            return Err(Error::at_key(
                ErrorKind::ReservedBindingKey { key: value.clone() },
                value,
            ));
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

    /// Whether the name is spelled as a hash: `0x` and 8 hexadecimal digits.
    #[must_use]
    pub fn is_hash(&self) -> bool {
        matches!(self.0, EntryNameKind::Hash(_))
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

/// An author's nonempty label for a module, kept as written.
///
/// A name carries no meaning for loading or application. Two modules of one layer may hold
/// the same name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ModuleName(String);

impl TryFrom<String> for ModuleName {
    type Error = Error;

    /// # Errors
    ///
    /// [`ErrorKind::EmptyModuleName`] for an empty spelling.
    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty() {
            return Err(Error::at_key(ErrorKind::EmptyModuleName, "name"));
        }
        Ok(Self(value))
    }
}

impl TryFrom<&str> for ModuleName {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        Self::try_from(value.to_owned())
    }
}

impl From<ModuleName> for String {
    fn from(name: ModuleName) -> Self {
        name.0
    }
}

impl fmt::Display for ModuleName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ModuleName {
    /// The name's spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
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
            return Err(Error::at_key(ErrorKind::LinkPathLength, "links"));
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
/// Construction enforces the spelling and keeps it as written: nonempty, relative, no
/// backslash, no drive prefix, no empty, `.`, or `..` segment, and a `.ptch` extension
/// compared ASCII case-insensitively. A `.rito` path is refused with an error naming the
/// extension. A first segment holding a `:` spells a drive path, `C:/a.ptch` or `C:a.ptch`,
/// which names a location outside the layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OverridePath(String);

impl TryFrom<String> for OverridePath {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty() {
            return Err(Error::at_key(ErrorKind::EmptyOverridePath, "overrides"));
        }
        if value.contains('\\') {
            return Err(Error::at_key(ErrorKind::OverridePathBackslash, "overrides"));
        }
        if value.starts_with('/') {
            return Err(Error::at_key(ErrorKind::OverridePathAbsolute, "overrides"));
        }
        if value
            .split('/')
            .next()
            .is_some_and(|first| first.contains(':'))
        {
            return Err(Error::at_key(ErrorKind::OverridePathAbsolute, "overrides"));
        }
        if value
            .split('/')
            .any(|segment| matches!(segment, "" | "." | ".."))
        {
            return Err(Error::at_key(ErrorKind::OverridePathSegment, "overrides"));
        }
        let file_name = value.rsplit('/').next().unwrap_or(&value);
        match file_name.rsplit_once('.') {
            Some((stem, extension))
                if !stem.is_empty() && extension.eq_ignore_ascii_case("ptch") =>
            {
                Ok(Self(value))
            }
            Some((_, extension)) if extension.eq_ignore_ascii_case("rito") => {
                Err(Error::at_key(ErrorKind::OverridePathRito, "overrides"))
            }
            _ => Err(Error::at_key(ErrorKind::OverridePathExtension, "overrides")),
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
