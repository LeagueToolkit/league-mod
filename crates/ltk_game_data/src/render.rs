//! Rendering: the reading of a bin's property value as a [`Value`].
//!
//! The rules are the rendering table of `docs/design/game-data.md` section 6, the inverse of
//! the coercion table beside it ([ADR-0020]).
//!
//! [ADR-0020]: https://github.com/LeagueToolkit/league-mod/blob/main/docs/adr/0020-value-rendering.md

use std::borrow::Cow;

use indexmap::IndexMap;
use ltk_hash::BinHash;
use ltk_meta::{
    PropertyValueEnum as V,
    path::{FieldNames, PropertyPath},
    property::values,
};

use crate::{
    Error, ErrorKind, Value,
    apply::{hash32_of, hash64_of},
    kind_named,
    value::REFERENCE_KEY,
};

/// Plaintext for the hashes a rendered value carries ([ADR-0020]).
///
/// A name that does not hash back to the value it names is ignored, and the value renders
/// as its `0x` spelling.
///
/// [ADR-0020]: https://github.com/LeagueToolkit/league-mod/blob/main/docs/adr/0020-value-rendering.md
pub trait Names: FieldNames {
    /// The name of a struct's class.
    fn class(&self, class: BinHash) -> Option<Cow<'_, str>>;

    /// The object path a `link` value hashes.
    fn entry(&self, entry: BinHash) -> Option<Cow<'_, str>>;

    /// The chunk path a `file` value hashes.
    fn file(&self, chunk: u64) -> Option<Cow<'_, str>>;
}

/// Names nothing: every hash renders as its `0x` spelling, and every struct field is nameless.
impl Names for () {
    fn class(&self, _class: BinHash) -> Option<Cow<'_, str>> {
        None
    }

    fn entry(&self, _entry: BinHash) -> Option<Cow<'_, str>> {
        None
    }

    fn file(&self, _chunk: u64) -> Option<Cow<'_, str>> {
        None
    }
}

impl<N: Names + ?Sized> Names for &N {
    fn class(&self, class: BinHash) -> Option<Cow<'_, str>> {
        (**self).class(class)
    }

    fn entry(&self, entry: BinHash) -> Option<Cow<'_, str>> {
        (**self).entry(entry)
    }

    fn file(&self, chunk: u64) -> Option<Cow<'_, str>> {
        (**self).file(chunk)
    }
}

impl Value {
    /// The literal that coerces back to `value` under the value's own shape.
    ///
    /// The literal carries no type pin. A struct renders as a struct pin, whose fields a
    /// schema types when the literal is read back. A struct field `names` has no name for
    /// renders as `0x` and its 8 hexadecimal digits.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::UnrenderableKey`] for a map key with no spelling, and
    /// [`ErrorKind::UnrenderableValue`] for a value of kind `none`. The error's key is the
    /// path to the key or the value inside `value`.
    pub fn render(value: &V, names: &dyn Names) -> Result<Self, Error> {
        Renderer {
            names,
            path: String::new(),
        }
        .value(value)
    }
}

/// Renders one value, keeping the path to where it stands for an error to name.
struct Renderer<'a> {
    names: &'a dyn Names,
    path: String,
}

