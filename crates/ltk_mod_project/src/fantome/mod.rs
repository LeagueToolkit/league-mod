//! Packing mod projects to the Fantome format, and importing archives back.
//!
//! This module requires the `fantome` feature to be enabled.
//!
//! The format itself lives in the `ltk_fantome` crate: this module provides
//! the Fantome backends for the crate's format-neutral traits, composing
//! that crate's `FantomeWriter` and `FantomeReader`:
//!
//! - [`FantomeFormat`] implements [`PackFormat`](crate::PackFormat), driven
//!   by [`ProjectPacker`](crate::ProjectPacker).
//! - [`FantomeImporter`] implements [`ImportFormat`](crate::ImportFormat)
//!   and turns an archive into a project directory.
//!
//! Fantome stores only the base layer; use
//! [`ModProject::non_base_layers`](crate::ModProject::non_base_layers) to
//! warn about layers a pack will drop.

mod convert;
mod import;
mod pack;

#[cfg(test)]
mod tests;

pub use convert::{fantome_info_from_project, project_from_fantome_info};
pub use import::{FantomeImportError, FantomeImporter};
pub use ltk_fantome::WadHashtable;
pub use pack::{FantomeFormat, FantomePackError};
