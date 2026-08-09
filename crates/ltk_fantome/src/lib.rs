use camino::Utf8Path;
use image::ImageFormat;
use indexmap::IndexMap;
use ltk_mod_project::{
    ModIgnore, ModProject, ModProjectAuthor, ModProjectLayer, ModProjectLicense, PackageFormat,
    canonical_license_file_name, find_license_file,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use zip::{ZipWriter, write::SimpleFileOptions};

pub mod error;
mod extractor;
mod hashtable;

pub use error::{FantomeExtractError, FantomePackError, WadHashtableError};
pub use extractor::{FantomeExtractResult, FantomeExtractor};
pub use hashtable::{WadHashtable, format_chunk_path_hash};

/// Fantome metadata structure that goes into info.json
#[derive(Serialize, Deserialize, Debug)]
pub struct FantomeInfo {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Author")]
    pub author: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Description")]
    pub description: String,
    /// The license the mod is distributed under.
    ///
    /// Independent of the `META/LICENSE` archive entry: this names the terms,
    /// that entry carries their text. Either, both, or neither may be present.
    #[serde(rename = "License", default, skip_serializing_if = "Option::is_none")]
    pub license: Option<FantomeLicense>,
    /// Tags/categories for the mod (e.g., "champion-skin", "sfx").
    #[serde(rename = "Tags", default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Champions this mod targets (e.g., "Aatrox", "Ahri").
    #[serde(rename = "Champions", default, skip_serializing_if = "Vec::is_empty")]
    pub champions: Vec<String>,
    /// Maps this mod targets (e.g., "Summoner's Rift", "Howling Abyss").
    #[serde(rename = "Maps", default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<String>,
    /// Per-layer metadata including string overrides.
    #[serde(rename = "Layers", default, skip_serializing_if = "HashMap::is_empty")]
    pub layers: HashMap<String, FantomeLayerInfo>,
}

/// The license declaration in a Fantome info.json.
///
/// Either a bare SPDX identifier (`"License": "MIT"`) or an object naming the
/// license with an optional link (`"License": {"Name": "...", "Url": "..."}`).
/// Note the PascalCase inner keys: this is a distinct shape from
/// [`ModProjectLicense`], whose object form uses `{name, url}`.
///
/// Unknown fields are rejected: with `Url` optional, `Name` is the only
/// required key, so an object with a misspelled `Url` would otherwise still
/// match `Custom` and quietly lose the link.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum FantomeLicense {
    Spdx(String),
    Custom {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "Url", default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
}

impl From<&ModProjectLicense> for FantomeLicense {
    fn from(license: &ModProjectLicense) -> Self {
        match license {
            ModProjectLicense::Spdx(id) => FantomeLicense::Spdx(id.clone()),
            ModProjectLicense::Custom { name, url } => FantomeLicense::Custom {
                name: name.clone(),
                url: url.clone(),
            },
        }
    }
}

impl From<FantomeLicense> for ModProjectLicense {
    fn from(license: FantomeLicense) -> Self {
        match license {
            FantomeLicense::Spdx(id) => ModProjectLicense::Spdx(id),
            FantomeLicense::Custom { name, url } => ModProjectLicense::Custom { name, url },
        }
    }
}

/// Per-layer metadata in a Fantome info.json.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FantomeLayerInfo {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(
        rename = "DisplayName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub display_name: Option<String>,
    #[serde(rename = "Priority")]
    pub priority: i32,
    /// String overrides for this layer, organized by locale.
    /// Outer key: locale (e.g., "en_us", "ko_kr", or "default")
    /// Inner map: field name -> replacement string
    #[serde(
        rename = "StringOverrides",
        default,
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub string_overrides: IndexMap<String, IndexMap<String, String>>,
}

/// Create a standard Fantome file name from a mod project.
///
/// If `custom_name` is provided, it will be used (with `.fantome` extension added if missing).
/// Otherwise, generates `{name}_{version}.fantome`.
pub fn create_file_name(mod_project: &ModProject, custom_name: Option<String>) -> String {
    mod_project.package_file_name(custom_name, PackageFormat::Fantome)
}