impl Renderer<'_> {
    fn value(&mut self, value: &V) -> Result<Value, Error> {
        Ok(match value {
            V::None(_) => return Err(self.error(ErrorKind::UnrenderableValue)),
            V::Bool(flag) => Value::Bool(flag.value),
            V::BitBool(flag) => Value::Bool(flag.value),
            V::I8(number) => Value::Integer(number.value.into()),
            V::U8(number) => Value::Integer(number.value.into()),
            V::I16(number) => Value::Integer(number.value.into()),
            V::U16(number) => Value::Integer(number.value.into()),
            V::I32(number) => Value::Integer(number.value.into()),
            V::U32(number) => Value::Integer(number.value.into()),
            V::I64(number) => Value::Integer(number.value.into()),
            V::U64(number) => Value::Integer(number.value.into()),
            V::F32(number) => single(number.value),
            V::Vector2(vector) => singles(&vector.value.to_array()),
            V::Vector3(vector) => singles(&vector.value.to_array()),
            V::Vector4(vector) => singles(&vector.value.to_array()),
            // Coercion reads sixteen numbers as rows, so the rows are what is written.
            V::Matrix44(matrix) => singles(&matrix.value.transpose().to_cols_array()),
            V::Color(color) => {
                let color = color.value;
                Value::List(
                    [color.r, color.g, color.b, color.a]
                        .map(|channel| Value::Integer(channel.into()))
                        .into(),
                )
            }
            V::String(text) => Value::String(text.value.clone()),
            V::Hash(hash) => Value::String(self.hash(hash.value)),
            V::ObjectLink(link) => {
                Value::String(spelled32(link.value, self.names.entry(link.value)))
            }
            V::WadChunkLink(file) => Value::String(self.file(file.value.0)),
            V::Container(items) => self.list(items.items())?,
            V::UnorderedContainer(items) => self.list(items.0.items())?,
            V::Optional(option) => self.optional(option)?,
            V::Map(map) => self.map(map)?,
            V::Struct(pointer) if *pointer.class_hash == 0 => Value::Null,
            V::Struct(pointer) => self.struct_pin("pointer", pointer)?,
            V::Embedded(embed) => self.struct_pin("embed", &embed.0)?,
        })
    }

    fn list(&mut self, items: &[V]) -> Result<Value, Error> {
        items
            .iter()
            .enumerate()
            .map(|(index, item)| self.under(&format!("[{index}]"), |this| this.value(item)))
            .collect::<Result<_, _>>()
            .map(Value::List)
    }

    /// An element that renders as a list or as null is wrapped in a one-element list, because
    /// coercion reads a bare list as the option's elements and a bare null as the empty option.
    fn optional(&mut self, option: &values::Optional) -> Result<Value, Error> {
        let Some(element) = option.value() else {
            return Ok(Value::Null);
        };
        Ok(match self.under("[0]", |this| this.value(element))? {
            element @ (Value::List(_) | Value::Null) => Value::List(vec![element]),
            element => element,
        })
    }

    fn map(&mut self, map: &values::Map) -> Result<Value, Error> {
        let mut mapping = IndexMap::with_capacity(map.entries().len());
        for (key, value) in map.entries() {
            let text = self.key(key)?;
            let step = format!("{{{text}}}");
            let value = self.under(&step, |this| this.value(value))?;
            if mapping.insert(text, value).is_some() {
                // A mapping spells each key once, and the map holds this one twice.
                return Err(self.error_under(&step, ErrorKind::UnrenderableKey));
            }
        }
        // Coercion reads a one-key mapping keyed by a type name or `ref` as a pin or a
        // reference, never as a map of one entry.
        if let [key] = mapping.keys().collect::<Vec<_>>()[..]
            && (kind_named(key).is_some() || key == REFERENCE_KEY)
        {
            let step = format!("{{{key}}}");
            return Err(self.error_under(&step, ErrorKind::UnrenderableKey));
        }
        Ok(Value::Mapping(mapping))
    }

    /// A map key as the string the key rule of coercion reads back.
    fn key(&mut self, key: &V) -> Result<String, Error> {
        Ok(match key {
            V::Bool(flag) => flag.value.to_string(),
            V::I8(number) => number.value.to_string(),
            V::U8(number) => number.value.to_string(),
            V::I16(number) => number.value.to_string(),
            V::U16(number) => number.value.to_string(),
            V::I32(number) => number.value.to_string(),
            V::U32(number) => number.value.to_string(),
            V::I64(number) => number.value.to_string(),
            V::U64(number) => number.value.to_string(),
            V::F32(number) => number.value.to_string(),
            V::String(text) => text.value.clone(),
            V::Hash(hash) => self.hash(hash.value),
            V::WadChunkLink(file) => self.file(file.value.0),
            other => {
                let step = format!("{{{}}}", crate::name_of(other.kind()).unwrap_or("?"));
                return Err(self.error_under(&step, ErrorKind::UnrenderableKey));
            }
        })
    }

    fn struct_pin(&mut self, pin: &str, value: &values::Struct) -> Result<Value, Error> {
        let class = value.class_hash;
        let mut fields = IndexMap::from([(
            "class".to_owned(),
            Value::String(spelled32(class, self.names.class(class))),
        )]);
        let mut set = IndexMap::with_capacity(value.properties.len());
        for (&field, property) in &value.properties {
            let name = self
                .names
                .field(field, Some(class))
                .filter(|name| names_field(name, field))
                .map_or_else(|| format!("0x{:08x}", *field), Cow::into_owned);
            let rendered = self.under(&name, |this| this.value(property))?;
            set.insert(name, rendered);
        }
        if !set.is_empty() {
            fields.insert("set".to_owned(), Value::Mapping(set));
        }
        Ok(Value::Mapping(IndexMap::from([(
            pin.to_owned(),
            Value::Mapping(fields),
        )])))
    }

    fn hash(&self, hash: BinHash) -> String {
        spelled32(hash, self.names.hash(hash))
    }

    fn file(&self, chunk: u64) -> String {
        match self.names.file(chunk) {
            Some(name) if hash64_of(&name) == chunk => name.into_owned(),
            _ => format!("0x{chunk:016x}"),
        }
    }

    /// Runs `render` one step further down the path.
    fn under<T>(
        &mut self,
        step: &str,
        render: impl FnOnce(&mut Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let length = self.path.len();
        push_step(&mut self.path, step);
        let rendered = render(self);
        self.path.truncate(length);
        rendered
    }

    fn error(&self, kind: ErrorKind) -> Error {
        Error::at_key(kind, self.path.clone())
    }

    fn error_under(&self, step: &str, kind: ErrorKind) -> Error {
        let mut path = self.path.clone();
        push_step(&mut path, step);
        Error::at_key(kind, path)
    }
}

/// Appends one step in the path grammar: a `.` before a field, none before a subscript.
fn push_step(path: &mut String, step: &str) {
    if !(path.is_empty() || step.starts_with(['[', '{'])) {
        path.push('.');
    }
    path.push_str(step);
}

/// `name` where it reads back as `hash`, else `0x` and 8 hexadecimal digits.
fn spelled32(hash: BinHash, name: Option<Cow<'_, str>>) -> String {
    match name {
        Some(name) if hash32_of(&name) == hash => name.into_owned(),
        _ => format!("0x{:08x}", *hash),
    }
}

/// Whether `name` is one field name a `set` key spells, naming `field`.
///
/// A name spelled `0x` and 8 hexadecimal digits names the field with that hash.
fn names_field(name: &str, field: BinHash) -> bool {
    let Ok(path) = PropertyPath::new(name) else {
        return false;
    };
    let mut segments = path.segments();
    matches!(
        (segments.next(), segments.next()),
        (Some(segment), None) if segment.subscript.is_none() && hash32_of(segment.name) == field
    )
}

/// The float with the shortest spelling that rounds to the bits of `single`.
fn single(single: f32) -> Value {
    // An `f32` displays as the shortest decimal that reads back to it, where its widening to
    // `f64` displays every digit of the binary fraction: `0.1` against `0.10000000149011612`.
    Value::Float(single.to_string().parse().unwrap_or(f64::from(single)))
}

fn singles(components: &[f32]) -> Value {
    Value::List(components.iter().copied().map(single).collect())
}
