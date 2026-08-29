//! [`FantomeWriter`]: writes the entries of a Fantome archive.
//!
//! The writer owns the archive flavor and the entry naming conventions
//! (`WAD/`, `META/`). It does not know what a mod project is: deciding which
//! files go into the archive is the caller's job (see `ltk_mod_project`'s
//! `fantome` module).
//!
//! The flavor is a Deflate-compressed zip with one exception: a **packed WAD
//! is stored**. A reader seeks into a stored entry to reach one chunk and has
//! to inflate a deflated one whole, so deflating a packed WAD costs a map mod
//! a minute of CPU to produce an archive nothing can read cheaply. That is the
//! same rule [`store_packed_wads`](crate::store_packed_wads) normalizes an
//! archive to, and the writer holds to it so that a rewrite passing a WAD
//! through cannot quietly undo it.

use std::io::{Read, Seek, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::{FantomeEntry, FantomeHashtable, FantomeInfo, classify_entry};

/// Failure to write a Fantome archive entry.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FantomeWriteError {
    /// The archive could not be written.
    #[error("Failed to write the archive")]
    Zip(#[from] zip::result::ZipError),

    /// Copying entry content into the archive failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// `META/info.json` could not be serialized.
    #[error("Failed to serialize META/info.json")]
    Json(#[from] serde_json::Error),
}

/// Writes a Fantome archive entry by entry.
///
/// Call the `write_*` methods in any order, then [`finish`](Self::finish) to
/// flush the archive.
pub struct FantomeWriter<W: Write + Seek> {
    zip: ZipWriter<W>,
    options: SimpleFileOptions,
}

impl<W: Write + Seek> FantomeWriter<W> {
    /// Create a writer producing the standard Fantome flavor: a zip archive
    /// with Deflate compression, its packed WADs stored.
    ///
    /// A reader seeks into a stored entry to reach one chunk of a WAD and has
    /// to inflate a deflated one whole, so a packed WAD is written stored
    /// however it arrived - which is what [`store_packed_wads`] normalizes an
    /// archive to, held to here so a rewrite cannot undo it.
    ///
    /// [`store_packed_wads`]: crate::store_packed_wads
    pub fn new(writer: W) -> Self {
        Self {
            zip: ZipWriter::new(writer),
            options: SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(0o755),
        }
    }

    /// Write a file belonging to a WAD directory, as `WAD/{wad_name}/{rel_path}`.
    ///
    /// `rel_path` is relative to the WAD directory; backslashes are normalized
    /// to the `/` separator archives use, whatever the host uses.
    pub fn write_wad_entry(
        &mut self,
        wad_name: &str,
        rel_path: &str,
        content: &mut impl Read,
    ) -> Result<(), FantomeWriteError> {
        let entry_path = format!("WAD/{}/{}", wad_name, rel_path.replace('\\', "/"));
        self.write_entry(&entry_path, content)
    }

    /// Write the mod metadata as `META/info.json`.
    pub fn write_info(&mut self, info: &FantomeInfo) -> Result<(), FantomeWriteError> {
        self.zip.start_file("META/info.json", self.options)?;
        self.zip
            .write_all(&serde_json::to_string_pretty(info)?.into_bytes())?;
        Ok(())
    }

    /// Write the mod's readme as `META/README.md`.
    pub fn write_readme(&mut self, content: &mut impl Read) -> Result<(), FantomeWriteError> {
        self.write_entry("META/README.md", content)
    }

    /// Write the mod's license text as `META/{file_name}`.
    ///
    /// Readers recognize `LICENSE`, `LICENSE.md` and `LICENSE.txt` (in any
    /// casing); pass the canonical spelling so pack, extract, pack again does
    /// not rename the file underneath the author.
    pub fn write_license(
        &mut self,
        file_name: &str,
        content: &mut impl Read,
    ) -> Result<(), FantomeWriteError> {
        self.write_entry(&format!("META/{file_name}"), content)
    }

    /// The inner zip writer, for the rewrite's raw entry copies.
    pub(crate) fn zip_mut(&mut self) -> &mut ZipWriter<W> {
        &mut self.zip
    }

    /// Write one hashtable file at the path its manifest entry names.
    ///
    /// The manifest entry itself travels in `META/info.json`, via
    /// [`write_info`](Self::write_info); this writes only the table file. A
    /// table a manifest does not declare does not exist, so pass the same
    /// entry to both.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry cannot be written.
    pub fn write_hashtable(
        &mut self,
        manifest: &FantomeHashtable,
        table: &ltk_hashtable::Hashtable,
    ) -> Result<(), FantomeWriteError> {
        self.zip.start_file(&manifest.path, self.options)?;
        table.write_to(&mut self.zip)?;
        Ok(())
    }

    /// Write the mod's thumbnail as `META/image.png`.
    ///
    /// The bytes must already be PNG-encoded; the format stores no other
    /// image encoding.
    pub fn write_image_png(&mut self, png: &[u8]) -> Result<(), FantomeWriteError> {
        self.zip.start_file("META/image.png", self.options)?;
        self.zip.write_all(png)?;
        Ok(())
    }

    /// Finish the archive and return the underlying writer.
    pub fn finish(self) -> Result<W, FantomeWriteError> {
        Ok(self.zip.finish()?)
    }

    pub(crate) fn write_entry(
        &mut self,
        entry_path: &str,
        content: &mut impl Read,
    ) -> Result<(), FantomeWriteError> {
        self.zip
            .start_file(entry_path, self.options_for(entry_path))?;
        std::io::copy(content, &mut self.zip)?;
        Ok(())
    }

    /// How the entry at `entry_path` is compressed.
    ///
    /// Read off the path rather than taken from the caller: whether an entry
    /// is a packed WAD is the only thing the answer depends on, and a caller
    /// asked to state it each time is a caller that can get it wrong. See the
    /// [module docs](self) for what deflating a packed WAD costs.
    fn options_for(&self, entry_path: &str) -> SimpleFileOptions {
        match classify_entry(entry_path) {
            Some(FantomeEntry::PackedWad(_)) => {
                self.options.compression_method(CompressionMethod::Stored)
            }
            _ => self.options,
        }
    }
}
