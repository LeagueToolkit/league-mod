//! The edits a manifest module or a source file carries.

use serde::{Deserialize, Serialize};

use crate::{Edit, Error, document::Bindings};

use super::DocumentPath;

/// The edits of a document body: the edit list under the `edits` key, or compact bindings
/// as one edit.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct Body {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) edits: Option<Vec<Bindings>>,
    #[serde(flatten)]
    pub(super) bindings: Bindings,
}

impl Body {
    /// Whether the body carries an edit list or any binding key.
    pub(super) fn is_present(&self) -> bool {
        self.edits.is_some() || self.bindings.is_present()
    }

    /// The body's edits, with override paths resolved against `document`. Every edit carries
    /// at least one binding.
    pub(super) fn into_edits(
        self,
        at: &str,
        document: DocumentPath<'_>,
    ) -> Result<Vec<Edit>, Error> {
        let resolve = |path: &str| document.resolve_override(path);
        if let Some(edits) = self.edits {
            if self.bindings.is_present() {
                return Err(Error::new(
                    at,
                    "`edits` and compact bindings are mutually exclusive",
                ));
            }
            if edits.is_empty() {
                return Err(Error::new(at, "`edits` requires at least one edit"));
            }
            return edits
                .into_iter()
                .enumerate()
                .map(|(index, edit)| {
                    let at = format!("{at}: edit {index}");
                    if !edit.is_present() {
                        return Err(Error::new(at, "edit requires at least one binding"));
                    }
                    edit.into_edit(resolve)
                        .map_err(|error| Error::new(at, error))
                })
                .collect();
        }
        if !self.bindings.is_present() {
            return Err(Error::new(
                at,
                "module requires bindings, `edits`, or source",
            ));
        }
        Ok(vec![
            self.bindings
                .into_edit(resolve)
                .map_err(|error| Error::new(at, error))?,
        ])
    }
}
