//! The serialized declaration document an archive carries as layer metadata.
//!
//! [`DeclarationDocument`] holds the document as JSON. [`Module`] and [`Bindings`] are the
//! serialized shapes of the executed [`crate::Module`], [`Edit`], and [`EntryEdit`]; every
//! override path in a document is layer-relative. [`Fields`] reads any document mapping key
//! by key: a value is deserialized where it is met, and a YAML tag on it survives.

use indexmap::IndexMap;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};

use crate::{
    Declarations, Edit, EntryEdit, EntryName, Error, ErrorKind, LinkEdit, LinkPath, Origin,
    OverridePath, PropertyEdit, Selector, Target, Value,
};

/// An archive's versioned declarations, including fields an older consumer cannot execute.
///
/// Reading refuses a duplicate mapping key anywhere in the document, the rule a manifest
/// obeys ([`crate::load_declarations`]). The archive metadata a document arrives in is not
/// necessarily written by this crate.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DeclarationDocument(serde_json::Value);

impl DeclarationDocument {
    /// Validates the complete layer declarations before execution.
    pub fn parse(&self) -> Result<Declarations, Error> {
        let declarations: Declarations =
            serde_json::from_value(self.0.clone()).map_err(|error| {
                Error::new(ErrorKind::Syntax {
                    detail: error.to_string(),
                })
            })?;
        declarations.validate()?;
        Ok(declarations)
    }
}

impl TryFrom<Declarations> for DeclarationDocument {
    type Error = Error;

    /// # Errors
    ///
    /// [`ErrorKind::Serialize`] for declarations the document form cannot hold: a binding
    /// keyword spelled as an entry name or a property path, one signed key held twice, and an
    /// integer outside the union of the `i64` and `u64` ranges.
    fn try_from(declarations: Declarations) -> Result<Self, Error> {
        serde_json::to_value(declarations)
            .map(Self)
            .map_err(|error| {
                Error::new(ErrorKind::Serialize {
                    detail: error.to_string(),
                })
            })
    }
}

impl<'de> Deserialize<'de> for DeclarationDocument {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonVisitor).map(Self)
    }
}

/// Reads any self-describing value as JSON, refusing a duplicate mapping key.
///
/// `serde_json::Value`'s own visitor keeps the last of a duplicate pair. A document is written
/// by an author, and dropping the first of two bindings drops what the author wrote.
struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = serde_json::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a declaration document")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(Self)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(serde_json::Value::from(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(serde_json::Value::from(value))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| de::Error::custom(format!("{value} is not a JSON number")))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(serde_json::Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(serde_json::Value::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = sequence.next_element_seed(Self)? {
            items.push(item);
        }
        Ok(serde_json::Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut entries = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value_seed(Self)?;
            if entries.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!("duplicate key `{key}`")));
            }
        }
        Ok(serde_json::Value::Object(entries))
    }
}

impl<'de> de::DeserializeSeed<'de> for JsonVisitor {
    type Value = serde_json::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

/// The serialized form of [`crate::Module`]. The selector keys match the manifest.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Module {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<Target>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edits: Option<Vec<Edit>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entries: Option<IndexMap<EntryName, EntryEdit>>,
    origin: Origin,
}

impl TryFrom<Module> for crate::Module {
    type Error = Error;

    fn try_from(module: Module) -> Result<Self, Error> {
        let origin = &module.origin;
        let at = |error: Error| {
            error
                .document(origin.manifest.clone())
                .module(origin.module_index)
        };
        let selector = match (
            SelectorKey::one(module.target, module.entries).map_err(at)?,
            module.edits,
        ) {
            (SelectorKey::Target(target), Some(edits)) => Selector::Target { target, edits },
            (SelectorKey::Target(_), None) => {
                return Err(at(Error::new(ErrorKind::TargetWithoutEdits)));
            }
            (SelectorKey::Entries(entries), None) => Selector::Entries(entries),
            (SelectorKey::Entries(_), Some(_)) => {
                return Err(at(Error::new(ErrorKind::EntriesWithEdits)));
            }
        };
        Ok(Self {
            selector,
            origin: module.origin,
        })
    }
}