/// Get layers that are not supported by the Fantome format.
///
/// Fantome only supports the base layer. This returns all non-base layers
/// from the project, which can be used to warn users about data loss.
pub fn get_unsupported_layers(mod_project: &ModProject) -> Vec<&ModProjectLayer> {
    mod_project
        .layers
        .iter()
        .filter(|layer| !layer.is_base())
        .collect()
}

/// Pack a mod project into a Fantome .zip format
///
/// Files under `content/` that the project's `.modignore` excludes are not
/// packed; a missing `.modignore` excludes nothing. Use
/// [`pack_to_fantome_with_ignore`] to supply the filter yourself.
pub fn pack_to_fantome<W: Write + std::io::Seek>(
    writer: W,
    mod_project: &ModProject,
    project_root: &Utf8Path,
) -> Result<(), FantomePackError> {
    let ignore = ModIgnore::load(project_root)?;

    pack_to_fantome_with_ignore(writer, mod_project, project_root, &ignore)
}

/// [`pack_to_fantome`], with a caller-supplied `.modignore` filter.
///
/// The filter must have been constructed with the same `project_root`, or
/// the walk fails rather than silently matching nothing. Pass
/// [`ModIgnore::empty`] to pack everything.
pub fn pack_to_fantome_with_ignore<W: Write + std::io::Seek>(
    writer: W,
    mod_project: &ModProject,
    project_root: &Utf8Path,
    ignore: &ModIgnore,
) -> Result<(), FantomePackError> {
    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    // Pack base layer WAD files
    pack_base_layer(&mut zip, project_root, ignore, &options)?;

    // Pack metadata
    pack_metadata(&mut zip, mod_project, project_root, &options)?;

    zip.finish()?;
    Ok(())
}

fn pack_base_layer<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    project_root: &Utf8Path,
    ignore: &ModIgnore,
    options: &SimpleFileOptions,
) -> Result<(), FantomePackError> {
    let base_layer_path = project_root.join("content").join("base");

    if !base_layer_path.exists() {
        return Err(FantomePackError::MissingBaseLayer(base_layer_path));
    }

    let entries = base_layer_path
        .read_dir_utf8()
        .map_err(|source| FantomePackError::read(&base_layer_path, source))?;

    // Iterate through all .wad.client directories in the base layer
    for entry in entries {
        let entry = entry.map_err(|source| FantomePackError::read(&base_layer_path, source))?;

        // Case-insensitive, as the modpkg packer and overlay builder detect
        // WAD directories: `Aatrox.WAD.Client` must not silently vanish
        // from one format only.
        let is_wad = entry
            .file_name()
            .to_ascii_lowercase()
            .ends_with(".wad.client");
        if entry.path().is_dir() && is_wad {
            let zip_prefix = format!("WAD/{}", entry.file_name());
            pack_wad_directory(zip, ignore, entry.path(), &zip_prefix, options)?;
        }
    }

    Ok(())
}

/// Write every file under `wad_dir` into the archive beneath `zip_prefix`,
/// skipping whatever `.modignore` excludes. An ignored `wad_dir` contributes
/// nothing.
fn pack_wad_directory<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    ignore: &ModIgnore,
    wad_dir: &Utf8Path,
    zip_prefix: &str,
    options: &SimpleFileOptions,
) -> Result<(), FantomePackError> {
    for file in ignore.walk(wad_dir) {
        let file_path = file.map_err(|error| {
            let (path, source) = error.into_parts();
            FantomePackError::read(path, source)
        })?;

        // The walker only yields paths under the directory it was given, so
        // the prefix always strips.
        let rel_path = file_path
            .strip_prefix(wad_dir)
            .expect("walked path must sit under the walked directory");

        // Archive separators are always `/`, whatever the host uses.
        let entry_zip_path = format!("{}/{}", zip_prefix, rel_path.as_str().replace('\\', "/"));

        zip.start_file(entry_zip_path, *options)?;
        copy_file_into(zip, &file_path)?;
    }

    Ok(())
}

