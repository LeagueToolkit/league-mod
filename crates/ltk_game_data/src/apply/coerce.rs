//! Coercion: the reading of a value as the shape of its property.
//!
//! The rules are the coercion table of `docs/design/game-data.md` section 6. A value that no
//! row accepts is a [`PropertySkipReason`], never a wrapped or narrowed value.

use glam::{Mat4, Vec2, Vec3, Vec4};
use ltk_hash::{BinHash, WadHash};
use ltk_meta::{PropertyKind as K, PropertyValueEnum as V, path::PropertyPath, property::values};

use crate::{Schema, Shape, Value, kind_named, path_hash};

use super::PropertySkipReason as Reason;

/// A coerced value, or why the value does not coerce.
pub(super) type Coerced = Result<V, Reason>;

/// The game's copy of each entry the edits reference, read once before any edit applies.
///
/// A reference reads the game, never the target being built, so every reference of a batch
/// answers from one reading taken before the first edit. An entry the caller does not supply
/// is absent here, which is what `ReferenceMissingEntry` reports.
///
/// Keyed by object hash, so the path form and the `0x` form of one entry are one key.
pub(super) type ResolvedReferences = indexmap::IndexMap<BinHash, ltk_meta::BinObject>;

/// Coerces values against a schema.
#[derive(Clone, Copy)]
pub(super) struct Coercer<'a> {
    pub(super) schema: &'a dyn Schema,
    pub(super) references: &'a ResolvedReferences,
}

