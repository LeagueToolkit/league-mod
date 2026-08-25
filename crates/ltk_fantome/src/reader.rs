//! [`FantomeReader`]: reads the entries of a Fantome archive.
//!
//! The reader knows the archive's entry conventions (`WAD/`, `RAW/`,
//! `META/`) and how to unpack a packed WAD it finds, but not what a mod
//! project looks like on disk: turning an archive into a project directory
//! is the caller's job (see `ltk_mod_project`'s `fantome` module).

use std::io::{Cursor, Read, Seek};

use camino::Utf8Path;
use ltk_wad::{HexPathResolver, Wad, WadExtractor};
use zip::ZipArchive;

use crate::FantomeInfo;
use crate::error::FantomeExtractError;
use crate::hashtable::WadHashtable;

/// Reads a Fantome archive entry by entry.
pub struct FantomeReader<R: Read + Seek> {
    archive: ZipArchive<R>,
}

impl<R: Read + Seek> FantomeReader<R> {
    /// Create a reader from anything the archive can be read out of.
    pub fn new(reader: R) -> Result<Self, FantomeExtractError> {
        let archive = ZipArchive::new(reader)?;
        Ok(Self { archive })
    }

    /// Read the mod metadata from `META/info.json`.
    pub fn read_info(&mut self) -> Result<FantomeInfo, FantomeExtractError> {
        // Entry names are matched case-insensitively: archives in the wild
        // spell the directory `META`, `meta` and `Meta`.
        let index = (0..self.archive.len()).find(|i| {
            self.archive
                .by_index(*i)
                .is_ok_and(|file| file.name().eq_ignore_ascii_case("META/info.json"))
        });

        let Some(index) = index else {
            return Err(FantomeExtractError::MissingMetadata);
        };

        let mut info_content = String::new();
        self.archive
            .by_index(index)?
            .read_to_string(&mut info_content)?;

        // Strip UTF-8 BOM if present
        let info_content = info_content.trim_start_matches('\u{feff}').trim();

        if info_content.is_empty() {
            return Err(FantomeExtractError::MissingMetadata);
        }

        let info: FantomeInfo = serde_json::from_str(info_content)?;
        Ok(info)
    }

    /// Extract every `WAD/` entry into `dest`, preserving the paths beneath
    /// the prefix.
    ///
    /// A packed WAD directly under `WAD/` is unpacked into a directory of its
    /// name rather than written out as a file, resolving chunk paths through
    /// `hashtable`; without one, extracted files are named by their hex hash.
    ///
    /// The prefix is matched case-insensitively.
    pub fn extract_wads(
        &mut self,
        dest: &Utf8Path,
        hashtable: Option<&WadHashtable>,
    ) -> Result<(), FantomeExtractError> {
        for i in 0..self.archive.len() {
            let mut file = self.archive.by_index(i)?;
            let file_name = file.name().to_string();

            let Some(relative_path) = strip_prefix_ci(&file_name, "WAD/") else {
                continue;
            };
            if relative_path.is_empty() {
                continue;
            }

            let output_path = dest.join(relative_path);

            if file.is_dir() {
                create_dir(&output_path)?;
            } else if !relative_path.contains('/') && is_wad_file_name(relative_path) {
                extract_packed_wad(&mut file, &output_path, hashtable)?;
            } else {
                extract_entry(&mut file, &output_path)?;
            }
        }

        Ok(())
    }

    /// Extract every `RAW/` entry into `dest`, preserving the paths beneath
    /// the prefix.
    ///
    /// The prefix is matched case-insensitively.
    pub fn extract_raw(&mut self, dest: &Utf8Path) -> Result<(), FantomeExtractError> {
        for i in 0..self.archive.len() {
            let mut file = self.archive.by_index(i)?;
            let file_name = file.name().to_string();

            let Some(relative_path) = strip_prefix_ci(&file_name, "RAW/") else {
                continue;
            };
            if relative_path.is_empty() {
                continue;
            }

            let output_path = dest.join(relative_path);

            if file.is_dir() {
                create_dir(&output_path)?;
            } else {
                extract_entry(&mut file, &output_path)?;
            }
        }

        Ok(())
    }

    /// Read the `META/README.md` entry, if the archive has one.
    pub fn read_readme(&mut self) -> Result<Option<Vec<u8>>, FantomeExtractError> {
        self.read_meta_entry(|name| (name == "META/README.md").then_some(()))
            .map(|found| found.map(|((), bytes)| bytes))
    }

