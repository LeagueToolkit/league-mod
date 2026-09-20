//! Importing a packaged mod as a mod project.
//!
//! Importing is split between a format-neutral driver and per-format backends,
//! the mirror of [packing](crate::pack):
//!
//! - [`ProjectImporter`] is the single entry point. It is built around the
//!   output directory, as [`ProjectPacker`](crate::ProjectPacker) is built
//!   around the project it packs. It creates that directory and the base
//!   layer's content directory, runs the backend, hands the project it decoded
//!   to the caller's config hook, gives every layer that project declares a
//!   directory, and writes `mod.config.json`.
//! - An [`ImportFormat`] implementation decodes one archive into that
//!   directory. The `modpkg` and `fantome` cargo features each provide one
//!   (`modpkg::ModpkgImporter`, `fantome::FantomeImporter`), and the trait is
//!   public API: an external crate can implement it against [`ImportTarget`]
//!   and [`ImportReporter`] and be driven the same way.
//!
//! Everything a caller of any format wants - progress, a name of its own for
//! the project, a way to stop - is the driver's, so a format cannot forget one
//! and three call sites cannot each remember a different default.
//!
//! Driver failures and format failures stay separate in the error type:
//! [`ImportError`] carries the shared variants once, and its transparent
//! `Format` variant carries the backend's own error.
//!
//! ```no_run
//! use ltk_mod_project::{ImportStage, ProjectImporter};
//! use ltk_mod_project::fantome::FantomeImporter;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = std::fs::File::open("my-mod.fantome")?;
//! let project = ProjectImporter::new("my-mod")
//!     .with_config(|project| project.name = "my-mod".to_owned())
//!     .import_with_progress(FantomeImporter::new(file), &mut |progress| {
//!         let (done, total) = (progress.current, progress.total);
//!         match progress.stage {
//!             ImportStage::Extracting { item } => println!("{done}/{total}: {item}"),
//!             ImportStage::WritingMetadata => println!("writing metadata"),
//!             ImportStage::Complete => println!("done"),
//!         }
//!     })?;
//! println!("imported {}", project.name);
//! # Ok(())
//! # }
//! ```

mod importer;
mod paths;
mod progress;
mod target;

#[cfg(test)]
mod tests;

pub use importer::{ConfigRefusal, ImportError, NoConfig, ProjectImporter};
pub use paths::{ProjectPath, ProjectPaths};
pub use progress::{ImportProgress, ImportReporter, ImportStage};
pub use target::ImportTarget;

use crate::ModProject;

/// An archive format [`ProjectImporter`] can materialize as a mod project.
///
/// Implementations receive an [`ImportTarget`] and only decode: creating the
/// project directory, applying the caller's edits and writing the config is the
/// driver's job and identical across formats. A format that stores no
/// counterpart for some part of a project (Fantome keeps content for the base
/// layer alone, for example) simply writes less.
///
/// The value is consumed: a format is constructed around its input (usually a
/// reader over an archive) and used for one import.
///
/// Because `import` consumes `self`, the trait is not dyn-compatible: there is
/// no `dyn ImportFormat`. Choosing a format at run time is a `match` that calls
/// [`ProjectImporter::import`] with the concrete format in each arm.
pub trait ImportFormat {
    /// Format-specific failures. Driver failures are not part of this type;
    /// [`ProjectImporter::import`] wraps both sides in [`ImportError`].
    type Error: std::error::Error + Send + Sync + 'static;

    /// Decode the archive into the directory `target` names, and return the
    /// project its metadata describes.
    ///
    /// The output directory and the base layer's content directory already
    /// exist. The config file is the driver's to write: what this returns is
    /// what lands in it, after the caller's own edits.
    ///
    /// A format writes content only for the layers the archive holds; the
    /// driver gives every layer the returned project declares a directory, so a
    /// format need not create one for a layer it found nothing for.
    ///
    /// Call [`ImportReporter::set_total`] once the archive's units of content
    /// are known and [`ImportReporter::report_item`] before unpacking each, so
    /// a caller watching a long import sees where it has got to. Content the
    /// format unpacks in one pass is one unit, reported under the archive's name
    /// for it, so the counters reach the total when the extraction is over.
    fn import(
        self,
        target: &ImportTarget<'_>,
        progress: &mut ImportReporter<'_>,
    ) -> Result<ModProject, Self::Error>;

    /// Whether `error` is this format's report of the cancellation
    /// [`ImportTarget::is_cancelled`] answered `true` to.
    ///
    /// The driver folds such an error into [`ImportError::Cancelled`], so one
    /// cancellation has one error however deep in the import it landed. A
    /// format with no cancelled variant leaves this alone.
    fn is_cancellation(error: &Self::Error) -> bool {
        let _ = error;
        false
    }
}