impl From<crate::Module> for Module {
    fn from(module: crate::Module) -> Self {
        let (target, edits, entries) = match module.selector {
            Selector::Target { target, edits } => (Some(target), Some(edits), None),
            Selector::Entries(entries) => (None, None, Some(entries)),
        };
        Self {
            target,
            edits,
            entries,
            origin: module.origin,
        }
    }
}

/// The one selector key a module carries.
#[derive(Debug)]
pub(crate) enum SelectorKey<T, E> {
    Target(T),
    Entries(E),
}

impl<T, E> SelectorKey<T, E> {
    /// Exactly one of `target` and `entries`. The error carries no location.
    pub(crate) fn one(target: Option<T>, entries: Option<E>) -> Result<Self, Error> {
        match (target, entries) {
            (Some(target), None) => Ok(Self::Target(target)),
            (None, Some(entries)) => Ok(Self::Entries(entries)),
            (Some(_), Some(_)) => Err(Error::new(ErrorKind::SelectorConflict)),
            (None, None) => Err(Error::new(ErrorKind::SelectorMissing)),
        }
    }
}

/// The keys a document mapping accepts beside the binding keys.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Accepts {
    pub(crate) version: bool,
    pub(crate) selector: bool,
    pub(crate) source: bool,
    pub(crate) edits: bool,
}

/// A document mapping read key by key.
///
/// A known key is deserialized as its type where it is met, and a second occurrence is an
/// error. Every other key is kept with its [`Value`] in [`Bindings::rest`]. `E` is the type
/// of the `entries` key.
#[derive(Debug)]
pub(crate) struct Fields<E> {
    pub(crate) version: Option<u32>,
    pub(crate) target: Option<Target>,
    pub(crate) entries: Option<E>,
    pub(crate) source: Option<String>,
    pub(crate) edits: Option<Vec<Bindings>>,
    pub(crate) bindings: Bindings,
}

impl<E> Default for Fields<E> {
    fn default() -> Self {
        Self {
            version: None,
            target: None,
            entries: None,
            source: None,
            edits: None,
            bindings: Bindings::default(),
        }
    }
}

impl<'de, E: Deserialize<'de>> Fields<E> {
    /// Reads a mapping through `map`, accepting the keys `accepts` names.
    pub(crate) fn read<A: MapAccess<'de>>(mut map: A, accepts: Accepts) -> Result<Self, A::Error> {
        let mut fields = Self::default();
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "version" if accepts.version => fill(&mut fields.version, &mut map, &key)?,
                "target" if accepts.selector => fill(&mut fields.target, &mut map, &key)?,
                "entries" if accepts.selector => fill(&mut fields.entries, &mut map, &key)?,
                "source" if accepts.source => fill(&mut fields.source, &mut map, &key)?,
                "edits" if accepts.edits => fill(&mut fields.edits, &mut map, &key)?,
                _ => match BindingKeyword::of(&key) {
                    Some(BindingKeyword::Overrides) => {
                        fill(&mut fields.bindings.overrides, &mut map, &key)?;
                    }
                    // Not `fill`: the two spellings are one binding, so the message names
                    // the pair rather than whichever of them came second.
                    Some(BindingKeyword::AddLinks) => {
                        if fields.bindings.add_links.is_some() {
                            return Err(de::Error::custom(
                                "a body holds one of `links` and `+links`, once",
                            ));
                        }
                        fields.bindings.add_links = Some(map.next_value()?);
                    }
                    Some(BindingKeyword::RemoveLinks) => {
                        fill(&mut fields.bindings.remove_links, &mut map, &key)?;
                    }
                    None => {
                        let value: Value = map.next_value()?;
                        if fields.bindings.rest.insert(key.clone(), value).is_some() {
                            return Err(de::Error::custom(format!("duplicate key `{key}`")));
                        }
                    }
                },
            }
        }
        Ok(fields)
    }

    /// The visitor of a mapping read with `accepts`.
    pub(crate) fn visitor(accepts: Accepts) -> FieldsVisitor<E> {
        FieldsVisitor {
            accepts,
            entries: std::marker::PhantomData,
        }
    }
}