fn pack_metadata<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    mod_project: &ModProject,
    project_root: &Utf8Path,
    options: &SimpleFileOptions,
) -> Result<(), FantomePackError> {
    // Build layers map with string overrides
    let layers = build_fantome_layers(mod_project);

    // Create info.json
    let info = FantomeInfo {
        name: mod_project.display_name.clone(),
        author: format_authors(&mod_project.authors),
        version: mod_project.version.clone(),
        description: mod_project.description.clone(),
        license: mod_project.license.as_ref().map(FantomeLicense::from),
        tags: mod_project.tags.iter().map(|t| t.to_string()).collect(),
        champions: mod_project.champions.clone(),
        maps: mod_project.maps.iter().map(|m| m.to_string()).collect(),
        layers,
    };

    zip.start_file("META/info.json", *options)?;
    zip.write_all(&serde_json::to_string_pretty(&info)?.into_bytes())?;

    // Add README.md if it exists
    let readme_path = project_root.join("README.md");
    if readme_path.exists() {
        zip.start_file("META/README.md", *options)?;
        copy_file_into(zip, &readme_path)?;
    }

    // Add the license text, if the project ships one. The entry keeps the
    // canonical spelling of the source file's name (`license.txt` becomes
    // `META/LICENSE.txt`), which is what the extractor writes back out, so
    // pack → extract → pack does not rename the file underneath the author.
    if let Some(license_path) = find_license_file(project_root) {
        let entry_name = license_path
            .file_name()
            .and_then(canonical_license_file_name)
            .unwrap_or("LICENSE");

        zip.start_file(format!("META/{entry_name}"), *options)?;
        copy_file_into(zip, &license_path)?;
    }

    // Add image.png if thumbnail exists
    if let Some(thumbnail_path) = &mod_project.thumbnail {
        let full_thumbnail_path = project_root.join(thumbnail_path);
        if full_thumbnail_path.exists() {
            pack_image(zip, &full_thumbnail_path, options)?;
        }
    }

    Ok(())
}

/// Copy a project file into the entry the archive is currently writing.
fn copy_file_into<W: Write>(zip: &mut W, path: &Utf8Path) -> Result<(), FantomePackError> {
    let mut file = File::open(path).map_err(|source| FantomePackError::read(path, source))?;

    std::io::copy(&mut file, zip).map_err(|source| FantomePackError::read(path, source))?;

    Ok(())
}

fn pack_image<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    image_path: &Utf8Path,
    options: &SimpleFileOptions,
) -> Result<(), FantomePackError> {
    let thumbnail_error = |source: image::ImageError| FantomePackError::Thumbnail {
        path: image_path.to_owned(),
        source: Box::new(source),
    };

    let img = image::open(image_path).map_err(thumbnail_error)?;

    let mut png_buffer = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png_buffer), ImageFormat::Png)
        .map_err(thumbnail_error)?;

    zip.start_file("META/image.png", *options)?;
    zip.write_all(&png_buffer)?;

    Ok(())
}

fn build_fantome_layers(mod_project: &ModProject) -> HashMap<String, FantomeLayerInfo> {
    let mut layers = HashMap::new();
    for layer in &mod_project.layers {
        // Only include layers that have string overrides
        if !layer.string_overrides.is_empty() {
            layers.insert(
                layer.name.clone(),
                FantomeLayerInfo {
                    name: layer.name.clone(),
                    display_name: layer.display_name.clone(),
                    priority: layer.priority,
                    string_overrides: layer.string_overrides.clone(),
                },
            );
        }
    }
    layers
}

