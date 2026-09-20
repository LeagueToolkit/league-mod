//! The literal a property edit carries.
//!
//! A [`Value`] is what the author wrote: null, boolean, integer, float, string, list, or
//! mapping, in spelled order. The build reads it by the property's type. A YAML local tag on a
//! value loads as the one-key mapping of its name, the type pin's document form.
//!
//! Two one-key mappings mean more than a mapping. A type name pins the type the value reads
//! as, and `ref` names a value of the installed game to read instead of a literal.

use std::fmt;

use indexmap::IndexMap;
use ltk_meta::PropertyKind;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, SeqAccess, Visitor},
    ser,
};

use crate::{ErrorKind, Reference};

/// The literal of a property edit, or of one element, key, or field inside it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// `null` in YAML and JSON. TOML has no null.
    Null,
    Bool(bool),
    /// An integer of the union of the `i64` and `u64` ranges.
    Integer(i128),
    /// A float, or an integer past the integer ranges.
    Float(f64),
    String(String),
    /// A list in spelled order.
    List(Vec<Value>),
    /// A mapping in spelled order. A duplicate key does not deserialize.
    Mapping(IndexMap<String, Value>),
}

/// The one-key mapping key a reference spells. Not a type name, so not a pin.
pub(crate) const REFERENCE_KEY: &str = "ref";

/// The type names a pin spells, each with the kind it names.
const TYPE_NAMES: [(&str, PropertyKind); 23] = [
    ("bool", PropertyKind::Bool),
    ("i8", PropertyKind::I8),
    ("i16", PropertyKind::I16),
    ("i32", PropertyKind::I32),
    ("i64", PropertyKind::I64),
    ("u8", PropertyKind::U8),
    ("u16", PropertyKind::U16),
    ("u32", PropertyKind::U32),
    ("u64", PropertyKind::U64),
    ("f32", PropertyKind::F32),
    ("vec2", PropertyKind::Vector2),
    ("vec3", PropertyKind::Vector3),
    ("vec4", PropertyKind::Vector4),
    ("mtx44", PropertyKind::Matrix44),
    ("rgba", PropertyKind::Color),
    ("string", PropertyKind::String),
    ("hash", PropertyKind::Hash),
    ("file", PropertyKind::WadChunkLink),
    ("link", PropertyKind::ObjectLink),
    ("flag", PropertyKind::BitBool),
    ("option", PropertyKind::Optional),
    ("pointer", PropertyKind::Struct),
    ("embed", PropertyKind::Embedded),
];

/// The kind a type name names, or `None` for a name that is not a type name.
#[must_use]
pub fn kind_named(name: &str) -> Option<PropertyKind> {
    TYPE_NAMES
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, kind)| *kind)
}

/// The type name of a kind, or `None` for a container kind, which no pin names.
#[must_use]
pub fn name_of(kind: PropertyKind) -> Option<&'static str> {
    TYPE_NAMES
        .iter()
        .find(|(_, candidate)| *candidate == kind)
        .map(|(name, _)| *name)
}

impl Value {
    /// The type name of a one-key mapping whose key is a type name, or `None`.
    ///
    /// The build reads such a mapping as a pin on every property whose type is not a struct.
    #[must_use]
    pub fn pin(&self) -> Option<&str> {
        self.pinned().map(|(name, _)| name)
    }

    /// The pinned value of a one-key mapping whose key is a type name.
    #[must_use]
    pub(crate) fn pinned(&self) -> Option<(&str, &Value)> {
        match self {
            Self::Mapping(mapping) if mapping.len() == 1 => mapping
                .iter()
                .next()
                .map(|(key, value)| (key.as_str(), value))
                .filter(|(key, _)| kind_named(key).is_some()),
            _ => None,
        }
    }

    /// Whether the value is a struct pin: a one-key mapping keyed `pointer` or `embed`.
    #[must_use]
    pub fn is_struct_pin(&self) -> bool {
        matches!(self.pin(), Some("pointer" | "embed"))
    }

