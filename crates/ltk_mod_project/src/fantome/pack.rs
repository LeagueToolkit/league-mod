//! [`FantomeFormat`]: encodes a pack plan as a Fantome archive.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Seek, Write};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{FantomeInfo, FantomeWriteError, FantomeWriter};

use crate::{PackFormat, PackPlan};

/// Failure to encode a pack plan as a Fantome archive.
///
/// Driver failures (scanning, `.modignore`, layout validation) are not here;
/// they surface as the shared variants of
/// [`PackError`](crate::PackError).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FantomePackError {
    /// A file in the project could not be read.
    #[error("Failed to read {path}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    /// The archive could not be written.
    #[error(transparent)]
    Write(#[from] FantomeWriteError),

    /// The thumbnail could not be read, or re-encoded as the PNG Fantome stores.
    #[error("Failed to convert the thumbnail {path}")]
    Thumbnail {
        path: Utf8PathBuf,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

impl FantomePackError {
    fn read(path: impl Into<Utf8PathBuf>, source: io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }
}

/// Packs a mod project into a Fantome archive; the Fantome backend for
/// [`ProjectPacker`](crate::ProjectPacker).
///
/// Fantome stores less than a plan can carry: only the base layer is packed
/// (use [`ModProject::non_base_layers`](crate::ModProject::non_base_layers)
/// to warn about layers a pack will drop), and within it only files inside
/// `.wad.client` directories. See the [`pack` module docs](crate::pack) for
/// how formats plug into the driver.
///
/// # Example
///
/// ```no_run
/// use ltk_mod_project::fantome::FantomeFormat;
/// use ltk_mod_project::ProjectPacker;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let packer = ProjectPacker::from_dir("path/to/my-mod")?;
/// let file = std::fs::File::create("build/my-mod_1.0.0.fantome")?;
/// packer.pack(FantomeFormat::new(file))?;
/// # Ok(())
/// # }
/// ```
pub struct FantomeFormat<W> {
    writer: W,
}

impl<W: Write + Seek> FantomeFormat<W> {
    /// Create a format writing the archive to `writer`.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W> fmt::Debug for FantomeFormat<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FantomeFormat").finish_non_exhaustive()
    }
}

impl<W: Write + Seek> PackFormat for FantomeFormat<W> {
    type Error = FantomePackError;

    fn pack(self, plan: &PackPlan<'_>) -> Result<(), Self::Error> {
        let mut writer = FantomeWriter::new(self.writer);

        pack_base_layer(&mut writer, plan)?;
        pack_metadata(&mut writer, plan)?;

        writer.finish()?;
        Ok(())
    }
}

fn pack_base_layer<W: Write + Seek>(
    writer: &mut FantomeWriter<W>,
    plan: &PackPlan<'_>,
) -> Result<(), FantomePackError> {
    for file in plan.base_layer().files() {
        // Fantome has no place for content outside a WAD directory.
        let Some(wad_name) = file.wad() else {
            continue;
        };

        let mut content = File::open(file.source())
            .map_err(|source| FantomePackError::read(file.source(), source))?;

        writer
            .write_wad_entry(wad_name, file.rel_path(), &mut content)
            .map_err(|error| attribute_write_error(error, file.source()))?;
    }

    Ok(())
}

fn pack_metadata<W: Write + Seek>(
    writer: &mut FantomeWriter<W>,
    plan: &PackPlan<'_>,
) -> Result<(), FantomePackError> {
    writer.write_info(&FantomeInfo::from(plan.project()))?;

    if let Some(readme) = plan.readme() {
        let mut file =
            File::open(readme).map_err(|source| FantomePackError::read(readme, source))?;
        writer
            .write_readme(&mut file)
            .map_err(|error| attribute_write_error(error, readme))?;
    }

    // The entry keeps the canonical spelling of the source file's name
    // (`license.txt` becomes `META/LICENSE.txt`), which is what the importer
    // writes back out, so pack -> extract -> pack does not rename the file
    // underneath the author.
    if let Some(license) = plan.license() {
        let mut file = File::open(license.source())
            .map_err(|source| FantomePackError::read(license.source(), source))?;
        writer
            .write_license(license.canonical_name(), &mut file)
            .map_err(|error| attribute_write_error(error, license.source()))?;
    }

    if let Some(thumbnail) = plan.thumbnail() {
        let png = encode_thumbnail_png(thumbnail)?;
        writer.write_image_png(&png)?;
    }

    Ok(())
}

/// Re-encode the project's thumbnail as the PNG the format stores.
fn encode_thumbnail_png(image_path: &Utf8Path) -> Result<Vec<u8>, FantomePackError> {
    let thumbnail_error = |source: image::ImageError| FantomePackError::Thumbnail {
        path: image_path.to_owned(),
        source: Box::new(source),
    };

    let img = image::open(image_path).map_err(thumbnail_error)?;

    let mut png_buffer = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_buffer),
        image::ImageFormat::Png,
    )
    .map_err(thumbnail_error)?;

    Ok(png_buffer)
}

/// A copy failure while writing `path`'s entry is a read of that file as far
/// as the author is concerned; archive-level failures stay archive failures.
fn attribute_write_error(error: FantomeWriteError, path: &Utf8Path) -> FantomePackError {
    match error {
        FantomeWriteError::Io(source) => FantomePackError::read(path, source),
        other => FantomePackError::Write(other),
    }
}