impl Coercer<'_> {
    /// Reads `value` as `shape`. `base` is the property's base value, the class source of a
    /// struct pin without `class`.
    ///
    /// Every element, key, field, and operand reaches its own value through here, so the
    /// reference row is read once and holds everywhere a value is.
    pub(super) fn coerce(&self, value: &Value, shape: Shape, base: Option<&V>) -> Coerced {
        if let Some(text) = value.reference() {
            return self.referenced(text, shape);
        }
        match value.pinned() {
            Some((name, inner)) => self.pinned(name, inner, shape, base),
            None => self.bare(value, shape),
        }
    }

    /// Reads the game's copy of the value `text` names.
    ///
    /// The game's value carries its own kinds, so it is read as it is rather than coerced.
    /// What the property asks of it is that the two shapes agree.
    fn referenced(&self, text: &str, shape: Shape) -> Coerced {
        // Loading parses every reference it reads, so this only refuses one built by hand.
        let reference = crate::Reference::parse(text).map_err(|_| Reason::ReferenceUnresolved)?;
        let object = self
            .references
            .get(&reference.entry.object_hash())
            .ok_or(Reason::ReferenceMissingEntry)?;
        let value = object
            .resolve(&reference.path)
            .map_err(|_| Reason::ReferenceUnresolved)?;
        if Shape::of(value) == shape {
            Ok(value.clone())
        } else {
            Err(Reason::KindMismatch)
        }
    }

    /// Reads a pinned value: the pin fixes the kind, then the bare rules apply.
    fn pinned(&self, name: &str, inner: &Value, shape: Shape, base: Option<&V>) -> Coerced {
        // A pin fixes the kind a literal reads as. A reference has no literal spelling and
        // carries the game's own kinds, so a pin over one asks for nothing and is refused.
        // Without this the inner value would reach `bare` as a mapping and read as a kind
        // mismatch, which names the shape rather than the pin that is the real fault.
        if inner.reference().is_some() {
            return Err(Reason::PinMismatch);
        }
        let pin = kind_named(name).expect("a pin names a kind");
        match shape.kind {
            K::Struct | K::Embedded => match name {
                "pointer" | "embed" if pin == shape.kind => {
                    self.struct_pin(inner, shape.kind, base)
                }
                _ => Err(Reason::PinMismatch),
            },
            K::Container | K::UnorderedContainer | K::Map => {
                if Some(pin) != shape.item {
                    return Err(Reason::PinMismatch);
                }
                self.bare(inner, shape)
            }
            K::Optional if name == "option" => match inner {
                Value::Mapping(fields) if fields.is_empty() => self.optional(&Value::Null, shape),
                inner => self.optional(inner, shape),
            },
            // A struct pin on an option of structs is the element's own spelling, so the
            // element keeps it. Any other pin names the item kind and wraps a bare element.
            K::Optional if Some(pin) == shape.item => {
                let element = match pin {
                    K::Struct | K::Embedded => {
                        Value::Mapping([(name.to_owned(), inner.clone())].into())
                    }
                    _ => inner.clone(),
                };
                self.optional(&Value::List(vec![element]), shape)
            }
            K::Optional => Err(Reason::PinMismatch),
            kind if pin == kind => self.bare(inner, shape),
            _ => Err(Reason::PinMismatch),
        }
    }

    /// Reads an unpinned value by the row of its shape's kind.
    fn bare(&self, value: &Value, shape: Shape) -> Coerced {
        Ok(match shape.kind {
            K::None => return Err(Reason::KindMismatch),
            K::Bool => match value {
                Value::Bool(flag) => values::Bool::new(*flag).into(),
                _ => return Err(Reason::KindMismatch),
            },
            K::BitBool => match value {
                Value::Bool(flag) => values::BitBool::new(*flag).into(),
                Value::Integer(0) => values::BitBool::new(false).into(),
                Value::Integer(1) => values::BitBool::new(true).into(),
                Value::Integer(_) => return Err(Reason::OutOfRange),
                _ => return Err(Reason::KindMismatch),
            },
            K::I8 => values::I8::new(integer(value)?).into(),
            K::U8 => values::U8::new(integer(value)?).into(),
            K::I16 => values::I16::new(integer(value)?).into(),
            K::U16 => values::U16::new(integer(value)?).into(),
            K::I32 => values::I32::new(integer(value)?).into(),
            K::U32 => values::U32::new(integer(value)?).into(),
            K::I64 => values::I64::new(integer(value)?).into(),
            K::U64 => values::U64::new(integer(value)?).into(),
            K::F32 => values::F32::new(single(value)?).into(),
            K::Vector2 => {
                let [x, y] = singles(value)?;
                values::Vector2::new(Vec2::new(x, y)).into()
            }
            K::Vector3 => {
                let [x, y, z] = singles(value)?;
                values::Vector3::new(Vec3::new(x, y, z)).into()
            }
            K::Vector4 => {
                let [x, y, z, w] = singles(value)?;
                values::Vector4::new(Vec4::new(x, y, z, w)).into()
            }
            K::Matrix44 => {
                let rows: [f32; 16] = singles(value)?;
                values::Matrix44::new(Mat4::from_cols_array(&rows).transpose()).into()
            }
            K::Color => {
                let Value::List(items) = value else {
                    return Err(Reason::KindMismatch);
                };
                let [r, g, b, a]: [u8; 4] = items
                    .iter()
                    .map(integer::<u8>)
                    .collect::<Result<Vec<_>, _>>()?
                    .try_into()
                    .map_err(|_| Reason::ArityMismatch)?;
                values::Color::new(ltk_primitives::Color::new(r, g, b, a)).into()
            }
            K::String => match value {
                Value::String(text) => values::String::new(text.clone()).into(),
                _ => return Err(Reason::KindMismatch),
            },
            K::Hash => values::Hash::new(hash32(value)?).into(),
            K::ObjectLink => values::ObjectLink::new(hash32(value)?).into(),
            K::WadChunkLink => values::WadChunkLink::new(hash64(value)?).into(),
            K::Container | K::UnorderedContainer => self.list(value, shape)?,
            K::Optional => self.optional(value, shape)?,
            K::Map => self.map(value, shape)?,
            K::Struct => match value {
                Value::Null => null_pointer(),
                _ => return Err(Reason::KindMismatch),
            },
            K::Embedded => return Err(Reason::KindMismatch),
        })
    }

    /// Reads a list as a container of the shape's item kind.
    fn list(&self, value: &Value, shape: Shape) -> Coerced {
        let item = shape.item.ok_or(Reason::Untypable)?;
        let Value::List(items) = value else {
            return Err(Reason::KindMismatch);
        };
        let items = items
            .iter()
            .map(|element| self.coerce(element, Shape::bare(item), None))
            .collect::<Result<Vec<_>, _>>()?;
        let container = values::Container::new(item, items).map_err(|_| Reason::KindMismatch)?;
        Ok(if shape.kind == K::UnorderedContainer {
            values::UnorderedContainer(container).into()
        } else {
            container.into()
        })
    }

    /// Reads null, a list of zero or one element, or one value as an option.
    fn optional(&self, value: &Value, shape: Shape) -> Coerced {
        let item = shape.item.ok_or(Reason::Untypable)?;
        let content = match value {
            Value::Null => None,
            Value::List(items) => match items.as_slice() {
                [] => None,
                [one] => Some(self.coerce(one, Shape::bare(item), None)?),
                _ => return Err(Reason::ArityMismatch),
            },
            one => Some(self.coerce(one, Shape::bare(item), None)?),
        };
        values::Optional::new(item, content)
            .map(V::from)
            .map_err(|_| Reason::KindMismatch)
    }

    /// Reads a mapping as a map of the shape's key and value kinds.
    fn map(&self, value: &Value, shape: Shape) -> Coerced {
        let (key, item) = match (shape.key, shape.item) {
            (Some(key), Some(item)) => (key, item),
            _ => return Err(Reason::Untypable),
        };
        let Value::Mapping(mapping) = value else {
            return Err(Reason::KindMismatch);
        };
        let entries = mapping
            .iter()
            .map(|(text, element)| {
                Ok((
                    self.key(text, key)?,
                    self.coerce(element, Shape::bare(item), None)?,
                ))
            })
            .collect::<Result<Vec<_>, Reason>>()?;
        values::Map::new(key, item, entries)
            .map(V::from)
            .map_err(|_| Reason::KindMismatch)
    }

    /// Reads a map key, spelled as text, as the key kind.
    pub(super) fn key(&self, text: &str, kind: K) -> Coerced {
        let value = match kind {
            K::Bool => match text {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => return Err(Reason::InvalidKey),
            },
            K::I8 | K::U8 | K::I16 | K::U16 | K::I32 | K::U32 | K::I64 | K::U64 => {
                Value::Integer(text.parse().map_err(|_| Reason::InvalidKey)?)
            }
            K::F32 => Value::Float(text.parse().map_err(|_| Reason::InvalidKey)?),
            K::String | K::Hash | K::WadChunkLink => Value::String(text.to_owned()),
            _ => return Err(Reason::InvalidKey),
        };
        self.bare(&value, Shape::bare(kind))
    }

    /// Constructs the struct of a `pointer` or `embed` pin from `class` and `set`.
    fn struct_pin(&self, inner: &Value, kind: K, base: Option<&V>) -> Coerced {
        let fields = match inner {
            Value::Null if kind == K::Struct => return Ok(null_pointer()),
            Value::Mapping(fields) if fields.is_empty() && kind == K::Struct => {
                return Ok(null_pointer());
            }
            Value::Mapping(fields) => fields,
            _ => return Err(Reason::KindMismatch),
        };
        let base_class = match base {
            Some(V::Struct(pointer)) if *pointer.class_hash != 0 => Some(pointer.class_hash),
            Some(V::Embedded(values::Embedded(embed))) => Some(embed.class_hash),
            _ => None,
        };
        let class_hash = match fields.get("class") {
            Some(Value::String(name)) => hash32_of(name),
            Some(_) => return Err(Reason::KindMismatch),
            None => base_class.ok_or(Reason::Untypable)?,
        };
        if kind == K::Embedded && base_class.is_some_and(|base| base != class_hash) {
            return Err(Reason::PinMismatch);
        }
        // The shipped bin attests the class it already carries, whether the pin names that
        // class or leaves it out. The schema answers for every other class, and refusing one
        // it does not know keeps a typo out of the written object ([ADR-0022]).
        //
        // [ADR-0022]: https://github.com/LeagueToolkit/league-mod/blob/main/docs/adr/0022-unattested-class-refusal.md
        if base_class != Some(class_hash) && !self.schema.has_class(class_hash) {
            return Err(Reason::UnknownClass);
        }
        let mut properties = indexmap::IndexMap::new();
        if let Some(Value::Mapping(set)) = fields.get("set") {
            for (key, value) in set {
                let path = PropertyPath::new(key.as_str()).map_err(|_| Reason::InvalidPath)?;
                let mut segments = path.segments();
                let field = match (segments.next(), segments.next()) {
                    (Some(segment), None) if segment.subscript.is_none() => segment.name_hash(),
                    _ => return Err(Reason::InvalidPath),
                };
                let shape = self
                    .schema
                    .expected(class_hash, field)
                    .ok_or(Reason::Untypable)?;
                properties.insert(field, self.coerce(value, shape, None)?);
            }
        }
        let value = values::Struct {
            class_hash,
            properties,
        };
        Ok(if kind == K::Embedded {
            values::Embedded(value).into()
        } else {
            value.into()
        })
    }
}

