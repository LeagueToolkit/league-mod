//! [`ImportTarget`]: the directory an import writes into.

use camino::{Utf8Path, Utf8PathBuf};

use crate::{Cancellation, ModProjectLayer, CONTENT_DIR_NAME};

/// The directory an import writes into, and whether it has been asked to stop.
///
/// Handed to an [`ImportFormat`](super::ImportFormat) by
/// [`ProjectImporter`](super::ProjectImporter), and the counterpart of the
/// [`PackPlan`](crate::PackPlan) a format is handed to write: the resolved
/// description of the job, which the format reads and does not change. The
/// directories it names already exist.
#[derive(Debug, Clone, Copy)]
pub struct ImportTarget<'a> {
    output_dir: &'a Utf8Path,
    cancellation: Cancellation<'a>,
}

impl<'a> ImportTarget<'a> {
    pub(crate) fn new(output_dir: &'a Utf8Path, cancellation: Cancellation<'a>) -> Self {
        Self {
            output_dir,
            cancellation,
        }
    }

    /// The project directory being written, which exists.
    pub fn output_dir(&self) -> &'a Utf8Path {
        self.output_dir
    }

    /// The directory every layer's content sits under, `content/`.
    ///
    /// Only the base layer's is created up front; a format writing another
    /// layer creates it.
    pub fn content_dir(&self) -> Utf8PathBuf {
        self.output_dir.join(CONTENT_DIR_NAME)
    }

    /// The base layer's content directory, `content/base/`, which exists.
    ///
    /// Created whatever the archive holds, because a project whose base layer
    /// has no directory is one [`ProjectPacker`](crate::ProjectPacker) refuses,
    /// and an archive carrying metadata alone would otherwise import into one.
    pub fn base_layer_dir(&self) -> Utf8PathBuf {
        ModProjectLayer::content_path(self.output_dir, ModProjectLayer::BASE_NAME)
    }

    /// Whether the caller has asked the import to stop.
    ///
    /// A format checks this between its steps and fails with its own cancelled
    /// error, which the driver folds into
    /// [`ImportError::Cancelled`](super::ImportError::Cancelled).
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// The cancellation, for handing to something that takes one of its own.
    pub fn cancellation(&self) -> Cancellation<'a> {
        self.cancellation
    }
}
