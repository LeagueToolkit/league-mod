//! [`HashtableEntry`]: the manifest entry as a domain type.

use camino::{Utf8Path, Utf8PathBuf};

use crate::{Algorithm, Category, KeyWidth};

/// One manifest entry: where a table file lives and how its names are keyed.
///
/// Carries no serde on purpose - each container spells the same four fields
/// its own way and converts, the way the container manifests already convert
/// between each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashtableEntry {
    path: Utf8PathBuf,
    category: Category,
    algorithm: Algorithm,
    width: KeyWidth,
}

impl HashtableEntry {
    /// Declare a table at `path`, relative to the container root.
    pub fn new(
        path: impl Into<Utf8PathBuf>,
        category: Category,
        algorithm: Algorithm,
        width: KeyWidth,
    ) -> Self {
        Self {
            path: path.into(),
            category,
            algorithm,
            width,
        }
    }

    /// Where the table file lives, relative to the container root.
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// The lookup domain of the table's names.
    pub fn category(&self) -> &Category {
        &self.category
    }

    /// The hash function keying the table's names.
    pub fn algorithm(&self) -> &Algorithm {
        &self.algorithm
    }

    /// The declared key width.
    pub fn width(&self) -> KeyWidth {
        self.width
    }
}
