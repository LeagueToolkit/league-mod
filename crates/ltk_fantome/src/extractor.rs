use std::fs::File;
use std::io::{Cursor, Read, Seek};

use camino::Utf8Path;
use image::ImageFormat;
use ltk_mod_project::{
    ModMap, ModProject, ModProjectAuthor, ModProjectLicense, ModTag, default_layers,
};
use ltk_wad::{HexPathResolver, Wad, WadExtractor};
use zip::ZipArchive;

use crate::FantomeInfo;
use crate::error::FantomeExtractError;
use crate::hashtable::WadHashtable;

/// Result of extracting a Fantome package.
pub struct FantomeExtractResult {
    /// The mod project configuration extracted from the Fantome package.
    pub mod_project: ModProject,
}

/// Extractor for Fantome packages.
///
/// This struct provides functionality to extract a Fantome (.fantome) archive
/// to a mod project directory structure.
pub struct FantomeExtractor<R: Read + Seek> {
    archive: ZipArchive<R>,
    hashtable: Option<WadHashtable>,
}

impl<R: Read + Seek> FantomeExtractor<R> {
    /// Create a new extractor from a reader.
    pub fn new(reader: R) -> Result<Self, FantomeExtractError> {
        let archive = ZipArchive::new(reader)?;
        Ok(Self {
            archive,
            hashtable: None,
        })
    }

    /// Set the WAD hashtable for resolving path hashes to human-readable paths.
    ///
    /// Accepts a hashtable or an `Option`. Without one, extracted files are
    /// named by their hex hash.
    pub fn with_hashtable(mut self, hashtable: impl Into<Option<WadHashtable>>) -> Self {
        self.hashtable = hashtable.into();
        self
    }

