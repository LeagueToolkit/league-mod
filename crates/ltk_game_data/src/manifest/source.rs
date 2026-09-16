//! A source file named by a manifest module.

use serde::Deserialize;

use crate::{Edit, Error};

use super::{Body, DocumentPath, Reading, Version};

/// A source file: its own version and one body. The manifest supplies its target.
/// Unknown keys are refused by the flattened bindings.
#[derive(Debug, Deserialize)]
pub(super) struct Source {
    #[expect(dead_code, reason = "the version is validated on deserialization")]
    version: Version,
    #[serde(flatten)]
    body: Body,
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