/// The null pointer: a struct of class `0`.
fn null_pointer() -> V {
    values::Struct::default().into()
}

/// The hash of a name, or the hash a `0x` and 8 hexadecimal digits spell.
pub(crate) fn hash32_of(name: &str) -> BinHash {
    hex32(name).unwrap_or_else(|| BinHash::from(name))
}

/// The hash `0x` and exactly 8 hexadecimal digits spell.
fn hex32(text: &str) -> Option<BinHash> {
    hex(text, 8).and_then(|hash| u32::try_from(hash).ok().map(BinHash))
}

/// The hash `0x` and exactly `digits` hexadecimal digits spell.
fn hex(text: &str, digits: usize) -> Option<u64> {
    let spelled = text.strip_prefix("0x")?;
    (spelled.len() == digits && spelled.bytes().all(|c| c.is_ascii_hexdigit()))
        .then(|| u64::from_str_radix(spelled, 16).ok())
        .flatten()
}

/// The hash of a chunk path, or the hash a `0x` and 16 hexadecimal digits spell.
pub(crate) fn hash64_of(path: &str) -> u64 {
    hex(path, 16).unwrap_or_else(|| path_hash(path))
}

/// An integer in the range of `T`.
fn integer<T: TryFrom<i128>>(value: &Value) -> Result<T, Reason> {
    match value {
        Value::Integer(integer) => T::try_from(*integer).map_err(|_| Reason::OutOfRange),
        _ => Err(Reason::KindMismatch),
    }
}

