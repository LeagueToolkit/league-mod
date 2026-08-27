//! [`FantomeImporter`]: decodes a Fantome archive into a mod project directory.

use std::fmt;
use std::io::{Read, Seek};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{
    classify_entry, FantomeEntry, FantomeExtractError, FantomeReader, NamingPolicy, NoResolver,
    PathResolver, WadExtractOptions, WadProgress,
};

use crate::{ImportFormat, ImportReporter, ImportTarget, ModProject, ModProjectLayer};

/// Failure to decode a Fantome archive into a project directory.
///
/// Driver failures (creating the output directory, writing the config) are not
/// here; they surface as the shared variants of
/// [`ImportError`](crate::ImportError).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FantomeImportError {
    /// The archive could not be read.
    #[error(transparent)]
    Extract(#[from] FantomeExtractError),

    /// A file could not be written to the output directory.
    #[error("Failed to write {path}")]
    Write {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// `META/image.png` could not be decoded, or re-encoded as the project
    /// thumbnail.
    #[error("Failed to convert the thumbnail")]
    Thumbnail(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// The import's cancellation answered `true`. The driver folds this into
    /// [`ImportError::Cancelled`](crate::ImportError::Cancelled).
    #[error("The import was cancelled")]
    Cancelled,
}

impl FantomeImportError {
    fn write(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Write {
            path: path.into(),
            source,
        }
    }
}

/// Decodes a Fantome archive into a mod project directory; the Fantome backend
/// for [`ProjectImporter`](crate::ProjectImporter).
///
/// Importing will:
/// 1. Extract WAD contents to `content/base/`, unpacking packed WADs through
///    the resolver [`with_path_resolver`](Self::with_path_resolver) supplied
///    and through the WAD's own bins for whatever it could not name
/// 2. Extract `RAW/` entries to `content/base/raw/`, and `README.md`, the
///    license text and the thumbnail (converted to `thumbnail.webp`), if
///    present
///
/// The project the archive's metadata describes is returned for the driver to
/// write out as `mod.config.json`.
///
/// # Example
///
/// ```no_run
/// use ltk_mod_project::ProjectImporter;
/// use ltk_mod_project::fantome::FantomeImporter;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let file = std::fs::File::open("my-mod.fantome")?;
/// let project = ProjectImporter::new("my-mod")
///     .with_config(|project| project.name = "my-mod".to_owned())
///     .import(FantomeImporter::new(file))?;
/// println!("imported {}", project.name);
/// # Ok(())
/// # }
/// ```
pub struct FantomeImporter<'a, R> {
    reader: R,
    resolver: Option<&'a dyn PathResolver>,
    naming: NamingPolicy,
}

impl<'a, R: Read + Seek> FantomeImporter<'a, R> {
    /// Create an importer reading the archive from `reader`.
    ///
    /// Use [`with_path_resolver`](Self::with_path_resolver) to supply paths for
    /// WAD chunks. Without one the import falls back to the names the archive's
    /// own bins hold, and a chunk nothing names keeps its hash.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            resolver: None,
            naming: NamingPolicy::Lossless,
        }
    }

    /// Unpack packed WADs through `resolver` so their files come out under
    /// their real paths instead of hex hashes.
    ///
    /// A caller implements [`PathResolver`] over whatever names it already
    /// holds, rather than copying them into a table this crate owns. It covers
    /// what the game ships. The archive's own bins cover what its author
    /// invented, and are read whether or not a resolver is supplied.
    #[must_use]
    pub fn with_path_resolver(mut self, resolver: &'a dyn PathResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Name the chunks of a packed WAD under `naming` rather than
    /// [`NamingPolicy::Lossless`].
    ///
    /// Lossless is the default because an import is the only copy of the
    /// content left: [`NamingPolicy::Descriptive`] drops a chunk whose resolved
    /// path another chunk claimed first, and a project that will be packed again
    /// has to keep every one. Ask for another policy only when the extracted
    /// tree is for reading rather than for repacking.
    #[must_use]
    pub fn with_naming_policy(mut self, naming: NamingPolicy) -> Self {
        self.naming = naming;
        self
    }
}

