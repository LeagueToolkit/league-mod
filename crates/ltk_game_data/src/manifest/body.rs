//! The edits a manifest module or a source file carries.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    Edit, Error, ErrorKind,
    document::{Accepts, Bindings, Fields},
};

use super::DocumentPath;

/// The edits of a document body: the edit list under the `edits` key, or compact bindings
/// as one edit.
#[derive(Debug, Default, Serialize)]
pub(super) struct Body {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) edits: Option<Vec<Bindings>>,
    #[serde(flatten)]
    pub(super) bindings: Bindings,
}

impl<'de> Deserialize<'de> for Body {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let accepts = Accepts {
            edits: true,
            ..Accepts::default()
        };
        deserializer
            .deserialize_map(Fields::<()>::visitor(accepts))
            .map(Self::from)
    }
}

impl<E> From<Fields<E>> for Body {
    fn from(fields: Fields<E>) -> Self {
        Self {
            edits: fields.edits,
            bindings: fields.bindings,
        }
    }
}

impl Body {
    /// Whether the body carries an edit list or any binding key.
    pub(super) fn is_present(&self) -> bool {
        self.edits.is_some() || self.bindings.is_present()
    }

    /// The body's edits, with override paths resolved against `document`. Every edit carries
    /// at least one binding. An error names its edit index and no document.
    pub(super) fn into_edits(self, document: DocumentPath<'_>) -> Result<Vec<Edit>, Error> {
        let resolve = |path: &str| document.resolve_override(path);
        if let Some(edits) = self.edits {
            if self.bindings.is_present() {
                return Err(Error::new(ErrorKind::MixedBodies));
            }
            if edits.is_empty() {
                return Err(Error::new(ErrorKind::EditsEmpty));
            }
            return edits
                .into_iter()
                .enumerate()
                .map(|(index, edit)| {
                    if !edit.is_present() {
                        return Err(Error::new(ErrorKind::EditWithoutBindings).edit(index));
                    }
                    edit.into_edit(resolve).map_err(|error| error.edit(index))
                })
                .collect();
        }
        if !self.bindings.is_present() {
            return Err(Error::new(ErrorKind::ModuleWithoutBindings));
        }
        Ok(vec![self.bindings.into_edit(resolve)?])
    }
}
