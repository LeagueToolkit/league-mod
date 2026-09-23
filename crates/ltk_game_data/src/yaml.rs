//! A [`Value`] written as YAML text.

use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_saphyr::{FlowMap, FlowSeq, Tagged};

use crate::{Error, ErrorKind, Value};

impl Value {
    /// The value as YAML text, one node starting at column 0 with no trailing newline.
    ///
    /// A mapping and a list are written in block style, and a list whose items are all
    /// scalars in flow style. A string YAML reads as another type is quoted. A struct pin is
    /// written as its struct tag, `!pointer(C)` over the fields of its `set`.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Serialize`] for an integer outside the `i64` and `u64` ranges, which no
    /// loaded or rendered value holds.
    pub fn to_yaml(&self) -> Result<String, Error> {
        let mut text = serde_saphyr::to_string(&Yaml(self)).map_err(|error| {
            Error::new(ErrorKind::Serialize {
                detail: error.to_string(),
            })
        })?;
        text.truncate(text.trim_end().len());
        Ok(text)
    }
}

/// A value serialized in the layout [`Value::to_yaml`] writes.
struct Yaml<'a>(&'a Value);

impl Serialize for Yaml<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some((tag, set)) = struct_tag(self.0) {
            return match set {
                Set::Fields(set) => Tagged(Yaml(set), Some(tag)).serialize(serializer),
                Set::Empty => Tagged(FlowMap(Empty), Some(tag)).serialize(serializer),
                Set::Null => Tagged((), Some(tag)).serialize(serializer),
            };
        }
        match self.0 {
            Value::List(items) if items.iter().all(is_scalar) => {
                FlowSeq(items).serialize(serializer)
            }
            Value::List(items) => serializer.collect_seq(items.iter().map(Yaml)),
            Value::Mapping(mapping) => {
                serializer.collect_map(mapping.iter().map(|(key, value)| (key, Yaml(value))))
            }
            scalar => scalar.serialize(serializer),
        }
    }
}

fn is_scalar(value: &Value) -> bool {
    !matches!(value, Value::List(_) | Value::Mapping(_))
}

/// The struct tag and the `set` a struct pin is written as, or `None` for any other value.
///
/// A pin with a `class` and no `set` is written over the empty mapping, `!pointer(C) {}`, and
/// a pin with neither is the null pointer, `!pointer null`. A class spelled with a character
/// outside `class_in_tag` stays in the pin's document form.
fn struct_tag(value: &Value) -> Option<(String, Set<'_>)> {
    let (name @ ("pointer" | "embed"), fields) = value.pinned()? else {
        return None;
    };
    let (class, set) = match fields {
        Value::Null => (None, Set::Null),
        Value::Mapping(fields) if fields.keys().all(|key| key == "class" || key == "set") => {
            let class = match fields.get("class") {
                None => None,
                Some(Value::String(class)) if class_in_tag(class) => Some(class.as_str()),
                Some(_) => return None,
            };
            let set = match (fields.get("set"), class) {
                (Some(Value::Mapping(set)), _) if set.is_empty() => Set::Empty,
                (Some(set), _) => Set::Fields(set),
                (None, Some(_)) => Set::Empty,
                (None, None) => Set::Null,
            };
            (class, set)
        }
        _ => return None,
    };
    let tag = match class {
        Some(class) => format!("!{name}({class})"),
        None => format!("!{name}"),
    };
    Some((tag, set))
}

/// The node a struct tag stands on.
enum Set<'a> {
    /// The pin's `set`.
    Fields(&'a Value),
    /// The empty mapping in flow style, for an empty `set` or a pin with a `class` and no `set`.
    Empty,
    /// Null, for the null pointer.
    Null,
}

/// The empty mapping.
struct Empty;

impl Serialize for Empty {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_map(Some(0))?.end()
    }
}

/// Whether `class` is nonempty and spelled with characters a YAML tag carries unescaped.
fn class_in_tag(class: &str) -> bool {
    !class.is_empty()
        && class
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '/'))
}
