use eyre::Result;
use image::ImageFormat;
use ltk_mod_project::{ModProject, ModProjectAuthor, ModProjectLayer};
use serde::{Deserialize, Serialize};
use std::fs::{File, read_dir};
use std::io::Write;
use std::path::Path;
use zip::{ZipWriter, write::SimpleFileOptions};

pub mod error;
mod extractor;

pub use error::FantomeExtractError;
pub use extractor::{FantomeExtractResult, FantomeExtractor};

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
}

/// Create a standard Fantome file name from a mod project.
///
/// If `custom_name` is provided, it will be used (with `.fantome` extension added if missing).
/// Otherwise, generates `{name}_{version}.fantome`.
pub fn create_file_name(mod_project: &ModProject, custom_name: Option<String>) -> String {
    match custom_name {
        Some(name) => {
            if name.ends_with(".fantome") {
                name
            } else {
                format!("{}.fantome", name)
            }
        }
        None => {
            format!("{}_{}.fantome", mod_project.name, mod_project.version)
        }
    }
}

/// Get layers that are not supported by the Fantome format.
///
/// Fantome only supports the base layer. This returns all non-base layers
/// from the project, which can be used to warn users about data loss.
pub fn get_unsupported_layers(mod_project: &ModProject) -> Vec<&ModProjectLayer> {
    mod_project
        .layers
        .iter()
        .filter(|layer| layer.name != "base")
        .collect()
}

/// Check if the mod project has layers that won't be included in Fantome format.
pub fn has_unsupported_layers(mod_project: &ModProject) -> bool {
    mod_project.layers.iter().any(|layer| layer.name != "base")
}

/// Pack a mod project into a Fantome .zip format
pub fn pack_to_fantome<W: Write + std::io::Seek>(
    writer: W,
    mod_project: &ModProject,
    project_root: &Path,
) -> Result<()> {
    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    // Pack base layer WAD files
    pack_base_layer(&mut zip, project_root, &options)?;

    // Pack metadata
    pack_metadata(&mut zip, mod_project, project_root, &options)?;

    zip.finish()?;
    Ok(())
}

fn pack_base_layer<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    project_root: &Path,
    options: &SimpleFileOptions,
) -> Result<()> {
    let base_layer_path = project_root.join("content").join("base");

    if !base_layer_path.exists() {
        return Err(eyre::eyre!(
            "Base layer directory does not exist: {}",
            base_layer_path.display()
        ));
    }

    // Iterate through all .wad.client directories in the base layer
    for entry in read_dir(&base_layer_path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir()
            && path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".wad.client")
        {
            let wad_name = path.file_name().unwrap().to_string_lossy();
            pack_wad_directory(zip, &path, &format!("WAD/{}", wad_name), options)?;
        }
    }

    Ok(())
}

fn pack_wad_directory<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    wad_dir: &Path,
    zip_prefix: &str,
    options: &SimpleFileOptions,
) -> Result<()> {
    for entry in walkdir::WalkDir::new(wad_dir).into_iter() {
        let entry = entry.map_err(|e| eyre::eyre!("Failed to walk directory: {}", e))?;
        let path = entry.path();

        if path.is_file() {
            let relative_path = path.strip_prefix(wad_dir)?;
            let zip_path = format!(
                "{}/{}",
                zip_prefix,
                relative_path.to_string_lossy().replace('\\', "/")
            );

            zip.start_file(zip_path, *options)?;
            let mut file = File::open(path)?;
            std::io::copy(&mut file, zip)?;
        }
    }

    Ok(())
}

fn pack_metadata<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    mod_project: &ModProject,
    project_root: &Path,
    options: &SimpleFileOptions,
) -> Result<()> {
    // Create info.json
    let info = FantomeInfo {
        name: mod_project.display_name.clone(),
        author: format_authors(&mod_project.authors),
        version: mod_project.version.clone(),
        description: mod_project.description.clone(),
    };

    zip.start_file("META/info.json", *options)?;
    zip.write_all(&serde_json::to_string_pretty(&info)?.into_bytes())?;

    // Add README.md if it exists
    let readme_path = project_root.join("README.md");
    if readme_path.exists() {
        zip.start_file("META/README.md", *options)?;
        let mut readme_file = File::open(readme_path)?;
        std::io::copy(&mut readme_file, zip)?;
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

fn pack_image<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    image_path: &Path,
    options: &SimpleFileOptions,
) -> Result<()> {
    let img = image::open(image_path)?;

    let mut png_buffer = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png_buffer), ImageFormat::Png)?;

    zip.start_file("META/image.png", *options)?;
    zip.write_all(&png_buffer)?;

    Ok(())
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