impl<R> fmt::Debug for FantomeImporter<'_, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FantomeImporter")
            .field("has_resolver", &self.resolver.is_some())
            .field("naming", &self.naming)
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek> ImportFormat for FantomeImporter<'_, R> {
    type Error = FantomeImportError;

    /// # Errors
    ///
    /// [`FantomeImportError`] covers a malformed archive, a file that could not
    /// be written into the output directory, a thumbnail that could not be
    /// converted, and a cancellation that answered `true`.
    fn import(
        self,
        target: &ImportTarget<'_>,
        progress: &mut ImportReporter<'_>,
    ) -> Result<ModProject, Self::Error> {
        let Self {
            reader,
            resolver,
            naming,
        } = self;

        let mut reader = FantomeReader::new(reader)?;
        let mod_project = ModProject::from(reader.read_info()?);

        let output_dir = target.output_dir();
        let cancellation = target.cancellation();
        let is_cancelled = || cancellation.is_cancelled();

        // Both passes count, so the total is reached when the extraction is
        // over rather than when the WADs are. A mod carrying most of its bytes
        // as `RAW/` entries spends most of the import in the second pass, and
        // leaving it out of the total showed a full bar for all of it.
        let raw_pass = reader
            .entry_names()
            .any(|name| matches!(classify_entry(name), Some(FantomeEntry::Raw(_))));
        progress.set_total(reader.wad_names().len() as u32 + u32::from(raw_pass));

        {
            let mut on_wad = |wad: WadProgress<'_>| progress.report_item(wad.name);

            let options = WadExtractOptions::new()
                .with_path_resolver(resolver.unwrap_or(&NoResolver))
                .with_naming_policy(naming)
                .with_progress(&mut on_wad)
                .with_cancellation(&is_cancelled);

            reader
                .extract_wads(&target.base_layer_dir(), options)
                .map_err(import_error)?;
        }

        if target.is_cancelled() {
            return Err(FantomeImportError::Cancelled);
        }

        if raw_pass {
            progress.report_item(RAW_PASS_NAME);
            reader
                .extract_raw(
                    &ModProjectLayer::raw_content_path(output_dir),
                    Some(&is_cancelled),
                )
                .map_err(import_error)?;
        }

        if target.is_cancelled() {
            return Err(FantomeImportError::Cancelled);
        }
        progress.report_writing_metadata();

        if let Some(readme) = reader.read_readme()? {
            write_file(&output_dir.join("README.md"), &readme)?;
        }

        if let Some((file_name, license)) = reader.read_license()? {
            write_file(&output_dir.join(file_name), &license)?;
        }

        if let Some(png) = reader.read_image_png()? {
            write_thumbnail(&png, &output_dir.join("thumbnail.webp"))?;
        }

        Ok(mod_project)
    }

    fn is_cancellation(error: &Self::Error) -> bool {
        matches!(error, FantomeImportError::Cancelled)
    }
}

/// What the `RAW/` pass is called when it is reported.
///
/// The archive's own name for it, as a WAD unit is reported under the name its
/// `WAD/` entry carries. Every `RAW/` entry is unpacked in one pass, so the pass
/// is one unit of the import rather than one per file: a file under `RAW/` is
/// copied out as-is, where a WAD has to be opened and its chunks named.
const RAW_PASS_NAME: &str = "RAW";

/// Fold the extractor's own cancellation into the import's, so a cancellation
/// has one error however deep in the import it landed.
fn import_error(source: FantomeExtractError) -> FantomeImportError {
    match source {
        FantomeExtractError::Cancelled => FantomeImportError::Cancelled,
        other => FantomeImportError::Extract(other),
    }
}

fn write_file(path: &Utf8Path, bytes: &[u8]) -> Result<(), FantomeImportError> {
    std::fs::write(path, bytes).map_err(|source| FantomeImportError::write(path, source))
}

/// Convert the archive's PNG thumbnail to the WebP a project stores.
fn write_thumbnail(png: &[u8], output_path: &Utf8Path) -> Result<(), FantomeImportError> {
    let thumbnail_error =
        |source: image::ImageError| FantomeImportError::Thumbnail(Box::new(source));

    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(thumbnail_error)?;

    img.save(output_path).map_err(thumbnail_error)?;

    Ok(())
}