fn format_authors(authors: &[ModProjectAuthor]) -> String {
    if authors.is_empty() {
        return "Unknown".to_string();
    }

    let author_names: Vec<String> = authors
        .iter()
        .map(|author| match author {
            ModProjectAuthor::Name(name) => name.clone(),
            ModProjectAuthor::Role { name, role: _ } => name.clone(),
        })
        .collect();

    author_names.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use std::io::Cursor;
    use tempfile::TempDir;

    /// The temp directory's path, which packing takes as UTF-8.
    fn utf8_dir(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap()
    }

    fn test_project(license: Option<ModProjectLicense>) -> ModProject {
        ModProject {
            name: "test-mod".to_string(),
            display_name: "Test Mod".to_string(),
            version: "1.0.0".to_string(),
            description: "A test mod".to_string(),
            authors: vec![ModProjectAuthor::Name("Alice".to_string())],
            license,
            tags: vec![],
            champions: vec![],
            maps: vec![],
            transformers: vec![],
            layers: ltk_mod_project::default_layers(),
            thumbnail: None,
        }
    }

    /// Write a minimal project tree with one base-layer WAD file.
    fn write_project_tree(root: &Utf8Path) {
        let wad_dir = root.join("content").join("base").join("Test.wad.client");
        std::fs::create_dir_all(&wad_dir).unwrap();
        std::fs::write(wad_dir.join("data.bin"), b"content").unwrap();
    }

    fn info_json(license: Option<ModProjectLicense>) -> serde_json::Value {
        let project = test_project(license);
        let info = FantomeInfo {
            name: project.display_name.clone(),
            author: format_authors(&project.authors),
            version: project.version.clone(),
            description: project.description.clone(),
            license: project.license.as_ref().map(FantomeLicense::from),
            tags: vec![],
            champions: vec![],
            maps: vec![],
            layers: HashMap::new(),
        };
        serde_json::to_value(&info).unwrap()
    }

    #[test]
    fn info_json_emits_spdx_license() {
        let json = info_json(Some(ModProjectLicense::Spdx("MIT".to_string())));
        assert_eq!(json["License"], serde_json::json!("MIT"));
    }

    #[test]
    fn info_json_emits_custom_license() {
        let json = info_json(Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: Some("https://example.com/terms".to_string()),
        }));
        assert_eq!(
            json["License"],
            serde_json::json!({ "Name": "My License", "Url": "https://example.com/terms" })
        );

        let json = info_json(Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: None,
        }));
        assert_eq!(json["License"], serde_json::json!({ "Name": "My License" }));
    }

    #[test]
    fn info_json_omits_absent_license() {
        let json = info_json(None);
        assert!(
            json.get("License").is_none(),
            "License key must be omitted entirely, got: {json}"
        );
    }

    #[test]
    fn legacy_info_json_without_license_still_parses() {
        let legacy = r#"{
            "Name": "Old Mod",
            "Author": "Someone",
            "Version": "1.0.0",
            "Description": "Packed before licenses existed"
        }"#;

        let info: FantomeInfo = serde_json::from_str(legacy).unwrap();

        assert_eq!(info.name, "Old Mod");
        assert_eq!(info.license, None);
    }

    #[test]
    fn pack_writes_license_file_and_field() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);
        std::fs::write(root.join("LICENSE.md"), "The terms.").unwrap();

        let project = test_project(Some(ModProjectLicense::Spdx("MIT".to_string())));

        let mut buffer = Cursor::new(Vec::new());
        pack_to_fantome(&mut buffer, &project, &root).unwrap();

        buffer.set_position(0);
        let mut archive = zip::ZipArchive::new(buffer).unwrap();

        // The source file's name is preserved in the entry name.
        let mut license = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("META/LICENSE.md").unwrap(),
            &mut license,
        )
        .unwrap();
        assert_eq!(license, "The terms.");

        let mut info_content = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("META/info.json").unwrap(),
            &mut info_content,
        )
        .unwrap();
        let info: FantomeInfo = serde_json::from_str(&info_content).unwrap();
        assert_eq!(info.license, Some(FantomeLicense::Spdx("MIT".to_string())));
    }

    #[test]
    fn pack_omits_license_entry_when_project_has_none() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);

        let project = test_project(None);

        let mut buffer = Cursor::new(Vec::new());
        pack_to_fantome(&mut buffer, &project, &root).unwrap();

        buffer.set_position(0);
        let mut archive = zip::ZipArchive::new(buffer).unwrap();
        assert!(archive.by_name("META/LICENSE").is_err());
    }

    #[test]
    fn license_survives_project_fantome_project_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp).join("project");
        write_project_tree(&root);
        std::fs::write(root.join("LICENSE.txt"), "Round trip terms.").unwrap();

        let project = test_project(Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: None,
        }));

        let mut buffer = Cursor::new(Vec::new());
        pack_to_fantome(&mut buffer, &project, &root).unwrap();

        buffer.set_position(0);
        let extracted = utf8_dir(&tmp).join("extracted");
        let result = FantomeExtractor::new(buffer)
            .unwrap()
            .extract_to(&extracted)
            .unwrap();

        assert_eq!(
            result.mod_project.license,
            Some(ModProjectLicense::Custom {
                name: "My License".to_string(),
                url: None,
            })
        );

        // The file comes back under the name it went in with.
        assert_eq!(
            std::fs::read_to_string(extracted.join("LICENSE.txt")).unwrap(),
            "Round trip terms."
        );
    }

    #[test]
    fn pack_canonicalizes_license_entry_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);
        std::fs::write(root.join("license.txt"), "The terms.").unwrap();

        let mut buffer = Cursor::new(Vec::new());
        pack_to_fantome(&mut buffer, &test_project(None), &root).unwrap();

        buffer.set_position(0);
        let mut archive = zip::ZipArchive::new(buffer).unwrap();

        // A lowercase source name is written under its canonical spelling, so
        // repacking an extracted project is stable rather than case-drifting.
        assert!(archive.by_name("META/LICENSE.txt").is_ok());
    }

    #[test]
    fn custom_license_rejects_unknown_field() {
        let typoed = r#"{
            "Name": "Test",
            "Author": "Test",
            "Version": "1.0.0",
            "Description": "Test",
            "License": { "Name": "My License", "Ur1": "https://example.com/terms" }
        }"#;

        assert!(
            serde_json::from_str::<FantomeInfo>(typoed).is_err(),
            "a misspelled license key must not parse as a URL-less license"
        );
    }

    #[test]
    fn pack_skips_modignored_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);

        let wad_dir = root.join("content").join("base").join("Test.wad.client");
        std::fs::write(wad_dir.join("source.psd"), b"working file").unwrap();
        std::fs::write(root.join(".modignore"), "*.psd\n").unwrap();

        let mut buffer = Cursor::new(Vec::new());
        pack_to_fantome(&mut buffer, &test_project(None), &root).unwrap();

        buffer.set_position(0);
        let mut archive = zip::ZipArchive::new(buffer).unwrap();

        // The rest of the WAD directory is packed as before.
        assert!(archive.by_name("WAD/Test.wad.client/data.bin").is_ok());
        assert!(archive.by_name("WAD/Test.wad.client/source.psd").is_err());
    }

    #[test]
    fn pack_detects_wad_directories_case_insensitively() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);

        let wad_dir = root.join("content").join("base").join("Upper.WAD.Client");
        std::fs::create_dir_all(&wad_dir).unwrap();
        std::fs::write(wad_dir.join("data.bin"), b"data").unwrap();

        let mut buffer = Cursor::new(Vec::new());
        pack_to_fantome(&mut buffer, &test_project(None), &root).unwrap();

        buffer.set_position(0);
        let mut archive = zip::ZipArchive::new(buffer).unwrap();

        // The entry keeps the author's spelling; only detection is folded.
        assert!(archive.by_name("WAD/Upper.WAD.Client/data.bin").is_ok());
    }

    #[test]
    fn pack_applies_nested_modignore_and_never_archives_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);

        let wad_dir = root.join("content").join("base").join("Test.wad.client");
        std::fs::write(wad_dir.join("source.psd"), b"working file").unwrap();
        std::fs::write(wad_dir.join(".modignore"), "*.psd\n").unwrap();

        let mut buffer = Cursor::new(Vec::new());
        pack_to_fantome(&mut buffer, &test_project(None), &root).unwrap();

        buffer.set_position(0);
        let mut archive = zip::ZipArchive::new(buffer).unwrap();

        assert!(archive.by_name("WAD/Test.wad.client/data.bin").is_ok());
        assert!(archive.by_name("WAD/Test.wad.client/source.psd").is_err());
        assert!(
            archive.by_name("WAD/Test.wad.client/.modignore").is_err(),
            "filter metadata leaked into the archive"
        );
    }

    #[test]
    fn pack_with_explicit_empty_ignore_packs_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);

        let wad_dir = root.join("content").join("base").join("Test.wad.client");
        std::fs::write(wad_dir.join("source.psd"), b"working file").unwrap();
        std::fs::write(root.join(".modignore"), "*.psd\n").unwrap();

        let mut buffer = Cursor::new(Vec::new());
        let ignore = ltk_mod_project::ModIgnore::empty(&root);
        pack_to_fantome_with_ignore(&mut buffer, &test_project(None), &root, &ignore).unwrap();

        buffer.set_position(0);
        let mut archive = zip::ZipArchive::new(buffer).unwrap();
        assert!(archive.by_name("WAD/Test.wad.client/source.psd").is_ok());
    }

    #[test]
    fn pack_with_invalid_modignore_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);
        std::fs::write(root.join(".modignore"), "a{b\n").unwrap();

        let mut buffer = Cursor::new(Vec::new());
        let error = pack_to_fantome(&mut buffer, &test_project(None), &root).unwrap_err();

        assert!(
            matches!(error, FantomePackError::Ignore(_)),
            "expected Ignore, got {error:?}"
        );
    }

    /// A project with nothing to pack is a distinct, matchable failure rather
    /// than a string a caller can only print.
    #[test]
    fn pack_without_a_base_layer_names_the_missing_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);

        let mut buffer = Cursor::new(Vec::new());
        let error = pack_to_fantome(&mut buffer, &test_project(None), &root).unwrap_err();

        match error {
            FantomePackError::MissingBaseLayer(path) => {
                assert_eq!(path, root.join("content").join("base"));
            }
            other => panic!("expected MissingBaseLayer, got {other:?}"),
        }
    }

    #[test]
    fn pack_reports_an_unreadable_thumbnail_with_its_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);

        // Present, so packing reaches it, but not an image.
        std::fs::write(root.join("thumbnail.webp"), b"not an image").unwrap();

        let project = ModProject {
            thumbnail: Some("thumbnail.webp".to_string()),
            ..test_project(None)
        };

        let mut buffer = Cursor::new(Vec::new());
        let error = pack_to_fantome(&mut buffer, &project, &root).unwrap_err();

        match error {
            FantomePackError::Thumbnail { path, .. } => {
                assert_eq!(path, root.join("thumbnail.webp"));
            }
            other => panic!("expected Thumbnail, got {other:?}"),
        }
    }

    /// An error's own message must not repeat what its source says, or an error
    /// chain prints the same sentence twice.
    #[test]
    fn pack_error_display_does_not_embed_its_source() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);
        std::fs::write(root.join("thumbnail.webp"), b"not an image").unwrap();

        let project = ModProject {
            thumbnail: Some("thumbnail.webp".to_string()),
            ..test_project(None)
        };

        let mut buffer = Cursor::new(Vec::new());
        let error = pack_to_fantome(&mut buffer, &project, &root).unwrap_err();

        let source = std::error::Error::source(&error).unwrap().to_string();
        assert!(
            !error.to_string().contains(&source),
            "`{error}` already contains its source `{source}`"
        );
    }
}
