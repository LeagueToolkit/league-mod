//! Packing a mod project into an archive.
//!
//! Packing is split between a format-neutral driver and per-format backends:
//!
//! - [`ProjectPacker`] is the single entry point. It loads the config,
//!   validates the layer layout, walks `content/` once with the project's
//!   `.modignore` filter (per [`PackOptions`]), and resolves the metadata
//!   files (readme, license, thumbnail) into a [`PackPlan`].
//!   The scan and the write both report themselves through the callback
//!   [`pack_with_progress`](ProjectPacker::pack_with_progress) takes.
//! - A [`PackFormat`] implementation turns that plan into an archive. The
//!   `modpkg` and `fantome` cargo features each provide one
//!   (`modpkg::ModpkgFormat`, `fantome::FantomeFormat`), and the trait is
//!   public API: an external crate can implement it against the plan's
//!   accessors and be driven the same way.
//!
//! Driver failures and format failures stay separate in the error type:
//! [`PackError`] carries the shared variants once, and its transparent
//! `Format` variant carries the backend's own error.
//!
//! ```no_run
//! use ltk_mod_project::{PackFormat, PackFormatReport, PackPlan, PackReporter, ProjectPacker};
//!
//! /// A toy format: counts the files a pack would write.
//! struct EntryCount<'a>(&'a mut usize);
//!
//! impl PackFormat for EntryCount<'_> {
//!     type Error = std::convert::Infallible;
//!
//!     fn pack(
//!         self,
//!         plan: &PackPlan<'_>,
//!         progress: &mut PackReporter<'_>,
//!     ) -> Result<PackFormatReport, Self::Error> {
//!         for layer in plan.layers() {
//!             for file in layer.files() {
//!                 progress.report_file(file.rel_path());
//!                 *self.0 += 1;
//!             }
//!         }
//!         Ok(PackFormatReport::default())
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let packer = ProjectPacker::from_dir("path/to/my-mod")?;
//! let mut count = 0;
//! let report = packer.pack(EntryCount(&mut count))?;
//! println!("{count} files packed, {} ignored", report.ignored_count());
//! # Ok(())
//! # }
//! ```

mod options;
mod packer;
mod plan;
mod progress;

#[cfg(test)]
mod tests;

pub use options::{IgnoreMode, PackOptions};
pub use packer::{PackError, PackReport, ProjectPacker};
pub use plan::{PackPlan, PlannedFile, PlannedHashtable, PlannedLayer, PlannedLicense};
pub use progress::{PackProgress, PackReporter, PackStage};

/// An archive format [`ProjectPacker`] can pack a project into.
///
/// Implementations receive a resolved [`PackPlan`] and only encode: which
/// files are packed, and how the project was filtered and validated, is the
/// driver's job and identical across formats. A format that does not store
/// some part of the plan (Fantome keeps only the base layer, for example)
/// skips it rather than failing.
///
/// The value is consumed: a format is constructed around its output (usually
/// a writer), used for one pack, and finishes its archive before returning.
///
/// Because `pack` consumes `self`, the trait is not dyn-compatible: there is
/// no `dyn PackFormat`. Choosing a format at run time is a `match` that
/// calls [`ProjectPacker::pack`] with the concrete format in each arm.
pub trait PackFormat {
    /// Format-specific failures. Driver failures are not part of this type;
    /// [`ProjectPacker::pack`] wraps both sides in [`PackError`].
    type Error: std::error::Error + Send + Sync + 'static;

    /// Write everything in `plan` that this format stores, and finish the
    /// archive.
    ///
    /// Call [`PackReporter::report_file`] before writing each content file, so
    /// a caller watching a long pack sees where it has got to. A format that
    /// stores only part of the plan reports only what it writes.
    ///
    /// The returned [`PackFormatReport`] carries the facts only the format
    /// knows; the driver folds them into the [`PackReport`] it returns. A
    /// format with nothing to say returns `PackFormatReport::default()`.
    ///
    /// [`PackReport`]: crate::PackReport
    fn pack(
        self,
        plan: &PackPlan<'_>,
        progress: &mut PackReporter<'_>,
    ) -> Result<PackFormatReport, Self::Error>;
}

/// What a format did beyond writing the plan - facts the driver folds into
/// the [`PackReport`](crate::PackReport) it returns.
///
/// Distinct from [`PackReporter`], which streams progress while the pack
/// runs: this is the result, read when it is done.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PackFormatReport {
    /// `game` hashtable names the format omitted because the package's own
    /// chunk table already recovers them. Only the modpkg format trims;
    /// every other format reports 0.
    pub trimmed_game_names: usize,
}
