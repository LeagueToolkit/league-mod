//! How far a pack has got, as it reports itself.

use std::fmt;

/// What a pack is doing.
///
/// Emitted in order: [`Scanning`](Self::Scanning) once per layer,
/// [`Writing`](Self::Writing) once per content file, and
/// [`Complete`](Self::Complete) once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PackStage {
    /// Walking a layer's directory under `content/` and filtering it through
    /// the project's `.modignore`.
    Scanning,
    /// Writing a content file into the archive.
    Writing,
    /// The archive is finished.
    Complete,
}

/// How far a pack has got.
///
/// The counters are per stage, because the two stages count different things:
/// layers while [`Scanning`](PackStage::Scanning), content files while
/// [`Writing`](PackStage::Writing). How many files there are is not known until
/// the scan finishes, which is why the two cannot be one counter.
///
/// Deliberately not `#[non_exhaustive]`, where [`PackStage`] is: a caller only
/// reads a progress report, and sealing this would stop it constructing one in
/// its own tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackProgress<'a> {
    /// What the pack is doing.
    pub stage: PackStage,
    /// The layer being scanned, or the file being written, as its path inside
    /// the archive. `None` for a step naming nothing.
    pub current_item: Option<&'a str>,
    /// Steps of this stage finished so far, counting from 0.
    pub current: u32,
    /// How many steps this stage has.
    pub total: u32,
}

/// Where a [`PackFormat`](crate::PackFormat) reports what it is writing.
///
/// Counting is the driver's: it walked the project and knows how many files the
/// plan holds, so a format only says which file it has reached.
pub struct PackReporter<'a> {
    progress: Option<&'a mut dyn FnMut(PackProgress<'_>)>,
    written: u32,
    total: u32,
}

impl<'a> PackReporter<'a> {
    pub(crate) fn new(progress: Option<&'a mut dyn FnMut(PackProgress<'_>)>) -> Self {
        Self {
            progress,
            written: 0,
            total: 0,
        }
    }

    /// Tell the reporter how many files the plan holds, once the scan is done.
    pub(crate) fn set_total(&mut self, total: u32) {
        self.total = total;
    }

    /// Report the layer the scan has reached, before it is walked.
    pub(crate) fn report_layer(&mut self, name: &str, index: u32, total: u32) {
        self.emit(PackProgress {
            stage: PackStage::Scanning,
            current_item: Some(name),
            current: index,
            total,
        });
    }

    /// Report that the archive is finished.
    pub(crate) fn report_complete(&mut self) {
        let total = self.total;
        self.emit(PackProgress {
            stage: PackStage::Complete,
            current_item: None,
            current: total,
            total,
        });
    }

    /// Report the content file the format has reached, before writing it.
    ///
    /// `archive_path` is the file's path inside the archive, as
    /// [`PlannedFile::rel_path`](crate::PlannedFile::rel_path) gives it.
    pub fn report_file(&mut self, archive_path: &str) {
        let (current, total) = (self.written, self.total);
        self.written += 1;
        self.emit(PackProgress {
            stage: PackStage::Writing,
            current_item: Some(archive_path),
            current,
            total,
        });
    }

    fn emit(&mut self, progress: PackProgress<'_>) {
        if let Some(report) = self.progress.as_mut() {
            report(progress);
        }
    }
}

impl fmt::Debug for PackReporter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PackReporter")
            .field("has_progress", &self.progress.is_some())
            .field("written", &self.written)
            .field("total", &self.total)
            .finish()
    }
}
