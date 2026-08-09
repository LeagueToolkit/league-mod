//! High-level utilities for packing mod projects to `.modpkg` format.
//!
//! This module requires the `project` feature to be enabled.
//!
//! Packing is filtered by the project's `.modignore` files, if any exist
//! (at the project root and nested inside `content/`): files their
//! gitignore-style patterns exclude are not packed, and the ignore files
//! themselves are never packed. [`PackOptions`] controls the filter, and
//! the skipped entries are reported on [`PackResult::ignored_files`].
//!
//! # Example
//!
//! ```ignore
//! use ltk_modpkg::project::ProjectPacker;
//! use camino::Utf8PathBuf;
//!
//! let project_root = Utf8PathBuf::from("my-mod");
//!
//! ProjectPacker::new(project_root)?
//!     .pack("build/my-mod_1.0.0.modpkg".into())?;
//! ```

mod packer;
pub mod thumbnail;

#[cfg(test)]
mod tests;

pub use packer::{IgnoreMode, PackOptions, ProjectPacker};
pub use thumbnail::{load_thumbnail, ThumbnailError, MAX_THUMBNAIL_SIZE};

use crate::builder::ModpkgBuilderError;
use crate::error::{EncodingError, InvalidSlugError, ReadFileError, StripPrefixError};
use camino::Utf8Path;
use camino::Utf8PathBuf;
use ltk_mod_project::{ModProject, ModProjectError, PackageFormat};
use std::io;

// ---------------------------------------------------------------------------
// Error & result types
// ---------------------------------------------------------------------------

/// Error type for project packing operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PackError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    ReadFile(#[from] ReadFileError),

    #[error(transparent)]
    Builder(#[from] ModpkgBuilderError),

    #[error("Config file not found in project directory: {0}")]
    ConfigNotFound(Utf8PathBuf),

    #[error(transparent)]
    Config(#[from] ModProjectError),

    #[error("Layer directory missing: {layer} at {path}")]
    LayerDirMissing { layer: String, path: Utf8PathBuf },

    #[error("Invalid layer name")]
    InvalidLayerName(#[source] InvalidSlugError),

    #[error("Base layer must have priority 0, got: {0}")]
    InvalidBaseLayerPriority(i32),

    #[error(transparent)]
    Thumbnail(#[from] ThumbnailError),

    #[error("Invalid mod version")]
    InvalidVersion(#[from] semver::Error),

    /// The project's `.modignore` could not be loaded: the file is
    /// unreadable, or a pattern in it does not parse. A broken pattern fails
    /// the pack rather than silently shipping files the author excluded.
    #[error(transparent)]
    Ignore(#[from] ltk_mod_project::ModIgnoreError),

    /// A content directory or file could not be read during the scan.
    #[error(transparent)]
    Walk(#[from] ltk_mod_project::ContentWalkError),

    /// An [`IgnoreMode::Explicit`] filter was built for a different project
    /// root than the packer was given, so it would relate every scanned path
    /// to the wrong content dir and quietly filter wrong.
    #[error("Ignore filter is rooted at {filter_root}, but the project's content dir is {project_content_dir}")]
    IgnoreRootMismatch {
        filter_root: Utf8PathBuf,
        project_content_dir: Utf8PathBuf,
    },

    #[error(transparent)]
    Encoding(#[from] EncodingError),

    #[error("Path has no file name: {0}")]
    MissingFileName(Utf8PathBuf),

    #[error(transparent)]
    StripPrefix(#[from] StripPrefixError),
}

/// Result of a successful pack operation.
#[derive(Debug)]
pub struct PackResult {
    output_path: Utf8PathBuf,
    ignored: Vec<Utf8PathBuf>,
}

impl PackResult {
    pub(crate) fn new(output_path: impl Into<Utf8PathBuf>, ignored: Vec<Utf8PathBuf>) -> Self {
        Self {
            output_path: output_path.into(),
            ignored,
        }
    }

    /// The path to the created `.modpkg` file.
    pub fn output_path(&self) -> &Utf8Path {
        &self.output_path
    }

    /// Consume the result, yielding the output path.
    pub fn into_output_path(self) -> Utf8PathBuf {
        self.output_path
    }

    /// The entries `.modignore` excluded from the archive, in traversal
    /// order. A pruned directory appears once; the files beneath it are not
    /// enumerated.
    pub fn ignored_files(&self) -> &[Utf8PathBuf] {
        &self.ignored
    }

    /// `ignored_files().len()`, for reporting.
    pub fn ignored_count(&self) -> usize {
        self.ignored.len()
    }
}

// ---------------------------------------------------------------------------
// Convenience functions
// ---------------------------------------------------------------------------

/// Create a standard modpkg file name from a mod project.
///
/// If `custom_name` is provided, it will be used (with `.modpkg` extension added if missing).
/// Otherwise, generates `{name}_{version}.modpkg`.
pub fn create_file_name(mod_project: &ModProject, custom_name: Option<String>) -> String {
    mod_project.package_file_name(custom_name, PackageFormat::Modpkg)
}

/// Pack a mod project directory to a `.modpkg` file.
///
/// Loads the config from `project_root` automatically.
/// This is a convenience wrapper around [`ProjectPacker`].
pub fn pack_from_project(
    project_root: &Utf8Path,
    output_path: &Utf8Path,
) -> Result<PackResult, PackError> {
    ProjectPacker::new(project_root.to_owned())?.pack(output_path)
}

/// Pack a mod project to a `.modpkg` file with an already-loaded config.
///
/// Use this when you have a [`ModProject`] from another source.
pub fn pack_from_project_with_config(
    project_root: &Utf8Path,
    output_path: &Utf8Path,
    mod_project: &ModProject,
) -> Result<PackResult, PackError> {
    ProjectPacker::with_mod_project(mod_project.clone(), project_root.to_owned())?.pack(output_path)
}
