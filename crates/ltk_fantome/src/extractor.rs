use std::fs::File;
use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

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
    /// If not set, extracted files will use hex hashes as filenames.
    pub fn with_hashtable(mut self, hashtable: WadHashtable) -> Self {
        self.hashtable = Some(hashtable);
        self
    }

    /// Set the WAD hashtable from an optional value.
    pub fn with_hashtable_opt(mut self, hashtable: Option<WadHashtable>) -> Self {
        self.hashtable = hashtable;
        self
    }

    /// Validate the archive structure.
    pub fn validate(&mut self) -> Result<(), FantomeExtractError> {
        Ok(())
    }

    /// Read the metadata from the Fantome package.
    pub fn read_metadata(&mut self) -> Result<FantomeInfo, FantomeExtractError> {
        // Try common variations of the metadata path (case-insensitive search)
        let metadata_paths = ["META/info.json", "meta/info.json", "Meta/info.json"];

        let mut info_content = String::new();
        let mut found = false;

        for path in &metadata_paths {
            if let Ok(mut info_file) = self.archive.by_name(path) {
                info_file.read_to_string(&mut info_content)?;
                found = true;
                break;
            }
        }

        // If not found by exact path, search case-insensitively
        if !found {
            for i in 0..self.archive.len() {
                let file = self.archive.by_index(i)?;
                let name = file.name().to_lowercase();
                if name == "meta/info.json" {
                    drop(file);
                    let mut info_file = self.archive.by_index(i)?;
                    info_file.read_to_string(&mut info_content)?;
                    found = true;
                    break;
                }
            }
        }

        if !found {
            return Err(FantomeExtractError::MissingMetadata);
        }

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
    /// 1. Validate the archive structure
    /// 2. Extract WAD contents to content/base/
    /// 3. Extract README.md if present
    /// 4. Extract thumbnail image if present
    /// 5. Create a mod.config.json file
    ///
    /// Returns the mod project configuration that was created.
    pub fn extract_to(
        &mut self,
        output_dir: &Path,
    ) -> Result<FantomeExtractResult, FantomeExtractError> {
        self.validate()?;

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
            std::fs::create_dir_all(output_dir)?;
        }

        for i in 0..self.archive.len() {
            let mut file = self.archive.by_index(i)?;
            let file_name = file.name().to_string();

            if file_name.starts_with("WAD/") {
                let relative_path = file_name.strip_prefix("WAD/").unwrap();

                // Check if this is a packed WAD file (directly under WAD/, ends with .wad.client etc.)
                if !file.is_dir() && !relative_path.contains('/') && is_wad_file_name(relative_path)
                {
                    // Extract packed WAD file using WadExtractor
                    let wad_output_dir =
                        output_dir.join("content").join("base").join(relative_path);
                    extract_packed_wad(&mut file, &wad_output_dir, self.hashtable.as_ref())?;
                } else {
                    // Extract WAD folder content to content/base/
                    let output_file_path =
                        output_dir.join("content").join("base").join(relative_path);

                    if file.is_dir() {
                        std::fs::create_dir_all(&output_file_path)?;
                    } else {
                        if let Some(parent) = output_file_path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let mut outfile = File::create(&output_file_path)?;
                        std::io::copy(&mut file, &mut outfile)?;
                    }
                }
            } else if file_name.starts_with("RAW/") {
                let relative_path = file_name.strip_prefix("RAW/").unwrap();
                if relative_path.is_empty() {
                    continue;
                }

                let output_file_path = output_dir.join("RAW").join(relative_path);

                if file.is_dir() {
                    std::fs::create_dir_all(&output_file_path)?;
                } else {
                    if let Some(parent) = output_file_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let mut outfile = File::create(&output_file_path)?;
                    std::io::copy(&mut file, &mut outfile)?;
                }
            } else if file_name == "META/README.md" {
                // Extract README
                let output_file_path = output_dir.join("README.md");
                let mut outfile = File::create(&output_file_path)?;
                std::io::copy(&mut file, &mut outfile)?;
            } else if let Some(license_name) = license_entry_target(&file_name) {
                // Extract the license text
                let output_file_path = output_dir.join(license_name);
                let mut outfile = File::create(&output_file_path)?;
                std::io::copy(&mut file, &mut outfile)?;
            } else if file_name == "META/image.png" {
                // Extract and convert thumbnail to WebP
                let output_file_path = output_dir.join("thumbnail.webp");
                extract_thumbnail(&mut file, &output_file_path)?;
            }
        }

        // Write mod.config.json
        let config_path = output_dir.join("mod.config.json");
        let config_content = serde_json::to_string_pretty(&mod_project)?;
        let mut config_file = File::create(config_path)?;
        config_file.write_all(config_content.as_bytes())?;

        Ok(FantomeExtractResult { mod_project })
    }
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
    output_path: &Path,
) -> Result<(), FantomeExtractError> {
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;

    let img = image::load_from_memory_with_format(&data, ImageFormat::Png)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    img.save(output_path).map_err(std::io::Error::other)?;

    Ok(())
}

/// Extract a packed WAD file to a directory using WadExtractor
fn extract_packed_wad<R: Read>(
    wad_reader: &mut R,
    output_dir: &Path,
    hashtable: Option<&WadHashtable>,
) -> Result<(), FantomeExtractError> {
    let mut wad_data = Vec::new();
    wad_reader.read_to_end(&mut wad_data)?;

    let cursor = Cursor::new(wad_data);
    let mut wad = Wad::mount(cursor)?;

    std::fs::create_dir_all(output_dir)?;

    let output_dir_utf8 = Utf8Path::from_path(output_dir).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid UTF-8 path")
    })?;

    if let Some(ht) = hashtable {
        let extractor = WadExtractor::new(ht);
        extractor.extract_all(&mut wad, output_dir_utf8)?;
    } else {
        let resolver = HexPathResolver;
        let extractor = WadExtractor::new(&resolver);
        extractor.extract_all(&mut wad, output_dir_utf8)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::tempdir;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

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
        let result = extractor.extract_to(temp_dir.path()).unwrap();

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
            extractor.extract_to(temp_dir.path()).unwrap();

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
        let result = extractor.extract_to(temp_dir.path()).unwrap();

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
        let result = extractor.extract_to(temp_dir.path()).unwrap();

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
        let result = extractor.extract_to(temp_dir.path()).unwrap();

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
        let result = extractor.extract_to(temp_dir.path()).unwrap();
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