/// Reads the value of `key` into an unset `slot`.
fn fill<'de, T: Deserialize<'de>, A: MapAccess<'de>>(
    slot: &mut Option<T>,
    map: &mut A,
    key: &str,
) -> Result<(), A::Error> {
    if slot.is_some() {
        return Err(de::Error::custom(format!("duplicate field `{key}`")));
    }
    *slot = Some(map.next_value()?);
    Ok(())
}

/// The visitor of a [`Fields`] mapping.
pub(crate) struct FieldsVisitor<E> {
    accepts: Accepts,
    entries: std::marker::PhantomData<E>,
}

impl<'de, E: Deserialize<'de>> Visitor<'de> for FieldsVisitor<E> {
    type Value = Fields<E>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a mapping of bindings")
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        Fields::read(map, self.accepts)
    }
}

/// The compact binding body shared by [`Edit`] and [`EntryEdit`].
///
/// An override path is carried as spelled. A document body holds layer-relative paths; a
/// manifest or source body holds paths relative to the file naming them. `rest` holds every
/// key that is not a binding keyword, in spelled order: entry names in a target body,
/// signed property paths in an entry body.
#[derive(Debug, Default, Serialize)]
pub(crate) struct Bindings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) overrides: Option<Vec<String>>,
    #[serde(rename = "links", skip_serializing_if = "Option::is_none")]
    pub(crate) add_links: Option<Vec<LinkPath>>,
    #[serde(rename = "-links", skip_serializing_if = "Option::is_none")]
    pub(crate) remove_links: Option<Vec<LinkPath>>,
    #[serde(flatten)]
    pub(crate) rest: IndexMap<String, Value>,
}

impl<'de> Deserialize<'de> for Bindings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer
            .deserialize_map(Fields::<()>::visitor(Accepts::default()))
            .map(|fields| fields.bindings)
    }
}

impl Bindings {
    /// Whether any binding key or entry key is written, empty or not.
    pub(crate) fn is_present(&self) -> bool {
        self.overrides.is_some()
            || self.add_links.is_some()
            || self.remove_links.is_some()
            || !self.rest.is_empty()
    }

    fn into_links(self) -> LinkEdit {
        LinkEdit {
            add: self.add_links.unwrap_or_default(),
            remove: self.remove_links.unwrap_or_default(),
        }
    }

    fn from_links(links: LinkEdit) -> Self {
        Self {
            overrides: None,
            add_links: (!links.add.is_empty()).then_some(links.add),
            remove_links: (!links.remove.is_empty()).then_some(links.remove),
            rest: IndexMap::new(),
        }
    }

    /// The entry edits of a target body's remaining keys. Every key is an entry name.
    fn entries_of(
        rest: IndexMap<String, Value>,
    ) -> Result<IndexMap<EntryName, Vec<PropertyEdit>>, Error> {
        rest.into_iter()
            .map(|(key, value)| {
                // A binding keyword never reaches here: `Fields::read` routes it, and
                // `EntryName` refuses it. What is left is a key naming neither, which only
                // a target body can tell apart from an entry name, by the `/`.
                let name = EntryName::try_from(key.as_str())?;
                if !name.as_str().contains('/') && !name.is_hash() {
                    return Err(Error::at_key(
                        ErrorKind::UnsupportedBinding { key: key.clone() },
                        key,
                    ));
                }
                let Value::Mapping(body) = value else {
                    return Err(Error::new(ErrorKind::EntryBodyShape).entry(name.as_str()));
                };
                let edits = PropertyEdit::body(body).map_err(|error| error.entry(name.as_str()))?;
                Ok((name, edits))
            })
            .collect()
    }