    /// Read the metadata from the Fantome package.
    pub fn read_metadata(&mut self) -> Result<FantomeInfo, FantomeExtractError> {
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

    /// Extract the Fantome package to the specified output directory.
    ///
    /// This will:
    /// 1. Extract WAD contents to content/base/
    /// 2. Extract README.md if present
    /// 3. Extract the license text if present
    /// 4. Extract thumbnail image if present
    /// 5. Create a mod.config.json file
    ///
    /// Returns the mod project configuration that was created.
    pub fn extract_to(
        &mut self,
        output_dir: &Utf8Path,
    ) -> Result<FantomeExtractResult, FantomeExtractError> {
        let info = self.read_metadata()?;
        let mod_project = ModProject {
            name: slug::slugify(&info.name),
            display_name: info.name,
            version: info.version,
            description: info.description,
            authors: vec![ModProjectAuthor::Name(info.author)],
            license: info.license.map(ModProjectLicense::from),
            tags: info.tags.into_iter().map(ModTag::from).collect(),
            champions: info.champions,
            maps: info.maps.into_iter().map(ModMap::from).collect(),
            transformers: vec![],
            layers: default_layers(),
            thumbnail: None,
        };

        if !output_dir.exists() {
            std::fs::create_dir_all(output_dir)
                .map_err(|source| FantomeExtractError::write(output_dir, source))?;
        }

        for i in 0..self.archive.len() {
            let mut file = self.archive.by_index(i)?;
            let file_name = file.name().to_string();

            if let Some(relative_path) = file_name.strip_prefix("WAD/") {
                let output_file_path = output_dir.join("content").join("base").join(relative_path);

                if file.is_dir() {
                    create_dir(&output_file_path)?;
                } else if !relative_path.contains('/') && is_wad_file_name(relative_path) {
                    // A packed WAD directly under WAD/, unpacked by WadExtractor
                    // rather than written out as a file.
                    extract_packed_wad(&mut file, &output_file_path, self.hashtable.as_ref())?;
                } else {
                    extract_entry(&mut file, &output_file_path)?;
                }
            } else if let Some(relative_path) = file_name.strip_prefix("RAW/") {
                if relative_path.is_empty() {
                    continue;
                }

                let output_file_path = output_dir.join("RAW").join(relative_path);

                if file.is_dir() {
                    create_dir(&output_file_path)?;
                } else {
                    extract_entry(&mut file, &output_file_path)?;
                }
            } else if file_name == "META/README.md" {
                extract_entry(&mut file, &output_dir.join("README.md"))?;
            } else if let Some(license_name) = license_entry_target(&file_name) {
                extract_entry(&mut file, &output_dir.join(license_name))?;
            } else if file_name == "META/image.png" {
                // Extract and convert thumbnail to WebP
                extract_thumbnail(&mut file, &output_dir.join("thumbnail.webp"))?;
            }
        }

        mod_project.save(&output_dir.join("mod.config.json"))?;

        Ok(FantomeExtractResult { mod_project })
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

    let mut outfile = File::create(output_path).map_err(write_error)?;
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

/// Extract and convert a PNG thumbnail to WebP format.
fn extract_thumbnail<R: Read>(
    reader: &mut R,
    output_path: &Utf8Path,
) -> Result<(), FantomeExtractError> {
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;

    let thumbnail_error =
        |source: image::ImageError| FantomeExtractError::Thumbnail(Box::new(source));

    let img =
        image::load_from_memory_with_format(&data, ImageFormat::Png).map_err(thumbnail_error)?;

    img.save(output_path).map_err(thumbnail_error)?;

    Ok(())
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
    use std::io::{Cursor, Write};
    use tempfile::{TempDir, tempdir};
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    /// The temp directory's path, which extraction takes as UTF-8.
    fn utf8_dir(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap()
    }

    fn create_test_fantome() -> Vec<u8> {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        // Add META/info.json
        zip.start_file("META/info.json", options).unwrap();
        let info = r#"{
            "Name": "Test Mod",
            "Author": "Test Author",
            "Version": "1.0.0",
            "Description": "A test mod"
        }"#;
        zip.write_all(info.as_bytes()).unwrap();

        // Add a WAD directory with a file
        zip.add_directory("WAD/test.wad.client", options).unwrap();
        zip.start_file("WAD/test.wad.client/assets/test.bin", options)
            .unwrap();
        zip.write_all(b"test content").unwrap();

        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn test_extract_fantome() {
        let fantome_data = create_test_fantome();
        let cursor = Cursor::new(fantome_data);

        let mut extractor = FantomeExtractor::new(cursor).unwrap();

        let temp_dir = tempdir().unwrap();
        let result = extractor.extract_to(&utf8_dir(&temp_dir)).unwrap();

        assert_eq!(result.mod_project.display_name, "Test Mod");
        assert_eq!(result.mod_project.version, "1.0.0");

        // Check that mod.config.json was created
        assert!(temp_dir.path().join("mod.config.json").exists());

        // Check that WAD content was extracted
        assert!(
            temp_dir
                .path()
                .join("content/base/test.wad.client/assets/test.bin")
                .exists()
        );
    }

    /// Build a fantome archive whose license entry is named `license_entry`.
    fn create_fantome_with_license(license_entry: &str, info: &str) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        zip.start_file("META/info.json", options).unwrap();
        zip.write_all(info.as_bytes()).unwrap();

        zip.start_file(license_entry, options).unwrap();
        zip.write_all(b"The terms.").unwrap();

        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn test_extract_license_entry_case_and_extension_variants() {
        let info =
            r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;

        for (entry, expected_file) in [
            ("META/LICENSE", "LICENSE"),
            ("META/license.md", "LICENSE.md"),
            ("meta/LICENSE.TXT", "LICENSE.txt"),
        ] {
            let data = create_fantome_with_license(entry, info);
            let mut extractor = FantomeExtractor::new(Cursor::new(data)).unwrap();

            let temp_dir = tempdir().unwrap();
            extractor.extract_to(&utf8_dir(&temp_dir)).unwrap();

            let extracted = temp_dir.path().join(expected_file);
            assert!(
                extracted.exists(),
                "expected {expected_file} for archive entry {entry}"
            );
            assert_eq!(std::fs::read_to_string(&extracted).unwrap(), "The terms.");
        }
    }

    #[test]
    fn test_extract_license_field() {
        let info = r#"{
            "Name": "Test",
            "Author": "Test",
            "Version": "1.0.0",
            "Description": "Test",
            "License": "Apache-2.0"
        }"#;
        let data = create_fantome_with_license("META/LICENSE", info);

        let mut extractor = FantomeExtractor::new(Cursor::new(data)).unwrap();
        let temp_dir = tempdir().unwrap();
        let result = extractor.extract_to(&utf8_dir(&temp_dir)).unwrap();

        assert_eq!(
            result.mod_project.license,
            Some(ltk_mod_project::ModProjectLicense::Spdx(
                "Apache-2.0".to_string()
            ))
        );
    }

    #[test]
    fn test_extract_custom_license_field_without_url() {
        let info = r#"{
            "Name": "Test",
            "Author": "Test",
            "Version": "1.0.0",
            "Description": "Test",
            "License": { "Name": "My License" }
        }"#;
        let data = create_fantome_with_license("META/LICENSE", info);

        let mut extractor = FantomeExtractor::new(Cursor::new(data)).unwrap();
        let temp_dir = tempdir().unwrap();
        let result = extractor.extract_to(&utf8_dir(&temp_dir)).unwrap();

        assert_eq!(
            result.mod_project.license,
            Some(ltk_mod_project::ModProjectLicense::Custom {
                name: "My License".to_string(),
                url: None,
            })
        );
    }

    #[test]
    fn test_extract_legacy_fantome_has_no_license() {
        let fantome_data = create_test_fantome();
        let mut extractor = FantomeExtractor::new(Cursor::new(fantome_data)).unwrap();

        let temp_dir = tempdir().unwrap();
        let result = extractor.extract_to(&utf8_dir(&temp_dir)).unwrap();

        assert_eq!(result.mod_project.license, None);
        assert!(!temp_dir.path().join("LICENSE").exists());
    }

    #[test]
    fn test_extract_raw_files() {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        // Add META/info.json
        zip.start_file("META/info.json", options).unwrap();
        let info =
            r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;
        zip.write_all(info.as_bytes()).unwrap();

        // Add RAW directory with files
        zip.add_directory("RAW", options).unwrap();
        zip.start_file("RAW/assets/characters/aatrox/skin0.bin", options)
            .unwrap();
        zip.write_all(b"aatrox data").unwrap();
        zip.start_file("RAW/assets/maps/map11/scene.bin", options)
            .unwrap();
        zip.write_all(b"map data").unwrap();

        let buffer = zip.finish().unwrap().into_inner();

        let cursor = Cursor::new(buffer);
        let mut extractor = FantomeExtractor::new(cursor).unwrap();

        let temp_dir = tempdir().unwrap();
        let result = extractor.extract_to(&utf8_dir(&temp_dir)).unwrap();
        assert_eq!(result.mod_project.display_name, "Test");

        // Check that RAW files were extracted
        let raw_file1 = temp_dir
            .path()
            .join("RAW/assets/characters/aatrox/skin0.bin");
        assert!(raw_file1.exists());
        assert_eq!(std::fs::read(&raw_file1).unwrap(), b"aatrox data");

        let raw_file2 = temp_dir.path().join("RAW/assets/maps/map11/scene.bin");
        assert!(raw_file2.exists());
        assert_eq!(std::fs::read(&raw_file2).unwrap(), b"map data");
    }
}