    /// The text of a one-key mapping keyed `ref`, or `None`.
    ///
    /// The build reads such a mapping as the value the text names in the installed game,
    /// whatever the property's type. `ref` is not a type name, so a reference is not a pin
    /// and [`pin`](Self::pin) does not report one.
    #[must_use]
    pub fn reference(&self) -> Option<&str> {
        match self {
            Self::Mapping(mapping) if mapping.len() == 1 => match mapping.iter().next() {
                Some((key, Self::String(text))) if key == REFERENCE_KEY => Some(text.as_str()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Every reference in the value, in spelled order, duplicates included.
    ///
    /// A reference sits anywhere a value does, so this descends lists, mappings, and the
    /// `set` of a struct pin. A `ref` whose text does not parse is not one, and is left for
    /// the build to report where the author can see which key it was under.
    #[must_use]
    pub fn references(&self) -> Vec<Reference> {
        let mut found = Vec::new();
        self.collect_references(&mut found);
        found
    }

    fn collect_references(&self, found: &mut Vec<Reference>) {
        if let Some(text) = self.reference() {
            if let Ok(reference) = Reference::parse(text) {
                found.push(reference);
            }
            return;
        }
        match self {
            Self::List(items) => items.iter().for_each(|item| item.collect_references(found)),
            Self::Mapping(mapping) => mapping
                .values()
                .for_each(|item| item.collect_references(found)),
            _ => {}
        }
    }

    /// Whether the value is a one-key mapping keyed `ref`, whatever its value.
    ///
    /// [`reference`](Self::reference) answers only for the well-formed shape, so the check
    /// that refuses a malformed one asks this first.
    fn is_reference_key(&self) -> bool {
        match self {
            Self::Mapping(mapping) if mapping.len() == 1 => mapping
                .keys()
                .next()
                .is_some_and(|key| key == REFERENCE_KEY),
            _ => false,
        }
    }

    /// Checks every struct pin and every reference in the value.
    ///
    /// A `pointer` pin's value is null, the empty mapping, or a mapping; an `embed` pin's
    /// value is a mapping. The mapping holds `class`, a string, or `set`, a mapping, or both,
    /// and nothing else. The empty mapping is the null pointer. A `ref` key's value is a
    /// string [`Reference::parse`](crate::Reference::parse) accepts.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::StructPinShape`] for a struct pin of any other shape, and
    /// [`ErrorKind::ReferenceShape`] for a `ref` of any other shape.
    pub fn check_pins(&self) -> Result<(), ErrorKind> {
        if self.is_reference_key() {
            return match self.reference() {
                Some(text) => Reference::parse(text)
                    .map(|_| ())
                    .map_err(|error| error.kind),
                None => Err(ErrorKind::ReferenceShape),
            };
        }
        match self {
            Self::List(items) => items.iter().try_for_each(Self::check_pins),
            Self::Mapping(mapping) => match self.pinned() {
                Some((name @ ("pointer" | "embed"), value)) => match value {
                    Self::Null if name == "pointer" => Ok(()),
                    Self::Mapping(fields) => {
                        if (fields.is_empty() && name == "embed")
                            || fields
                                .keys()
                                .any(|key| !matches!(key.as_str(), "class" | "set"))
                            || fields
                                .get("class")
                                .is_some_and(|class| !matches!(class, Self::String(_)))
                        {
                            return Err(ErrorKind::StructPinShape);
                        }
                        match fields.get("set") {
                            None => Ok(()),
                            Some(Self::Mapping(set)) => set.values().try_for_each(Self::check_pins),
                            Some(_) => Err(ErrorKind::StructPinShape),
                        }
                    }
                    _ => Err(ErrorKind::StructPinShape),
                },
                _ => mapping.values().try_for_each(Self::check_pins),
            },
            _ => Ok(()),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Integer(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value:?}"),
            Self::String(value) => write!(f, "{value:?}"),
            Self::List(items) => {
                f.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Self::Mapping(mapping) => {
                f.write_str("{")?;
                for (index, (key, value)) in mapping.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{key}: {value}")?;
                }
                f.write_str("}")
            }
        }
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Integer(value) => match (i64::try_from(*value), u64::try_from(*value)) {
                (Ok(value), _) => serializer.serialize_i64(value),
                (_, Ok(value)) => serializer.serialize_u64(value),
                _ => Err(ser::Error::custom(format!(
                    "integer {value} is outside the i64 and u64 ranges"
                ))),
            },
            Self::Float(value) => serializer.serialize_f64(*value),
            Self::String(value) => serializer.serialize_str(value),
            Self::List(items) => serializer.collect_seq(items),
            Self::Mapping(mapping) => serializer.collect_map(mapping),
        }
    }
}

/// The YAML core tag namespace. A tag in it is the parser's own typing, not a pin.
const YAML_CORE: &str = "tag:yaml.org,2002:";

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let serde_saphyr::Tagged(value, tag) =
            serde_saphyr::Tagged::<Untagged>::deserialize(deserializer)?;
        let value = value.0;
        let Some(tag) = tag else {
            return Ok(value);
        };
        if tag.starts_with(YAML_CORE) {
            return Ok(value);
        }
        let name = tag.trim_start_matches('!');
        if kind_named(name).is_none() && name != REFERENCE_KEY {
            return Err(de::Error::custom(format!("unknown type pin `{tag}`")));
        }
        Ok(Self::Mapping(IndexMap::from([(name.to_owned(), value)])))
    }
}

/// A value read without its tag.
struct Untagged(Value);

impl<'de> Deserialize<'de> for Untagged {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor).map(Untagged)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a null, boolean, number, string, list, or mapping")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        Value::deserialize(deserializer)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Integer(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Integer(value.into()))
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<Value, E> {
        if i64::try_from(value).is_ok() || u64::try_from(value).is_ok() {
            Ok(Value::Integer(value))
        } else {
            Ok(Value::Float(value as f64))
        }
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<Value, E> {
        match u64::try_from(value) {
            Ok(value) => Ok(Value::Integer(value.into())),
            Err(_) => Ok(Value::Float(value as f64)),
        }
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Ok(Value::Float(value))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut items = Vec::with_capacity(sequence.size_hint().unwrap_or(0));
        while let Some(item) = sequence.next_element()? {
            items.push(item);
        }
        Ok(Value::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut mapping = IndexMap::with_capacity(map.size_hint().unwrap_or(0));
        while let Some((key, value)) = map.next_entry::<String, Value>()? {
            if mapping.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!("duplicate key `{key}`")));
            }
        }
        Ok(Value::Mapping(mapping))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_pins_are_checked_anywhere_in_a_value() {
        let ok: Value = serde_json::from_str(
            r#"{"a": [{"pointer": null}, {"pointer": {}}, {"pointer": {"class": "X"}}], "b": {"embed": {"set": {"c": {"pointer": {"class": "Y", "set": {}}}}}}}"#,
        )
        .unwrap();
        ok.check_pins().unwrap();
        for text in [
            r#"{"pointer": 5}"#,
            r#"{"embed": null}"#,
            r#"{"embed": {}}"#,
            r#"{"pointer": {"class": 1}}"#,
            r#"{"pointer": {"set": []}}"#,
            r#"{"pointer": {"class": "X", "extra": 1}}"#,
            r#"[{"embed": {"set": {"f": {"pointer": "X"}}}}]"#,
        ] {
            let value: Value = serde_json::from_str(text).unwrap();
            assert_eq!(value.check_pins(), Err(ErrorKind::StructPinShape), "{text}");
        }
    }

    #[test]
    fn integers_serialize_by_range() {
        assert_eq!(
            serde_json::to_string(&Value::Integer(u64::MAX.into())).unwrap(),
            "18446744073709551615"
        );
        assert_eq!(serde_json::to_string(&Value::Integer(-1)).unwrap(), "-1");
        assert_eq!(serde_json::to_string(&Value::Null).unwrap(), "null");
    }

    #[test]
    fn an_integer_outside_both_ranges_refuses_to_serialize() {
        for value in [i128::MAX, i128::MIN, i128::from(u64::MAX) + 1] {
            let error = serde_json::to_string(&Value::Integer(value)).unwrap_err();
            assert!(
                error.to_string().contains("outside the i64 and u64 ranges"),
                "{error}"
            );
        }
    }
}
