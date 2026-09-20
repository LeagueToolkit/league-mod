//! Packing mod projects to the `.modpkg` format, and importing packages back.
//!
//! This module requires the `modpkg` feature to be enabled.
//!
//! [`ModpkgFormat`] is the `.modpkg` backend for
//! [`ProjectPacker`](crate::ProjectPacker): the driver scans and filters the
//! project (see the [`pack` module docs](crate::pack)), this module encodes
//! the result through `ltk_modpkg`'s `ModpkgBuilder`.
//!
//! [`ModpkgImporter`] is its backend for
//! [`ProjectImporter`](crate::ProjectImporter), [`read_project`] reads a
//! package's config back without unpacking it at all, and
//! and an [`ExtractionPlan`](ltk_modpkg::ExtractionPlan) answers
//! [`ProjectPaths`](crate::ProjectPaths) so a caller can see where an import
//! would put every file without writing one.

mod convert;
mod format;
mod import;
pub mod thumbnail;

#[cfg(test)]
pub(crate) mod tests;

pub use convert::read_project;
pub use format::{ModpkgFormat, ModpkgPackError};
pub use import::{ModpkgImportError, ModpkgImporter};
pub use thumbnail::{load_thumbnail, ThumbnailError, MAX_THUMBNAIL_SIZE};
