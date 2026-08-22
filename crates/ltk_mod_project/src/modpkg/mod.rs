//! Packing mod projects to the `.modpkg` format.
//!
//! This module requires the `modpkg` feature to be enabled.
//!
//! [`ModpkgFormat`] is the `.modpkg` backend for
//! [`ProjectPacker`](crate::ProjectPacker): the driver scans and filters the
//! project (see the [`pack` module docs](crate::pack)), this module encodes
//! the result through `ltk_modpkg`'s `ModpkgBuilder`.

mod format;
pub mod thumbnail;

#[cfg(test)]
mod tests;

pub use format::{ModpkgFormat, ModpkgPackError};
pub use thumbnail::{load_thumbnail, ThumbnailError, MAX_THUMBNAIL_SIZE};
