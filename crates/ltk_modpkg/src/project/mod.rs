//! High-level utilities for packing mod projects to `.modpkg` format.
//!
//! This module requires the `project` feature to be enabled.
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

pub use packer::ProjectPacker;
pub use thumbnail::{load_thumbnail, MAX_THUMBNAIL_SIZE};

use crate::builder::ModpkgBuilderError;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use ltk_mod_project::ModProject;
use std::io;

// ---------------------------------------------------------------------------
// Error & result types
// ---------------------------------------------------------------------------

/// Error type for project packing operations.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Builder error: {0}")]
    Builder(#[from] ModpkgBuilderError),

    #[error("Config file not found in project directory: {0}")]
    ConfigNotFound(Utf8PathBuf),

    #[error("Failed to load project config: {0}")]
    ConfigError(String),

    #[error("Layer directory missing: {layer} at {path}")]
    LayerDirMissing { layer: String, path: Utf8PathBuf },

    #[error("Invalid layer name: {0}")]
    InvalidLayerName(String),

    #[error("Base layer must have priority 0, got: {0}")]
    InvalidBaseLayerPriority(i32),

    #[error("Failed to process thumbnail: {0}")]
    ThumbnailError(String),

    #[error("Invalid version format: {0}")]
    InvalidVersion(String),

    #[error("Glob pattern error: {0}")]
    GlobError(#[from] glob::PatternError),

    #[error("Invalid UTF-8 path: {0}")]
    InvalidUtf8Path(String),
}

/// Result of a successful pack operation.
#[derive(Debug)]
pub struct PackResult {
    /// The path to the created `.modpkg` file.
    pub output_path: Utf8PathBuf,
}

// ---------------------------------------------------------------------------
// Convenience functions
// ---------------------------------------------------------------------------

/// Create a standard modpkg file name from a mod project.
///
/// If `custom_name` is provided, it will be used (with `.modpkg` extension added if missing).
/// Otherwise, generates `{name}_{version}.modpkg`.
pub fn create_file_name(mod_project: &ModProject, custom_name: Option<String>) -> String {
    match custom_name {
        Some(name) => {
            if name.ends_with(".modpkg") {
                name
            } else {
                format!("{}.modpkg", name)
            }
        }
        None => {
            format!("{}_{}.modpkg", mod_project.name, mod_project.version)
        }
    }
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
