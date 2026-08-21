//! [`FantomeImporter`]: materializes a Fantome archive as a mod project.

use std::fmt;
use std::io::{Read, Seek};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{FantomeExtractError, FantomeReader, WadHashtable};

use crate::{ImportFormat, ModProject, ModProjectError};

/// Failure to import a Fantome archive as a mod project.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FantomeImportError {
    /// The archive could not be read.
    #[error(transparent)]
    Extract(#[from] FantomeExtractError),

    /// The imported project's `mod.config.json` could not be written.
    #[error(transparent)]
    Config(#[from] ModProjectError),

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
}

impl FantomeImportError {
    fn write(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Write {
            path: path.into(),
            source,
        }
    }
}

/// Imports a Fantome archive as a mod project; the Fantome implementation of
/// [`ImportFormat`].
///
/// Importing will:
/// 1. Extract WAD contents to `content/base/` (unpacking packed WADs through
///    the hashtable, if [`with_hashtable`](Self::with_hashtable) provided
///    one; without one, their files are named by hex hash)
/// 2. Extract `RAW/` entries, `README.md`, the license text, and the
///    thumbnail (converted to `thumbnail.webp`), if present
/// 3. Create a `mod.config.json` from the archive's metadata
///
/// # Example
///
/// ```no_run
/// use camino::Utf8Path;
/// use ltk_mod_project::fantome::FantomeImporter;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let file = std::fs::File::open("my-mod.fantome")?;
/// let project = FantomeImporter::new(file).import(Utf8Path::new("my-mod"))?;
/// println!("imported {}", project.name);
/// # Ok(())
/// # }
/// ```
pub struct FantomeImporter<'a, R> {
    reader: R,
    hashtable: Option<&'a WadHashtable>,
}

impl<'a, R: Read + Seek> FantomeImporter<'a, R> {
    /// Create an importer reading the archive from `reader`.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            hashtable: None,
        }
    }

    /// Unpack packed WADs through `hashtable` so their files come out under
    /// their real paths instead of hex hashes.
    #[must_use]
    pub fn with_hashtable(mut self, hashtable: &'a WadHashtable) -> Self {
        self.hashtable = Some(hashtable);
        self
    }

    /// Materialize the archive as a mod project laid out in `output_dir`,
    /// creating the directory if needed.
    ///
    /// On success the project's config has been written into `output_dir`
    /// and is returned. This is also the [`ImportFormat`] implementation;
    /// the trait does not need to be in scope to call it.
    ///
    /// # Errors
    ///
    /// [`FantomeImportError`] covers a malformed archive, a file that could
    /// not be written into `output_dir`, and a thumbnail that could not be
    /// converted.
    pub fn import(self, output_dir: &Utf8Path) -> Result<ModProject, FantomeImportError> {
        let mut reader = FantomeReader::new(self.reader)?;

        let info = reader.read_info()?;
        let mod_project = ModProject::from(info);

        if !output_dir.exists() {
            std::fs::create_dir_all(output_dir)
                .map_err(|source| FantomeImportError::write(output_dir, source))?;
        }

        reader.extract_wads(&output_dir.join("content").join("base"), self.hashtable)?;
        reader.extract_raw(&output_dir.join("RAW"))?;

        if let Some(readme) = reader.read_readme()? {
            let path = output_dir.join("README.md");
            std::fs::write(&path, readme)
                .map_err(|source| FantomeImportError::write(path, source))?;
        }

        if let Some((file_name, license)) = reader.read_license()? {
            let path = output_dir.join(file_name);
            std::fs::write(&path, license)
                .map_err(|source| FantomeImportError::write(path, source))?;
        }

        if let Some(png) = reader.read_image_png()? {
            write_thumbnail(&png, &output_dir.join("thumbnail.webp"))?;
        }

        mod_project.save(&output_dir.join("mod.config.json"))?;

        Ok(mod_project)
    }
}

impl<R> fmt::Debug for FantomeImporter<'_, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FantomeImporter")
            .field("has_hashtable", &self.hashtable.is_some())
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek> ImportFormat for FantomeImporter<'_, R> {
    type Error = FantomeImportError;

    fn import(self, output_dir: &Utf8Path) -> Result<ModProject, Self::Error> {
        self.import(output_dir)
    }
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
