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
    pub fn extract_wads(
        &mut self,
        dest: &Utf8Path,
        hashtable: Option<&WadHashtable>,
    ) -> Result<(), FantomeExtractError> {
        for i in 0..self.archive.len() {
            let mut file = self.archive.by_index(i)?;
            let file_name = file.name().to_string();

            let Some(relative_path) = file_name.strip_prefix("WAD/") else {
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
    pub fn extract_raw(&mut self, dest: &Utf8Path) -> Result<(), FantomeExtractError> {
        for i in 0..self.archive.len() {
            let mut file = self.archive.by_index(i)?;
            let file_name = file.name().to_string();

            let Some(relative_path) = file_name.strip_prefix("RAW/") else {
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
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use std::io::Write;
    use tempfile::{TempDir, tempdir};
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    /// The temp directory's path, which extraction takes as UTF-8.
    fn utf8_dir(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap()
    }

    fn create_test_fantome() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        zip.start_file("META/info.json", options).unwrap();
        let info = r#"{
            "Name": "Test Mod",
            "Author": "Test Author",
            "Version": "1.0.0",
            "Description": "A test mod"
        }"#;
        zip.write_all(info.as_bytes()).unwrap();

        zip.add_directory("WAD/test.wad.client", options).unwrap();
        zip.start_file("WAD/test.wad.client/assets/test.bin", options)
            .unwrap();
        zip.write_all(b"test content").unwrap();

        zip.start_file("RAW/assets/maps/map11/scene.bin", options)
            .unwrap();
        zip.write_all(b"map data").unwrap();

        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn read_info_parses_metadata() {
        let mut reader = FantomeReader::new(Cursor::new(create_test_fantome())).unwrap();
        let info = reader.read_info().unwrap();

        assert_eq!(info.name, "Test Mod");
        assert_eq!(info.version, "1.0.0");
    }

    #[test]
    fn extract_wads_preserves_paths_under_dest() {
        let mut reader = FantomeReader::new(Cursor::new(create_test_fantome())).unwrap();

        let tmp = tempdir().unwrap();
        let dest = utf8_dir(&tmp).join("wads");
        reader.extract_wads(&dest, None).unwrap();

        assert_eq!(
            std::fs::read(dest.join("test.wad.client/assets/test.bin")).unwrap(),
            b"test content"
        );
    }

    #[test]
    fn extract_raw_preserves_paths_under_dest() {
        let mut reader = FantomeReader::new(Cursor::new(create_test_fantome())).unwrap();

        let tmp = tempdir().unwrap();
        let dest = utf8_dir(&tmp).join("RAW");
        reader.extract_raw(&dest).unwrap();

        assert_eq!(
            std::fs::read(dest.join("assets/maps/map11/scene.bin")).unwrap(),
            b"map data"
        );
    }

    #[test]
    fn read_license_matches_case_and_extension_variants() {
        for (entry, expected_name) in [
            ("META/LICENSE", "LICENSE"),
            ("META/license.md", "LICENSE.md"),
            ("meta/LICENSE.TXT", "LICENSE.txt"),
        ] {
            let cursor = Cursor::new(Vec::new());
            let mut zip = ZipWriter::new(cursor);
            let options = SimpleFileOptions::default();
            zip.start_file(entry, options).unwrap();
            zip.write_all(b"The terms.").unwrap();
            let data = zip.finish().unwrap().into_inner();

            let mut reader = FantomeReader::new(Cursor::new(data)).unwrap();
            let (name, bytes) = reader
                .read_license()
                .unwrap()
                .unwrap_or_else(|| panic!("no license found for entry {entry}"));

            assert_eq!(name, expected_name);
            assert_eq!(bytes, b"The terms.");
        }
    }

    #[test]
    fn meta_entries_absent_read_as_none() {
        let mut reader = FantomeReader::new(Cursor::new(create_test_fantome())).unwrap();

        assert!(reader.read_readme().unwrap().is_none());
        assert!(reader.read_license().unwrap().is_none());
        assert!(reader.read_image_png().unwrap().is_none());
    }

    #[test]
    fn missing_info_is_a_distinct_error() {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        zip.start_file("WAD/x.bin", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        let data = zip.finish().unwrap().into_inner();

        let mut reader = FantomeReader::new(Cursor::new(data)).unwrap();
        assert!(matches!(
            reader.read_info(),
            Err(FantomeExtractError::MissingMetadata)
        ));
    }

    /// What the writer produces, the reader must give back.
    #[test]
    fn writer_reader_round_trip() {
        use crate::writer::FantomeWriter;

        let info = FantomeInfo {
            name: "Round Trip".to_string(),
            author: "Alice".to_string(),
            version: "1.0.0".to_string(),
            description: "".to_string(),
            license: None,
            tags: vec![],
            champions: vec![],
            maps: vec![],
            layers: Default::default(),
        };

        let mut writer = FantomeWriter::new(Cursor::new(Vec::new()));
        writer.write_info(&info).unwrap();
        writer
            .write_wad_entry("Test.wad.client", "data\\skin.bin", &mut &b"skin"[..])
            .unwrap();
        writer
            .write_license("LICENSE.md", &mut &b"terms"[..])
            .unwrap();
        writer.write_readme(&mut &b"readme"[..]).unwrap();
        writer.write_image_png(b"png bytes").unwrap();
        let mut buffer = writer.finish().unwrap();

        buffer.set_position(0);
        let mut reader = FantomeReader::new(buffer).unwrap();

        assert_eq!(reader.read_info().unwrap().name, "Round Trip");
        assert_eq!(
            reader.read_license().unwrap().unwrap(),
            ("LICENSE.md", b"terms".to_vec())
        );
        assert_eq!(reader.read_readme().unwrap().unwrap(), b"readme");
        assert_eq!(reader.read_image_png().unwrap().unwrap(), b"png bytes");

        // Backslashes in the relative path were normalized to `/`.
        let tmp = tempdir().unwrap();
        let dest = utf8_dir(&tmp);
        reader.extract_wads(&dest, None).unwrap();
        assert_eq!(
            std::fs::read(dest.join("Test.wad.client/data/skin.bin")).unwrap(),
            b"skin"
        );
    }
}
