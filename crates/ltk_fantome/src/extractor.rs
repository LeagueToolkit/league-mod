use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::Path;

use ltk_mod_project::{default_layers, ModProject, ModProjectAuthor};
use zip::ZipArchive;

use crate::error::FantomeExtractError;
use crate::FantomeInfo;

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
}

impl<R: Read + Seek> FantomeExtractor<R> {
    /// Create a new extractor from a reader.
    pub fn new(reader: R) -> Result<Self, FantomeExtractError> {
        let archive = ZipArchive::new(reader)?;
        Ok(Self { archive })
    }

    /// Validate the archive structure.
    ///
    /// Checks for unsupported features like RAW/ directories and packed WAD files.
    pub fn validate(&mut self) -> Result<(), FantomeExtractError> {
        for i in 0..self.archive.len() {
            let file = self.archive.by_index(i)?;
            let file_name = file.name();

            // Check for RAW/ directory (unsupported)
            if file_name.starts_with("RAW/") {
                return Err(FantomeExtractError::RawUnsupported);
            }

            // Check for packed WAD files in WAD/ directory
            // A packed WAD file would be directly under WAD/ without subdirectories
            // e.g., "WAD/Aatrox.wad.client" (file) vs "WAD/Aatrox.wad.client/" (directory)
            if file_name.starts_with("WAD/") && !file.is_dir() {
                let relative_path = file_name.strip_prefix("WAD/").unwrap();
                // Check if this is a direct WAD file (no path separator after WAD/)
                // e.g., "Aatrox.wad.client" with no further path components
                if !relative_path.contains('/') && is_wad_file_name(relative_path) {
                    return Err(FantomeExtractError::PackedWadUnsupported {
                        wad_name: relative_path.to_string(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Read the metadata from the Fantome package.
    pub fn read_metadata(&mut self) -> Result<FantomeInfo, FantomeExtractError> {
        let mut info_file = self
            .archive
            .by_name("META/info.json")
            .map_err(|_| FantomeExtractError::MissingMetadata)?;

        let mut info_content = String::new();
        info_file.read_to_string(&mut info_content)?;
        drop(info_file);

        let info: FantomeInfo = serde_json::from_str(&info_content)?;
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
    pub fn extract_to(&mut self, output_dir: &Path) -> Result<FantomeExtractResult, FantomeExtractError> {
        // Validate the archive structure
        self.validate()?;

        // Read metadata
        let info = self.read_metadata()?;

        // Create initial mod project
        let mut mod_project = ModProject {
            name: slug::slugify(&info.name),
            display_name: info.name,
            version: info.version,
            description: info.description,
            authors: vec![ModProjectAuthor::Name(info.author)],
            license: None,
            transformers: vec![],
            layers: default_layers(),
            thumbnail: None,
        };

        // Create output directory
        if !output_dir.exists() {
            std::fs::create_dir_all(output_dir)?;
        }

        // Track if we extract a thumbnail
        let mut has_thumbnail = false;

        // Extract files
        for i in 0..self.archive.len() {
            let mut file = self.archive.by_index(i)?;
            let file_name = file.name().to_string();

            if file_name.starts_with("WAD/") {
                // Extract WAD content to content/base/
                let relative_path = file_name.strip_prefix("WAD/").unwrap();
                let output_file_path = output_dir.join("content").join("base").join(relative_path);

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
            } else if file_name == "META/image.png" {
                // Extract thumbnail
                let output_file_path = output_dir.join("thumbnail.png");
                let mut outfile = File::create(&output_file_path)?;
                std::io::copy(&mut file, &mut outfile)?;
                has_thumbnail = true;
            }
        }

        // Update thumbnail in mod project if it was extracted
        if has_thumbnail {
            mod_project.thumbnail = Some("thumbnail.png".to_string());
        }

        // Write mod.config.json
        let config_path = output_dir.join("mod.config.json");
        let config_content = serde_json::to_string_pretty(&mod_project)?;
        let mut config_file = File::create(config_path)?;
        config_file.write_all(config_content.as_bytes())?;

        Ok(FantomeExtractResult { mod_project })
    }
}

/// Check if a filename looks like a WAD file (ends with .wad.client or similar WAD extensions)
fn is_wad_file_name(name: &str) -> bool {
    name.ends_with(".wad.client") || name.ends_with(".wad") || name.ends_with(".wad.mobile")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

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
        assert!(temp_dir
            .path()
            .join("content/base/test.wad.client/assets/test.bin")
            .exists());
    }

    #[test]
    fn test_validate_raw_unsupported() {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        // Add META/info.json
        zip.start_file("META/info.json", options).unwrap();
        let info = r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;
        zip.write_all(info.as_bytes()).unwrap();

        // Add RAW directory (unsupported)
        zip.add_directory("RAW", options).unwrap();
        zip.start_file("RAW/test.txt", options).unwrap();
        zip.write_all(b"test").unwrap();

        let buffer = zip.finish().unwrap().into_inner();

        let cursor = Cursor::new(buffer);
        let mut extractor = FantomeExtractor::new(cursor).unwrap();

        let result = extractor.validate();
        assert!(matches!(result, Err(FantomeExtractError::RawUnsupported)));
    }

    #[test]
    fn test_validate_packed_wad_unsupported() {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        // Add META/info.json
        zip.start_file("META/info.json", options).unwrap();
        let info = r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;
        zip.write_all(info.as_bytes()).unwrap();

        // Add a packed WAD file directly (unsupported)
        zip.start_file("WAD/Aatrox.wad.client", options).unwrap();
        zip.write_all(b"packed wad content").unwrap();

        let buffer = zip.finish().unwrap().into_inner();

        let cursor = Cursor::new(buffer);
        let mut extractor = FantomeExtractor::new(cursor).unwrap();

        let result = extractor.validate();
        assert!(matches!(
            result,
            Err(FantomeExtractError::PackedWadUnsupported { .. })
        ));
    }
}