    /// The edit of a body whose override paths resolve through `resolve`.
    pub(crate) fn into_edit(
        mut self,
        resolve: impl Fn(&str) -> Result<OverridePath, Error>,
    ) -> Result<Edit, Error> {
        let overrides = self
            .overrides
            .take()
            .unwrap_or_default()
            .iter()
            .map(|path| resolve(path))
            .collect::<Result<Vec<_>, _>>()?;
        let entries = Self::entries_of(std::mem::take(&mut self.rest))?;
        Ok(Edit {
            overrides,
            entries,
            links: self.into_links(),
        })
    }
}

impl TryFrom<Bindings> for Edit {
    type Error = Error;

    /// A document body: every override path is layer-relative.
    fn try_from(bindings: Bindings) -> Result<Self, Error> {
        bindings.into_edit(|path| OverridePath::try_from(path))
    }
}

impl TryFrom<Edit> for Bindings {
    type Error = Error;

    /// # Errors
    ///
    /// [`ErrorKind::ReservedBindingKey`] for an entry name spelling a binding keyword, and
    /// [`ErrorKind::DuplicatePropertyKey`] for an entry holding one signed key twice.
    fn try_from(edit: Edit) -> Result<Self, Error> {
        let mut rest = IndexMap::with_capacity(edit.entries.len());
        for (name, edits) in edit.entries {
            reserved(name.as_str())?;
            let body =
                PropertyEdit::try_into_body(edits).map_err(|error| error.entry(name.as_str()))?;
            rest.insert(String::from(name), Value::Mapping(body));
        }
        Ok(Self {
            overrides: (!edit.overrides.is_empty())
                .then(|| edit.overrides.into_iter().map(String::from).collect()),
            rest,
            ..Self::from_links(edit.links)
        })
    }
}

/// A key a body mapping reserves for a binding rather than for an entry name or a signed
/// property path.
///
/// A body carries the binding keys beside the entry names or the property paths, so one
/// spelling holds one meaning, and this is where the spellings are. The reader that routes
/// a key and the writer that refuses one have to agree about which keys are which; they
/// agree by both asking here, so a keyword gained or respelled is one edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingKeyword {
    /// `overrides`: the override files the edit applies.
    Overrides,
    /// `links` or `+links`: the dependencies the edit adds.
    AddLinks,
    /// `-links`: the dependencies the edit removes.
    RemoveLinks,
}

impl BindingKeyword {
    /// The keyword `key` spells, or `None` for an entry name or a property path.
    ///
    /// [`Bindings`] spells the same three in its `serde` renames, which take a literal and
    /// so cannot read them from here. Those renames and this function are the whole of it.
    pub(crate) fn of(key: &str) -> Option<Self> {
        match key {
            "overrides" => Some(Self::Overrides),
            "links" | "+links" => Some(Self::AddLinks),
            "-links" => Some(Self::RemoveLinks),
            _ => None,
        }
    }
}

/// Refuses a body key that spells a binding keyword.
fn reserved(key: &str) -> Result<(), Error> {
    if BindingKeyword::of(key).is_some() {
        return Err(Error::at_key(
            ErrorKind::ReservedBindingKey {
                key: key.to_owned(),
            },
            key,
        ));
    }
    Ok(())
}

impl TryFrom<Bindings> for EntryEdit {
    type Error = Error;

    fn try_from(mut bindings: Bindings) -> Result<Self, Error> {
        if bindings.overrides.is_some() {
            return Err(Error::at_key(ErrorKind::OverridesInEntry, "overrides"));
        }
        let properties = PropertyEdit::body(std::mem::take(&mut bindings.rest))?;
        Ok(Self {
            properties,
            links: bindings.into_links(),
        })
    }
}

impl TryFrom<EntryEdit> for Bindings {
    type Error = Error;

    /// # Errors
    ///
    /// [`ErrorKind::ReservedBindingKey`] for a property path spelling a binding keyword, and
    /// [`ErrorKind::DuplicatePropertyKey`] for one signed key held twice.
    fn try_from(edit: EntryEdit) -> Result<Self, Error> {
        let rest = PropertyEdit::try_into_body(edit.properties)?;
        for key in rest.keys() {
            reserved(key)?;
        }
        Ok(Self {
            rest,
            ..Self::from_links(edit.links)
        })
    }
}