    /// Read the `META/LICENSE*` entry, if the archive has one.
    ///
    /// The entry is matched case-insensitively across the `LICENSE`,
    /// `LICENSE.md` and `LICENSE.txt` spellings; the returned name is the
    /// canonical file name the license should be written to.
    pub fn read_license(&mut self) -> Result<Option<(&'static str, Vec<u8>)>, FantomeExtractError> {
        self.read_meta_entry(license_entry_target)
    }

    /// Read the `META/image.png` thumbnail entry, if the archive has one.
    ///
    /// The bytes are returned as stored; the format keeps thumbnails
    /// PNG-encoded.
    pub fn read_image_png(&mut self) -> Result<Option<Vec<u8>>, FantomeExtractError> {
        self.read_meta_entry(|name| (name == "META/image.png").then_some(()))
            .map(|found| found.map(|((), bytes)| bytes))
    }

    /// Find the first entry whose name `matcher` accepts and read it whole.
    fn read_meta_entry<T>(
        &mut self,
        matcher: impl Fn(&str) -> Option<T>,
    ) -> Result<Option<(T, Vec<u8>)>, FantomeExtractError> {
        for i in 0..self.archive.len() {
            let mut file = self.archive.by_index(i)?;
            let Some(matched) = matcher(file.name()) else {
                continue;
            };

            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            return Ok(Some((matched, bytes)));
        }

        Ok(None)
    }
}

/// Write one archive entry to `output_path`, creating its parent directories.
fn extract_entry<R: Read>(
    entry: &mut R,
    output_path: &Utf8Path,
) -> Result<(), FantomeExtractError> {
    let write_error = |source| FantomeExtractError::write(output_path, source);

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(write_error)?;
    }

    let mut outfile = std::fs::File::create(output_path).map_err(write_error)?;
    std::io::copy(entry, &mut outfile).map_err(write_error)?;

    Ok(())
}

/// Create a directory an archive entry names, and its parents.
fn create_dir(path: &Utf8Path) -> Result<(), FantomeExtractError> {
    std::fs::create_dir_all(path).map_err(|source| FantomeExtractError::write(path, source))
}

/// Match a `META/LICENSE*` archive entry case-insensitively and return the file
/// name it should be written to in the extracted project.
///
/// The canonical entry is extensionless `META/LICENSE`, but readers accept the
/// `.md` and `.txt` variants and preserve their extension on the way out.
fn license_entry_target(file_name: &str) -> Option<&'static str> {
    match file_name.to_ascii_lowercase().as_str() {
        "meta/license" => Some("LICENSE"),
        "meta/license.md" => Some("LICENSE.md"),
        "meta/license.txt" => Some("LICENSE.txt"),
        _ => None,
    }
}

/// Strip a leading ASCII prefix case-insensitively, returning the remainder.
///
/// Archives in the wild spell the top-level directories `WAD`, `wad`, `RAW`
/// and `raw`, so an entry is placed by a case-insensitive prefix.
fn strip_prefix_ci<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let head = name.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &name[prefix.len()..])
}

/// Check if a filename looks like a WAD file (ends with .wad.client or similar WAD extensions)
fn is_wad_file_name(name: &str) -> bool {
    name.ends_with(".wad.client") || name.ends_with(".wad") || name.ends_with(".wad.mobile")
}

/// Extract a packed WAD file to a directory using WadExtractor
fn extract_packed_wad<R: Read>(
    wad_reader: &mut R,
    output_dir: &Utf8Path,
    hashtable: Option<&WadHashtable>,
) -> Result<(), FantomeExtractError> {
    let mut wad_data = Vec::new();
    wad_reader.read_to_end(&mut wad_data)?;

    let cursor = Cursor::new(wad_data);
    let mut wad = Wad::mount(cursor)?;

    std::fs::create_dir_all(output_dir)
        .map_err(|source| FantomeExtractError::write(output_dir, source))?;

    match hashtable {
        Some(hashtable) => WadExtractor::new(hashtable).extract_all(&mut wad, output_dir)?,
        None => WadExtractor::new(&HexPathResolver).extract_all(&mut wad, output_dir)?,
    };

    Ok(())
}

#[cfg(test)]
mod tests;
