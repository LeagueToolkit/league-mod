//! A [`Value`] written as YAML text.

use serde::{Serialize, Serializer};
use serde_saphyr::FlowSeq;

use crate::{Error, ErrorKind, Value};

impl Value {
    /// The value as YAML text, one node starting at column 0 with no trailing newline.
    ///
    /// A mapping and a list are written in block style, and a list whose items are all
    /// scalars in flow style. A string YAML reads as another type is quoted.
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
