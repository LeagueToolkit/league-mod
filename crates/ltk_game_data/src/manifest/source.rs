//! A source file named by a manifest module.

use serde::{Deserialize, Deserializer, de};

use crate::{
    Edit, Error,
    document::{Accepts, Fields},
};

use super::{Body, DocumentPath, Reading, Version};

/// A source file: its own version and one body. The manifest supplies its target.
#[derive(Debug)]
pub(super) struct Source {
    body: Body,
}

impl<'de> Deserialize<'de> for Source {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let accepts = Accepts {
            version: true,
            edits: true,
            ..Accepts::default()
        };
        let fields: Fields<()> = deserializer.deserialize_map(Fields::visitor(accepts))?;
        let version = fields
            .version
            .ok_or_else(|| de::Error::missing_field("version"))?;
        Version::try_from(version).map_err(de::Error::custom)?;
        Ok(Self {
            body: Body::from(fields),
        })
    }
}

impl Source {
    /// The edits of the source at `path`. An error names the source as its document.
    pub(super) fn load(path: DocumentPath<'_>, text: &str) -> Result<Vec<Edit>, Error> {
        let source: Self = path.parse(text, Reading::Execution)?;
        source
            .body
            .into_edits(path)
            .map_err(|error| error.document(path.as_str()))
    }
}
