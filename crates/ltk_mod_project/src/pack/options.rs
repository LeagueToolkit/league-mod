//! Options shared by every packing format.
//!
//! The options here are format-independent: they describe how the project's
//! content is selected, not how an archive is written.

use crate::ModIgnore;

/// Options for packing a mod project.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PackOptions {
    /// Where the `.modignore` filter comes from.
    pub ignore: IgnoreMode,
}

impl PackOptions {
    /// Replace the ignore mode. The struct is `#[non_exhaustive]`, so this
    /// is how callers outside the crate set it.
    #[must_use]
    pub fn with_ignore(mut self, ignore: IgnoreMode) -> Self {
        self.ignore = ignore;
        self
    }
}

/// Where a pack operation's `.modignore` filter comes from.
#[derive(Debug, Clone, Default)]
pub enum IgnoreMode {
    /// Load `<project_root>/.modignore`; a missing file filters nothing.
    #[default]
    FromProject,
    /// Pack everything, whatever `.modignore` says.
    Disabled,
    /// Use a filter the caller already built. It must have been constructed
    /// with the same project root the pack operation is given; packing fails
    /// with `PackError::IgnoreRootMismatch` otherwise, rather than letting
    /// the filter silently match nothing.
    Explicit(ModIgnore),
}
