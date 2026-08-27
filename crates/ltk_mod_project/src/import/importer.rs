//! [`ProjectImporter`]: the format-neutral importing driver.

use std::fmt;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};

use super::{ImportFormat, ImportProgress, ImportReporter, ImportTarget};
use crate::{Cancellation, ModProject, ModProjectError, ModProjectLayer};

/// The config file an import writes into the project directory.
const CONFIG_FILE_NAME: &str = "mod.config.json";

/// Failure to import a packaged mod as a mod project.
///
/// The variants here are the driver's: they can occur whatever format is being
/// imported. Format-specific failures arrive through the transparent
/// [`Format`](Self::Format) variant, so matching on a concrete format's error is
/// one level deep: `ImportError::Format(inner)`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ImportError<E> {
    /// A directory could not be created under the output directory.
    #[error("Failed to write {path}")]
    Write {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    /// The imported project's `mod.config.json` could not be written.
    #[error(transparent)]
    Config(#[from] ModProjectError),

    /// The cancellation given to
    /// [`with_cancellation`](ProjectImporter::with_cancellation) answered
    /// `true`. The output directory holds however much of the project had been
    /// written, and removing it is the caller's.
    #[error("The import was cancelled")]
    Cancelled,

    /// The config hook given to
    /// [`try_with_config`](ProjectImporter::try_with_config) refused the project
    /// the archive described.
    ///
    /// The output directory holds the content the format extracted, as it does
    /// after a cancellation: the hook judges what was decoded, so it can only
    /// answer once the format has run. Removing the directory is the caller's.
    ///
    /// The reason is the caller's own error, boxed. A caller that wants its type
    /// back downcasts: `error.downcast_ref::<MyError>()`.
    #[error("The import was refused")]
    Refused(#[source] ConfigRefusal),

    /// The format failed to decode the archive; see the format's own error
    /// type for the cases.
    #[error(transparent)]
    Format(E),
}

/// Why a config hook refused an import.
///
/// Boxed rather than a type parameter of its own: a second parameter would
/// spell itself out in every signature that names an [`ImportError`], to carry
/// a value most imports never produce.
pub type ConfigRefusal = Box<dyn std::error::Error + Send + Sync>;

impl<E> ImportError<E> {
    fn write(path: impl Into<Utf8PathBuf>, source: io::Error) -> Self {
        Self::Write {
            path: path.into(),
            source,
        }
    }
}

/// The config hook of an importer that was given none.
///
/// A function pointer rather than a closure, so the type can be named: it is
/// [`ProjectImporter`]'s default type parameter, and a caller writing
/// `ProjectImporter::new(dir)` names it without knowing it.
pub type NoConfig = fn(&mut ModProject) -> Result<(), ConfigRefusal>;

fn no_config(_: &mut ModProject) -> Result<(), ConfigRefusal> {
    Ok(())
}

/// Materializes a packaged mod as a mod project directory.
///
/// The importer is format-neutral: it owns everything about laying out the
/// project (the output directory, the base layer's content directory, a
/// directory for every layer the project declares, the config write) and
/// everything a caller wants of an import whatever the format (progress, a name
/// of its own, cancellation). An [`ImportFormat`] implementation owns everything
/// about decoding its archive. See the [module docs](crate::import) for an
/// example.
///
/// It is the mirror of [`ProjectPacker`](crate::ProjectPacker), and reads the
/// same way round: the driver is built around the project - there for the
/// packer, here for the importer - and the format is the argument of the call
/// that does the work.
///
/// | | [`ProjectPacker`](crate::ProjectPacker) | `ProjectImporter` |
/// |---|---|---|
/// | built around | the project and its root | the output directory |
/// | format supplied to | [`pack`](crate::ProjectPacker::pack) | [`import`](Self::import) |
/// | progress | [`pack_with_progress`](crate::ProjectPacker::pack_with_progress) | [`import_with_progress`](Self::import_with_progress) |
///
/// The import runs on the calling thread and reports itself through the
/// callback [`import_with_progress`](Self::import_with_progress) takes, so a
/// caller driving it from a worker builds the importer there and forwards the
/// reports to wherever its UI lives.
pub struct ProjectImporter<'a, C = NoConfig> {
    output_dir: Utf8PathBuf,
    configure: C,
    cancellation: Cancellation<'a>,
}

impl<'a> ProjectImporter<'a> {
    /// Create an importer that will lay the project out in `output_dir`,
    /// creating the directory if needed.
    pub fn new(output_dir: impl Into<Utf8PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            configure: no_config as NoConfig,
            cancellation: Cancellation::NEVER,
        }
    }
}

impl<'a, C> ProjectImporter<'a, C> {
    /// Adjust the project the archive describes before it is written.
    ///
    /// The config is written once, at the end of the import, so a caller giving
    /// the project a name of its own - the directory it chose, the display name
    /// a user typed - edits it here rather than saving the file a second time.
    /// What `configure` leaves alone is what the archive said.
    ///
    /// Owned rather than borrowed, and stored as a type parameter rather than
    /// boxed: the hook configures the run, so it belongs to the importer for as
    /// long as the importer lives, and nothing about a one-shot run needs it on
    /// the heap.
    ///
    /// Use [`try_with_config`](Self::try_with_config) for a hook that can look
    /// at what the archive said and refuse it.
    #[must_use]
    pub fn with_config<D>(
        self,
        configure: D,
    ) -> ProjectImporter<'a, impl FnOnce(&mut ModProject) -> Result<(), ConfigRefusal>>
    where
        D: FnOnce(&mut ModProject),
    {
        self.try_with_config(move |project| {
            configure(project);
            Ok(())
        })
    }

