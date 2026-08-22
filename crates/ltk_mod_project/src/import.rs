//! Importing a packaged mod as a mod project.

use camino::Utf8Path;

use crate::ModProject;

/// An archive format whose packages can be materialized as mod projects.
///
/// The counterpart of `PackFormat`: an implementation is constructed around
/// its input (usually a reader over an archive) and consumed by one import.
/// The `fantome` cargo feature provides one (`fantome::FantomeImporter`).
///
/// Implementations should also expose `import` as an inherent method, with
/// the trait impl forwarding to it: direct callers then do not need the
/// trait in scope, and the trait serves code generic over formats.
pub trait ImportFormat {
    /// Format-specific failures.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Materialize the archive as a mod project laid out in `output_dir`,
    /// creating the directory if needed.
    ///
    /// On success the project's config has been written into `output_dir`
    /// and is returned.
    fn import(self, output_dir: &Utf8Path) -> Result<ModProject, Self::Error>;
}
