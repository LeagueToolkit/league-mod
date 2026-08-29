//! [`store_packed_wads`]: hold an archive's packed WADs stored rather than
//! deflated, so a reader can seek to their bytes in place.
//!
//! A deflated packed WAD has to be inflated whole before any chunk inside it
//! can be reached, which puts the entire WAD in memory for the sake of the few
//! chunks a build wants. Stored, the same entry is a byte range the reader
//! seeks into. Everything else the archive holds is deflated on purpose - the
//! loose files, the metadata, the tables - and is raw-copied untouched, wrong
//! CRC32 values included, on the same terms the hashtable rewrite copies them.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, Write};

use camino::{Utf8Path, Utf8PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::reader::copy_entry;
use crate::{FantomeEntry, FantomeExtractError, FantomeReader, FantomeWriteError, classify_entry};

/// What a normalize did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizeOutcome {
    /// Every packed WAD was already stored; nothing was written to the sink.
    Unchanged,
    /// The sink holds the normalized archive.
    Normalized {
        /// How many packed WADs were rewritten as stored entries.
        wads_stored: usize,
    },
}

/// Failure to normalize an archive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FantomeNormalizeError {
    /// A file could not be read or written.
    #[error("Failed to access {path}")]
    Io {
        /// The file that failed.
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The source archive could not be read.
    #[error(transparent)]
    Read(#[from] FantomeExtractError),

    /// The normalized archive could not be written.
    #[error(transparent)]
    Write(#[from] FantomeWriteError),
}

impl FantomeNormalizeError {
    fn io(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Normalize the `.fantome` archive at `source`, writing the result to `dest`.
///
/// [`store_packed_wads`] is the operation; this is the shape an importer wants
/// it in. The rewrite lands as a temporary file beside `dest` and is renamed
/// over it only once writing finishes cleanly, so an interrupted normalize
/// never leaves a half-written archive where a mod should be, and `source` is
/// only ever read - a normalize over a file the user handed in is exactly what
/// `adr/0002-normalization-happens-at-import-never-at-build.md` forbids, so
/// pass a copy the importer owns as `source`, or the same path as both.
///
/// `dest` ends up holding the archive either way: an archive that needed
/// nothing is copied there rather than left missing, so an import does not have
/// to know which outcome it got before it can find the mod. When `dest` *is*
/// `source` and nothing needed doing, nothing is written at all.
///
/// # Errors
///
/// Returns an error if the source cannot be read or the destination cannot be
/// written. `dest` is left as it was on any error.
pub fn normalize_archive(
    source: &Utf8Path,
    dest: &Utf8Path,
) -> Result<NormalizeOutcome, FantomeNormalizeError> {
    let file =
        File::open(source.as_std_path()).map_err(|e| FantomeNormalizeError::io(source, e))?;
    let mut reader = FantomeReader::new(BufReader::new(file))?;

    let parent = match dest.parent() {
        Some(parent) if !parent.as_str().is_empty() => parent,
        _ => Utf8Path::new("."),
    };
    fs::create_dir_all(parent.as_std_path()).map_err(|e| FantomeNormalizeError::io(parent, e))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent.as_std_path())
        .map_err(|e| FantomeNormalizeError::io(parent, e))?;

    let outcome = store_packed_wads(&mut reader, temp.as_file_mut())?;
    drop(reader);

    match outcome {
        NormalizeOutcome::Unchanged if source != dest => {
            // The plain copy still lands through the temp file and the rename,
            // so an interrupted normalize can no more truncate an already
            // normalized mod than a rewritten one.
            let mut original = File::open(source.as_std_path())
                .map_err(|e| FantomeNormalizeError::io(source, e))?;
            std::io::copy(&mut original, temp.as_file_mut())
                .map_err(|e| FantomeNormalizeError::io(dest, e))?;
            temp.persist(dest.as_std_path())
                .map_err(|e| FantomeNormalizeError::io(dest, e.error))?;
        }
        NormalizeOutcome::Unchanged => drop(temp),
        NormalizeOutcome::Normalized { .. } => {
            temp.persist(dest.as_std_path())
                .map_err(|e| FantomeNormalizeError::io(dest, e.error))?;
        }
    }

    Ok(outcome)
}

/// Write the archive `reader` holds to `sink`, its packed WADs stored.
///
/// A packed WAD already stored is left as it is, and every entry that is not a
/// packed WAD is raw-copied byte-for-byte - still deflated, and carrying
/// whatever CRC32 its author wrote, which is what [`FantomeReader`] already
/// expects. A WAD this does re-encode gains a CRC32 computed over the bytes it
/// writes, so the entries a reader now seeks into are the ones whose checksums
/// are true.
///
/// When every packed WAD is already stored the sink is left untouched and
/// [`NormalizeOutcome::Unchanged`] comes back - deciding costs the entry table
/// alone, so a normalized archive is never rewritten and a rerun is a no-op.
/// The caller owns where the sink lives; a normalize over a file the user did
/// not ask to lose belongs behind a temp-file-and-rename.
///
/// # Errors
///
/// Returns an error if the source archive cannot be read or the normalized
/// archive cannot be written. Nothing is written on a read failure.
pub fn store_packed_wads<R: Read + Seek, W: Write + Seek>(
    reader: &mut FantomeReader<R>,
    sink: W,
) -> Result<NormalizeOutcome, FantomeNormalizeError> {
    let deflated = deflated_packed_wads(reader)?;
    if deflated.is_empty() {
        return Ok(NormalizeOutcome::Unchanged);
    }

    // The re-encoded entries take the flavor this crate writes, minus the
    // compression: an entry a reader is meant to seek into cannot be deflated.
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);
    let mut writer = ZipWriter::new(sink);

    for index in 0..reader.entry_count() {
        if deflated.contains(&index) {
            let mut entry = reader
                .zip_archive_mut()
                .by_index(index)
                .map_err(FantomeExtractError::from)?;
            let name = entry.name().to_owned();
            writer
                .start_file(name, stored)
                .map_err(FantomeWriteError::from)?;
            copy_entry(&mut entry, &mut writer).map_err(FantomeExtractError::from)?;
        } else {
            let entry = reader
                .zip_archive_mut()
                .by_index_raw(index)
                .map_err(FantomeExtractError::from)?;
            writer
                .raw_copy_file(entry)
                .map_err(FantomeWriteError::from)?;
        }
    }
    writer.finish().map_err(FantomeWriteError::from)?;

    Ok(NormalizeOutcome::Normalized {
        wads_stored: deflated.len(),
    })
}

/// The indexes of the packed WAD entries held in anything but stored form.
///
/// Only the entry table is read, so an archive that needs nothing costs no
/// decompression to recognise.
fn deflated_packed_wads<R: Read + Seek>(
    reader: &mut FantomeReader<R>,
) -> Result<HashSet<usize>, FantomeExtractError> {
    let mut deflated = HashSet::new();
    for index in 0..reader.entry_count() {
        let entry = reader.zip_archive_mut().by_index_raw(index)?;
        if entry.compression() != CompressionMethod::Stored
            && matches!(
                classify_entry(entry.name()),
                Some(FantomeEntry::PackedWad(_))
            )
        {
            deflated.insert(index);
        }
    }
    Ok(deflated)
}

#[cfg(test)]
mod tests;
