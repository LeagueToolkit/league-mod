//! How far an import has got, as it reports itself.

use std::fmt;

/// What an import is doing.
///
/// Emitted in order: [`Extracting`](Self::Extracting) once per unit of content
/// the archive holds, then [`WritingMetadata`](Self::WritingMetadata) and
/// [`Complete`](Self::Complete) once each.
///
/// Every unit has a name, and the name sits in the variant rather than beside
/// the stage as an `Option`: a format that unpacks something in one pass names
/// the pass, so there is no such thing as extracting an unnamed thing and no
/// state a caller has to tell apart by whether a name happened to be present.
///
/// Deliberately not `#[non_exhaustive]`, where
/// [`PackStage`](crate::PackStage) is: a caller maps every stage onto something
/// of its own - a label, a bar, a spinner - and wants a compile error when one
/// is added, not a `_` arm that quietly renders the new stage as an old one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportStage<'a> {
    /// Unpacking one unit of the archive's content into `content/`. What a unit
    /// is depends on the format: for Fantome each WAD, plus the `RAW/` pass;
    /// for modpkg each layer.
    ///
    /// The counters on [`ImportProgress`] advance with these, and only these,
    /// so they reach the total exactly when the extraction is over.
    Extracting {
        /// The unit being unpacked, as the archive names it.
        item: &'a str,
    },

    /// Writing the files a project keeps at its root: the readme, the license,
    /// the thumbnail and `mod.config.json`.
    WritingMetadata,

    /// Everything the archive holds is on disk.
    Complete,
}

/// How far an import has got.
///
/// The counters describe the extraction, which is the long part of an import
/// and the only part with a count to give. Past it, `current` sits at `total` so
/// a bar drawn from the pair fills rather than restarting. `total` is zero for
/// an archive holding no content at all, which a caller drawing a bar has to
/// read as indeterminate.
///
/// Deliberately not `#[non_exhaustive]`: a caller only reads a progress report,
/// and sealing this would stop it constructing one in its own tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportProgress<'a> {
    /// What the import is doing, and what it is doing it to.
    pub stage: ImportStage<'a>,
    /// Units unpacked so far, counting from 0.
    pub current: u32,
    /// How many units the archive holds.
    pub total: u32,
}

/// Where an [`ImportFormat`](crate::ImportFormat) reports what it is unpacking.
///
/// The counterpart of [`PackReporter`](crate::PackReporter), with the counting
/// the other way round: a pack's total comes from the driver's own scan of the
/// project, where an import's is the archive's to know, so a format says how
/// many units it found ([`set_total`](Self::set_total)) before reporting them
/// one at a time ([`report_item`](Self::report_item)).
///
/// One method per stage, rather than one taking an [`ImportStage`]: only
/// [`report_item`](Self::report_item) advances the counters, and a single entry
/// point would let a format pair a stage with counters that do not belong to
/// it.
pub struct ImportReporter<'a> {
    progress: Option<&'a mut dyn FnMut(ImportProgress<'_>)>,
    reported: u32,
    total: u32,
}

impl<'a> ImportReporter<'a> {
    pub(crate) fn new(progress: Option<&'a mut dyn FnMut(ImportProgress<'_>)>) -> Self {
        Self {
            progress,
            reported: 0,
            total: 0,
        }
    }

    /// Tell the reporter how many units of content the archive holds.
    ///
    /// Repeating it is harmless, so a format learning the total from the first
    /// unit it reaches can set it there rather than counting the archive twice.
    pub fn set_total(&mut self, total: u32) {
        self.total = total;
    }

    /// Report the unit of content the format has reached, before unpacking it.
    ///
    /// The count is the reporter's - one per call - so it cannot disagree with
    /// the number of reports a caller saw. Every unit is named: a format that
    /// unpacks a whole class of entries in one pass reports the pass under the
    /// archive's name for it rather than reporting it nameless.
    pub fn report_item(&mut self, item: &str) {
        let (current, total) = (self.reported, self.total);
        self.reported += 1;
        self.emit(ImportProgress {
            stage: ImportStage::Extracting { item },
            current,
            total,
        });
    }

    /// Report that the content is out and the project's root files are being
    /// written.
    pub fn report_writing_metadata(&mut self) {
        self.emit_at_total(ImportStage::WritingMetadata);
    }

    /// Report that the import is finished, which is the driver's to say: a
    /// format has finished its own part of an import, not the import.
    pub(crate) fn report_complete(&mut self) {
        self.emit_at_total(ImportStage::Complete);
    }

    /// Emit a stage that carries no count of its own, with the counters left
    /// where the last named unit put them.
    fn emit_at_total(&mut self, stage: ImportStage<'_>) {
        let total = self.total;
        self.emit(ImportProgress {
            stage,
            current: total,
            total,
        });
    }

    fn emit(&mut self, progress: ImportProgress<'_>) {
        if let Some(report) = self.progress.as_mut() {
            report(progress);
        }
    }
}

impl fmt::Debug for ImportReporter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportReporter")
            .field("has_progress", &self.progress.is_some())
            .field("reported", &self.reported)
            .field("total", &self.total)
            .finish()
    }
}
