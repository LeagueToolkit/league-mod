//! [`ModpkgImporter`]: decodes a `.modpkg` archive into a mod project directory.

use std::fmt;
use std::io::{Read, Seek};

use camino::Utf8Path;
use ltk_modpkg::{
    ChunkDestination, ExtractionPlan, Modpkg, ModpkgError, ModpkgExtractor, PlannedChunk,
};

use super::read_project;
use crate::{
    ImportFormat, ImportReporter, ImportTarget, ModProject, ProjectPath, ProjectPaths,
    CONTENT_DIR_NAME,
};

/// Failure to decode a `.modpkg` archive into a project directory.
///
/// Driver failures (creating the output directory, writing the config) are not
/// here; they surface as the shared variants of
/// [`ImportError`](crate::ImportError).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ModpkgImportError {
    /// The package could not be mounted, read or unpacked.
    #[error(transparent)]
    Modpkg(#[from] ModpkgError),

    /// The import's cancellation answered `true`. The driver folds this into
    /// [`ImportError::Cancelled`](crate::ImportError::Cancelled).
    #[error("The import was cancelled")]
    Cancelled,
}

/// Decodes a `.modpkg` archive into a mod project directory; the modpkg backend
/// for [`ProjectImporter`](crate::ProjectImporter).
///
/// Importing will:
/// 1. Extract each layer's chunks to `content/{layer}/`
/// 2. Extract `README.md`, the license text and `thumbnail.webp` to the project
///    root
///
/// A package stores its chunks under the paths they were packed from, so there
/// is nothing to name and no counterpart here to the Fantome importer's
/// resolver and naming policy.
///
/// The progress unit is the layer: a package's content extracts to `content/`
/// whole rather than a WAD at a time, and the layer is the largest step an
/// unpack can be stopped between. Cancellation lands on the same boundary.
///
/// # Example
///
/// ```no_run
/// use ltk_mod_project::ProjectImporter;
/// use ltk_mod_project::modpkg::ModpkgImporter;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let file = std::fs::File::open("my-mod.modpkg")?;
/// let project = ProjectImporter::new("my-mod").import(ModpkgImporter::new(file))?;
/// println!("imported {}", project.name);
/// # Ok(())
/// # }
/// ```
pub struct ModpkgImporter<R> {
    reader: R,
}

impl<R: Read + Seek> ModpkgImporter<R> {
    /// Create an importer reading the package from `reader`.
    pub fn new(reader: R) -> Self {
        Self { reader }
    }
}

impl<R> fmt::Debug for ModpkgImporter<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModpkgImporter").finish_non_exhaustive()
    }
}

impl<R: Read + Seek> ImportFormat for ModpkgImporter<R> {
    type Error = ModpkgImportError;

    /// # Errors
    ///
    /// [`ModpkgImportError`] covers a malformed package, a chunk that could not
    /// be decompressed or written into the output directory, and a cancellation
    /// that answered `true`.
    fn import(
        self,
        target: &ImportTarget<'_>,
        progress: &mut ImportReporter<'_>,
    ) -> Result<ModProject, Self::Error> {
        let mut modpkg = Modpkg::mount_from_reader(self.reader)?;
        let project = read_project(&mut modpkg)?;

        let output_dir = target.output_dir();
        let content_dir = target.content_dir();
        progress.set_total(project.layers.len() as u32);

        let mut extractor = ModpkgExtractor::new(&mut modpkg);
        for layer in &project.layers {
            if target.is_cancelled() {
                return Err(ModpkgImportError::Cancelled);
            }

            progress.report_item(&layer.name);
            extractor.extract_layer(&layer.name, &content_dir)?;
        }

        if target.is_cancelled() {
            return Err(ModpkgImportError::Cancelled);
        }
        progress.report_writing_metadata();
        extractor.extract_meta(output_dir)?;

        Ok(project)
    }

    fn is_cancellation(error: &Self::Error) -> bool {
        matches!(error, ModpkgImportError::Cancelled)
    }
}

/// A `.modpkg` extraction plan answers where an import would put every chunk.
///
/// `ltk_modpkg`'s [`ExtractionPlan`] already says where a package's chunks land
/// relative to the roots they are unpacked into. This adds the one part of the
/// answer that is the project's rather than the package's: content sits under
/// `content/`, and the readme, license and thumbnail sit beside it at the
/// project root.
///
/// Narrowing the plan narrows the answer:
/// `modpkg.extraction_plan().layer("base").iter_project_paths()` is what
/// importing that one layer writes.
///
/// No path is an [`unpacked WAD`](ProjectPath::is_unpacked_wad): a package
/// stores its chunks individually and names every one of them, so the answer is
/// complete.
///
/// A WAD directory carries the name the package stores, which is lowercase: the
/// builder folds a WAD's name on the way in, so that is the name an extraction
/// writes too.
///
/// # Example
///
/// ```no_run
/// use ltk_mod_project::ProjectPaths;
/// use ltk_modpkg::Modpkg;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let modpkg = Modpkg::mount_from_reader(std::fs::File::open("my-mod.modpkg")?)?;
/// let output_dir = camino::Utf8Path::new("C:/mods/my-mod");
///
/// let longest = modpkg
///     .extraction_plan()
///     .iter_project_paths()
///     .map(|path| output_dir.join(path).as_str().len())
///     .max()
///     .unwrap_or(0);
/// println!("the longest path this import writes is {longest} characters");
/// # Ok(())
/// # }
/// ```
impl ProjectPaths for ExtractionPlan<'_> {
    fn iter_project_paths(&self) -> impl Iterator<Item = ProjectPath> + '_ {
        self.chunks().iter().map(project_path)
    }
}

/// Where one planned chunk lands, relative to the project directory.
///
/// Only the `content/` prefix is this crate's. Where a chunk sits beneath it is
/// the package format's business, so [`ChunkDestination::compose`] is asked
/// rather than `{layer}/{wad}/{path}` spelled again here. The root files are the
/// destinations that do not take the prefix - the package stores them under
/// `_meta_/`, but a project keeps them at its root - and the variant is what
/// says so.
fn project_path(planned: &PlannedChunk<'_>) -> ProjectPath {
    match planned.destination {
        ChunkDestination::Content { .. } => {
            ProjectPath::file(Utf8Path::new(CONTENT_DIR_NAME).join(planned.destination.compose()))
        }
        // The meta chunks are the project's root files, beside `content/`
        // rather than inside it.
        ChunkDestination::Root(file_name) => ProjectPath::file(file_name),
    }
}
