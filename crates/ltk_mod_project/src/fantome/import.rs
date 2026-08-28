//! [`FantomeImporter`]: decodes a Fantome archive into a mod project directory.

use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Seek};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{
    classify_entry, FantomeEntry, FantomeExtractError, FantomeReader, NamingPolicy, NoResolver,
    PathResolver, WadExtractOptions, WadProgress,
};
use ltk_hashtable::{GameResolver, Hashtable, HashtableEntry, HashtableSet};
use ltk_wad::WadHash;

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

    /// Two declared hashtable files land on one project file name.
    ///
    /// Declared tables land flat under `hashes/` by file name (a
    /// `META/hashes/` tail is carried whole), so declarations from
    /// different places can collide - and writing both would clobber one
    /// with the other. An archive shaped like this is ambiguous, and an
    /// import must not invent names to resolve it.
    #[error(transparent)]
    DuplicateHashtableName(#[from] crate::DuplicateHashtableName),

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
/// 3. Recover the hashtables the archive declares into `hashes/`, with the
///    project manifest rewritten to the new paths - and name packed WAD
///    chunks from those tables first, ahead of the supplied resolver, since
///    the mod's own table is the authority on the names its author invented
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
        let info = reader.read_info()?;
        // Where each declared table lands. Computed once, before anything is
        // written: what the routes declare is what the config carries and
        // where the files land, so the files and the manifest cannot
        // disagree - and an archive whose tables collide on one file name is
        // refused before the import touches the output directory.
        let routes = super::convert::project_routes(&info.hashtables)?;
        let table_routes: HashMap<&str, &str> = routes
            .iter()
            .map(|route| (route.source.as_str(), route.manifest.path.as_str()))
            .collect();
        let mut mod_project = ModProject::from(info);
        mod_project.hashtables = routes.iter().map(|route| route.manifest.clone()).collect();

        // Read before the WADs are unpacked: the mod's own tables name its
        // chunks, ahead of whatever resolver the caller supplied.
        let declared_tables = reader.read_hashtables()?;
        let own_names = HashtableSet::build(declared_tables.iter().cloned());

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

            let chained = ChainedResolver {
                own: GameResolver::new(&own_names),
                fallback: resolver.unwrap_or(&NoResolver),
            };
            let options = WadExtractOptions::new()
                .with_path_resolver(&chained)
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

        write_hashtables(output_dir, &table_routes, &declared_tables)?;

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

/// Names chunks from the mod's own declared tables first, the caller's
/// resolver second.
struct ChainedResolver<'a, 'b> {
    own: GameResolver<'a>,
    fallback: &'b dyn PathResolver,
}

impl PathResolver for ChainedResolver<'_, '_> {
    fn resolve(&self, path_hash: WadHash) -> Option<String> {
        self.own
            .resolve(path_hash)
            .or_else(|| self.fallback.resolve(path_hash))
    }

    fn is_known(&self, path_hash: WadHash) -> bool {
        self.own.is_known(path_hash) || self.fallback.is_known(path_hash)
    }
}

/// Write the archive's declared tables into `hashes/`, each at the project
/// path its route names.
///
/// The routes and the config manifest come from one mapping over one input,
/// and the pairing here is by the declared archive path rather than by list
/// position, so neither side's filtering can mispair them.
fn write_hashtables(
    output_dir: &Utf8Path,
    routes: &HashMap<&str, &str>,
    tables: &[(HashtableEntry, Hashtable)],
) -> Result<(), FantomeImportError> {
    for (entry, table) in tables {
        let project_path = routes
            .get(entry.path().as_str())
            .expect("every table read out of the manifest has a route from the same manifest");
        let path = output_dir.join(project_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent.as_std_path())
                .map_err(|source| FantomeImportError::write(parent, source))?;
        }
        let mut file = std::io::BufWriter::new(
            std::fs::File::create(path.as_std_path())
                .map_err(|source| FantomeImportError::write(&path, source))?,
        );
        table
            .write_to(&mut file)
            .and_then(|()| std::io::Write::flush(&mut file))
            .map_err(|source| FantomeImportError::write(&path, source))?;
    }
    Ok(())
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
