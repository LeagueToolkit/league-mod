//! The serialized declaration document an archive carries as layer metadata.
//!
//! [`DeclarationDocument`] holds the document as JSON. [`Module`] and [`Bindings`] are the
//! serialized shapes of the executed [`crate::Module`], [`Edit`], and [`EntryEdit`]; every
//! override path in a document is layer-relative.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::{
    Declarations, Edit, EntryEdit, EntryName, Error, LinkEdit, LinkPath, Origin, OverridePath,
    Selector, Target,
};

/// An archive's versioned declarations, including fields an older consumer cannot execute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeclarationDocument(serde_json::Value);

impl DeclarationDocument {
    /// Validates the complete layer declarations before execution.
    pub fn parse(&self) -> Result<Declarations, Error> {
        let declarations: Declarations = serde_json::from_value(self.0.clone())
            .map_err(|error| Error::new("game_data", error))?;
        declarations.validate()?;
        Ok(declarations)
    }
}

impl From<Declarations> for DeclarationDocument {
    fn from(declarations: Declarations) -> Self {
        Self(
            serde_json::to_value(declarations)
                .expect("declarations contain JSON-compatible fields"),
        )
    }
}

/// The serialized form of [`crate::Module`]. The selector keys match the manifest.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Module {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<Target>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edits: Option<Vec<Edit>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entries: Option<IndexMap<EntryName, EntryEdit>>,
    origin: Origin,
}

impl TryFrom<Module> for crate::Module {
    type Error = Error;

    fn try_from(module: Module) -> Result<Self, Error> {
        let at = format!(
            "{}: module {}",
            module.origin.manifest, module.origin.module_index
        );
        let selector = match (
            SelectorKey::one(&at, module.target, module.entries)?,
            module.edits,
        ) {
            (SelectorKey::Target(target), Some(edits)) => Selector::Target { target, edits },
            (SelectorKey::Target(_), None) => {
                return Err(Error::new(at, "target requires `edits`"));
            }
            (SelectorKey::Entries(entries), None) => Selector::Entries(entries),
            (SelectorKey::Entries(_), Some(_)) => {
                return Err(Error::new(at, "entries and `edits` are mutually exclusive"));
            }
        };
        Ok(Self {
            selector,
            origin: module.origin,
        })
    }
}

impl From<crate::Module> for Module {
    fn from(module: crate::Module) -> Self {
        let (target, edits, entries) = match module.selector {
            Selector::Target { target, edits } => (Some(target), Some(edits), None),
            Selector::Entries(entries) => (None, None, Some(entries)),
        };
        Self {
            target,
            edits,
            entries,
            origin: module.origin,
        }
    }
}

/// The one selector key a module carries.
#[derive(Debug)]
pub(crate) enum SelectorKey<T, E> {
    Target(T),
    Entries(E),
}

impl<T, E> SelectorKey<T, E> {
    /// Exactly one of `target` and `entries`, or the error for a module `at`.
    pub(crate) fn one(at: &str, target: Option<T>, entries: Option<E>) -> Result<Self, Error> {
        match (target, entries) {
            (Some(target), None) => Ok(Self::Target(target)),
            (None, Some(entries)) => Ok(Self::Entries(entries)),
            (Some(_), Some(_)) => Err(Error::new(at, "target and entries are mutually exclusive")),
            (None, None) => Err(Error::new(at, "module requires target or entries")),
        }
    }
}

/// The compact binding body shared by [`Edit`] and [`EntryEdit`].
///
/// An override path is carried as spelled. A document body holds layer-relative paths; a
/// manifest or source body holds paths relative to the file naming them.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bindings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) overrides: Option<Vec<String>>,
    #[serde(
        default,
        rename = "links",
        alias = "+links",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) add_links: Option<Vec<LinkPath>>,
    #[serde(default, rename = "-links", skip_serializing_if = "Option::is_none")]
    pub(crate) remove_links: Option<Vec<LinkPath>>,
}

impl Bindings {
    /// Whether any binding key is written, empty or not.
    pub(crate) fn is_present(&self) -> bool {
        self.overrides.is_some() || self.add_links.is_some() || self.remove_links.is_some()
    }

    fn into_links(self) -> LinkEdit {
        LinkEdit {
            add: self.add_links.unwrap_or_default(),
            remove: self.remove_links.unwrap_or_default(),
        }
    }

    fn from_links(links: LinkEdit) -> Self {
        Self {
            overrides: None,
            add_links: (!links.add.is_empty()).then_some(links.add),
            remove_links: (!links.remove.is_empty()).then_some(links.remove),
        }
    }

    /// The edit of a body whose override paths resolve through `resolve`.
    pub(crate) fn into_edit(
        mut self,
        resolve: impl Fn(&str) -> Result<OverridePath, Error>,
    ) -> Result<Edit, Error> {
        let overrides = self
            .overrides
            .take()
            .unwrap_or_default()
            .iter()
            .map(|path| resolve(path))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Edit {
            overrides,
            links: self.into_links(),
        })
    }
}

impl TryFrom<Bindings> for Edit {
    type Error = Error;

    /// A document body: every override path is layer-relative.
    fn try_from(bindings: Bindings) -> Result<Self, Error> {
        bindings.into_edit(|path| OverridePath::try_from(path))
    }
}

impl From<Edit> for Bindings {
    fn from(edit: Edit) -> Self {
        Self {
            overrides: (!edit.overrides.is_empty())
                .then(|| edit.overrides.into_iter().map(String::from).collect()),
            ..Self::from_links(edit.links)
        }
    }
}

impl TryFrom<Bindings> for EntryEdit {
    type Error = Error;

    fn try_from(bindings: Bindings) -> Result<Self, Error> {
        if bindings.overrides.is_some() {
            return Err(Error::new(
                "entries",
                "overrides is not permitted inside entries",
            ));
        }
        Ok(Self {
            links: bindings.into_links(),
        })
    }
}

impl From<EntryEdit> for Bindings {
    fn from(edit: EntryEdit) -> Self {
        Self::from_links(edit.links)
    }
}