/// A single-precision float: a float rounded, or an integer represented exactly.
fn single(value: &Value) -> Result<f32, Reason> {
    match value {
        Value::Float(float) => Ok(*float as f32),
        Value::Integer(integer) => {
            let single = *integer as f32;
            if single as i128 == *integer {
                Ok(single)
            } else {
                Err(Reason::PrecisionLoss)
            }
        }
        _ => Err(Reason::KindMismatch),
    }
}

/// Exactly `N` single-precision floats from a list.
fn singles<const N: usize>(value: &Value) -> Result<[f32; N], Reason> {
    let Value::List(items) = value else {
        return Err(Reason::KindMismatch);
    };
    items
        .iter()
        .map(single)
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| Reason::ArityMismatch)
}

/// A 32-bit hash: FNV-1a of a lowercased string, a spelled hash, or zero for null and `""`.
fn hash32(value: &Value) -> Result<BinHash, Reason> {
    match value {
        Value::Null => Ok(BinHash(0)),
        Value::String(text) if text.is_empty() => Ok(BinHash(0)),
        Value::String(text) => Ok(hash32_of(text)),
        _ => Err(Reason::KindMismatch),
    }
}

/// A 64-bit hash: XXH64 of a lowercased path, a spelled hash, or zero for null and `""`.
fn hash64(value: &Value) -> Result<WadHash, Reason> {
    match value {
        Value::Null => Ok(WadHash(0)),
        Value::String(text) if text.is_empty() => Ok(WadHash(0)),
        Value::String(text) => Ok(WadHash(hash64_of(text))),
        _ => Err(Reason::KindMismatch),
    }
}