    /// Adjust the project the archive describes, or refuse it.
    ///
    /// As [`with_config`](Self::with_config), except that the hook can answer
    /// `Err`, which fails the import with [`ImportError::Refused`] and leaves
    /// the config unwritten. That is the only point at which a caller has seen
    /// what the archive actually holds and can still stop: a package whose name
    /// collides with one already installed, a version the caller does not
    /// support, metadata that fails a rule of the caller's own.
    ///
    /// The content the format extracted is on disk by then, as it is after a
    /// cancellation, and removing it is the caller's.
    #[must_use]
    pub fn try_with_config<D>(self, configure: D) -> ProjectImporter<'a, D>
    where
        D: FnOnce(&mut ModProject) -> Result<(), ConfigRefusal>,
    {
        ProjectImporter {
            output_dir: self.output_dir,
            configure,
            cancellation: self.cancellation,
        }
    }

    /// Stop the import as soon as `cancellation` reads as cancelled.
    ///
    /// Checked between the import's steps and between the archive's entries, so
    /// a cancellation lands between files rather than part-way through one. It
    /// fails the import with [`ImportError::Cancelled`], leaving a part-written
    /// output directory for the caller to remove.
    ///
    /// One archive entry is the finest granularity there is, so a format storing
    /// a whole WAD as a single entry - as Fantome stores a packed WAD - reads the
    /// cancellation only once that entry is unpacked.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: impl Into<Cancellation<'a>>) -> Self {
        self.cancellation = cancellation.into();
        self
    }

    /// The directory the project will be laid out in.
    pub fn output_dir(&self) -> &Utf8Path {
        &self.output_dir
    }
}

impl<C> ProjectImporter<'_, C>
where
    C: FnOnce(&mut ModProject) -> Result<(), ConfigRefusal>,
{
    /// Materialize `format`'s archive as a mod project in the output directory.
    ///
    /// On success the project's config has been written into the output
    /// directory and is returned.
    ///
    /// Use [`import_with_progress`](Self::import_with_progress) to watch a long
    /// import as it runs.
    ///
    /// # Errors
    ///
    /// Driver failures (a directory that could not be created, a config that
    /// could not be written, a cancellation, a config hook that refused) surface
    /// as [`ImportError`]'s own variants; a failure inside the format surfaces as
    /// [`ImportError::Format`], except for the format's own report of a
    /// cancellation, which is folded into [`ImportError::Cancelled`] so that one
    /// cancellation has one error.
    pub fn import<F: ImportFormat>(self, format: F) -> Result<ModProject, ImportError<F::Error>> {
        self.import_reporting(format, ImportReporter::new(None))
    }

    /// Materialize `format`'s archive as a mod project, reporting each step to
    /// `progress`.
    ///
    /// The extraction reports a unit of the archive's content at a time and the
    /// steps past it report themselves, all on the importing thread. See
    /// [`ImportProgress`] for what the counters mean.
    ///
    /// The callback is borrowed rather than owned because an import is one-shot:
    /// there is nothing to keep it alive past this call, and a caller drawing a
    /// bar usually wants to write straight into state it already holds.
    ///
    /// # Errors
    ///
    /// As [`import`](Self::import).
    pub fn import_with_progress<F: ImportFormat>(
        self,
        format: F,
        progress: &mut dyn FnMut(ImportProgress<'_>),
    ) -> Result<ModProject, ImportError<F::Error>> {
        self.import_reporting(format, ImportReporter::new(Some(progress)))
    }

    fn import_reporting<F: ImportFormat>(
        self,
        format: F,
        mut progress: ImportReporter<'_>,
    ) -> Result<ModProject, ImportError<F::Error>> {
        let Self {
            output_dir,
            configure,
            cancellation,
        } = self;

        let target = ImportTarget::new(&output_dir, cancellation);

        create_dir(&output_dir)?;
        create_dir(&target.base_layer_dir())?;

        let mut project = match format.import(&target, &mut progress) {
            Ok(project) => project,
            Err(error) if F::is_cancellation(&error) => return Err(ImportError::Cancelled),
            Err(error) => return Err(ImportError::Format(error)),
        };

        if target.is_cancelled() {
            return Err(ImportError::Cancelled);
        }

        configure(&mut project).map_err(ImportError::Refused)?;

        // Every layer the config declares needs a directory, whether or not the
        // archive held content for it: `ProjectPacker` refuses a project with a
        // declared layer it cannot find, so an import that skipped one would
        // write a project that can never be packed again. After `configure`, so
        // that a layer the caller added gets one too.
        for layer in &project.layers {
            create_dir(&ModProjectLayer::content_path(&output_dir, &layer.name))?;
        }

        project.save(&output_dir.join(CONFIG_FILE_NAME))?;

        progress.report_complete();

        Ok(project)
    }
}

impl<C> fmt::Debug for ProjectImporter<'_, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProjectImporter")
            .field("output_dir", &self.output_dir)
            .field("cancellation", &self.cancellation)
            .finish_non_exhaustive()
    }
}

fn create_dir<E>(path: &Utf8Path) -> Result<(), ImportError<E>> {
    std::fs::create_dir_all(path).map_err(|source| ImportError::write(path, source))
}
