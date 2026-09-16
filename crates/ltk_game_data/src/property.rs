//! A property edit: one signed property path with its value, on one entry.

use indexmap::IndexMap;
use ltk_meta::path::PropertyPath;

use crate::{Error, ErrorKind, Value};

/// The operation of a property edit: the sign in front of its key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Sign {
    /// No sign: the value replaces what the base has.
    #[default]
    Set,
    /// `+`: elements to a list, entries to a map.
    Add,
    /// `-`: elements from a list, entries from a map.
    Remove,
}

impl Sign {
    /// The sign of a key and the key without it.
    #[must_use]
    pub fn of(key: &str) -> (Self, &str) {
        if let Some(rest) = key.strip_prefix('+') {
            (Self::Add, rest)
        } else if let Some(rest) = key.strip_prefix('-') {
            (Self::Remove, rest)
        } else {
            (Self::Set, key)
        }
    }

    /// The sign as spelled: `""`, `"+"`, or `"-"`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Set => "",
            Self::Add => "+",
            Self::Remove => "-",
        }
    }
}

/// One signed property path with its value.
///
/// The path is Riot's property path; the sign is not part of it. The value is the literal as
/// spelled. A one-key mapping keyed by a type name is a pin on every property; any other
/// mapping on a struct-typed property descends into it at apply time.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyEdit {
    /// The property path, without the sign.
    pub path: PropertyPath,
    /// The operation.
    pub sign: Sign,
    /// The literal as spelled.
    pub value: Value,
}

impl PropertyEdit {
    /// The edit of the key `key` with `value`.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidPropertyPath`] for a key whose path does not parse, or
    /// [`ErrorKind::StructPinShape`] for a malformed struct pin anywhere in the value. The
    /// error names the key.
    pub fn parse(key: &str, value: Value) -> Result<Self, Error> {
        let (sign, spelled) = Sign::of(key);
        let path = PropertyPath::new(spelled).map_err(|error| {
            Error::at_key(
                ErrorKind::InvalidPropertyPath {
                    detail: error.to_string(),
                },
                key,
            )
        })?;
        value
            .check_pins()
            .map_err(|kind| Error::at_key(kind, key))?;
        Ok(Self { path, sign, value })
    }

    /// The signed key as spelled.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}{}", self.sign.as_str(), self.path.as_str())
    }

    /// The edits of an entry body, in mapping order.
    pub(crate) fn body(mapping: IndexMap<String, Value>) -> Result<Vec<Self>, Error> {
        mapping
            .into_iter()
            .map(|(key, value)| Self::parse(&key, value))
            .collect()
    }

    /// The entry body of edits, in edit order.
    pub(crate) fn into_body(edits: Vec<Self>) -> IndexMap<String, Value> {
        edits
            .into_iter()
            .map(|edit| (edit.key(), edit.value))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_split_from_keys_and_rejoin() {
        assert_eq!(Sign::of("+a.b[0]"), (Sign::Add, "a.b[0]"));
        assert_eq!(Sign::of("-a"), (Sign::Remove, "a"));
        assert_eq!(Sign::of("a"), (Sign::Set, "a"));
        let edit = PropertyEdit::parse("-a.b[0]", Value::Null).unwrap();
        assert_eq!(edit.key(), "-a.b[0]");
        let error = PropertyEdit::parse("+a[", Value::Null).unwrap_err();
        assert!(matches!(error.kind, ErrorKind::InvalidPropertyPath { .. }));
        assert_eq!(error.location.key.as_deref(), Some("+a["));
    }
}
