//! High-level utilities for packing mod projects to `.modpkg` format.
//!
//! This module requires the `project` feature to be enabled.
//!
//! # Example
//!
//! ```ignore
//! use ltk_modpkg::project::{pack_from_project, PackOptions};
//! use camino::Utf8Path;
//!
//! let project_root = Utf8Path::new("my-mod");
//! let output_path = Utf8Path::new("build/my-mod_1.0.0.modpkg");
//!
//! pack_from_project(project_root, output_path, &mod_project)?;
//! ```

use crate::{
    builder::{ModpkgBuilder, ModpkgBuilderError, ModpkgChunkBuilder, ModpkgLayerBuilder},
    metadata::CURRENT_SCHEMA_VERSION,
    utils::hash_layer_name,
    ModpkgCompression, ModpkgLayerMetadata, ModpkgMetadata,
};
use camino::{Utf8Path, Utf8PathBuf};
use image::ImageFormat;
use ltk_mod_project::{ModProject, ModProjectAuthor, ModProjectLayer, ModProjectLicense};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Cursor, Read, Write};

/// Error type for project packing operations.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Builder error: {0}")]
    Builder(#[from] ModpkgBuilderError),

    #[error("Config file not found in project directory: {0}")]
    ConfigNotFound(Utf8PathBuf),

    #[error("Layer directory missing: {layer} at {path}")]
    LayerDirMissing { layer: String, path: Utf8PathBuf },

    #[error("Invalid layer name: {0}")]
    InvalidLayerName(String),

    #[error("Base layer must have priority 0, got: {0}")]
    InvalidBaseLayerPriority(i32),

    #[error("Failed to process thumbnail: {0}")]
    ThumbnailError(String),

    #[error("Invalid version format: {0}")]
    InvalidVersion(String),

    #[error("Glob pattern error: {0}")]
    GlobError(#[from] glob::PatternError),

    #[error("Invalid UTF-8 path: {0}")]
    InvalidUtf8Path(String),
}

/// Options for packing a mod project.
#[derive(Debug, Clone, Default)]
pub struct PackOptions {
    /// Custom output file name (without path). If None, uses `{name}_{version}.modpkg`.
    pub file_name: Option<String>,
}

/// Result of a successful pack operation.
#[derive(Debug)]
pub struct PackResult {
    /// The path to the created `.modpkg` file.
    pub output_path: Utf8PathBuf,
}

/// Create a standard modpkg file name from a mod project.
///
/// If `custom_name` is provided, it will be used (with `.modpkg` extension added if missing).
/// Otherwise, generates `{name}_{version}.modpkg`.
pub fn create_file_name(mod_project: &ModProject, custom_name: Option<String>) -> String {
    match custom_name {
        Some(name) => {
            if name.ends_with(".modpkg") {
                name
            } else {
                format!("{}.modpkg", name)
            }
        }
        None => {
            format!("{}_{}.modpkg", mod_project.name, mod_project.version)
        }
    }
}

/// Pack a mod project to a `.modpkg` file.
///
/// # Arguments
///
/// * `project_root` - Path to the mod project directory (containing `mod.config.json` or `mod.config.toml`)
/// * `output_path` - Path where the `.modpkg` file will be written
/// * `mod_project` - The parsed mod project configuration
///
/// # Returns
///
/// Returns `PackResult` on success with the output path.
pub fn pack_from_project(
    project_root: &Utf8Path,
    output_path: &Utf8Path,
    mod_project: &ModProject,
) -> Result<PackResult, PackError> {
    let content_dir = project_root.join("content");

    // Validate layers
    validate_layers(mod_project, project_root)?;

    // Build the modpkg
    let mut builder = ModpkgBuilder::default().with_layer(ModpkgLayerBuilder::base());
    let mut chunk_filepaths: HashMap<(u64, u64), Utf8PathBuf> = HashMap::new();

    // Add metadata
    builder = build_metadata(builder, mod_project)?;

    // Add layers and their content
    builder = build_layers(builder, &content_dir, mod_project, &mut chunk_filepaths)?;

    // Add meta chunks (README, thumbnail)
    builder = add_meta_chunks(builder, project_root, mod_project)?;

    // Create output directory if needed
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    // Write the modpkg file
    let mut writer = BufWriter::new(File::create(output_path)?);

    builder
        .build_to_writer(&mut writer, |chunk_builder, cursor| {
            write_chunk_payload(chunk_builder, cursor, &chunk_filepaths)
                .map_err(ModpkgBuilderError::from)
        })
        .map_err(PackError::Builder)?;

    Ok(PackResult {
        output_path: output_path.to_owned(),
    })
}

/// Validate that all layers exist and have valid configuration.
fn validate_layers(mod_project: &ModProject, project_root: &Utf8Path) -> Result<(), PackError> {
    for layer in &mod_project.layers {
        // Validate layer name is a valid slug
        if !is_valid_slug(&layer.name) {
            return Err(PackError::InvalidLayerName(layer.name.clone()));
        }

        // Base layer must have priority 0
        if layer.name == "base" && layer.priority != 0 {
            return Err(PackError::InvalidBaseLayerPriority(layer.priority));
        }

        // Check layer directory exists
        let layer_dir = project_root.join("content").join(&layer.name);
        if !layer_dir.exists() {
            return Err(PackError::LayerDirMissing {
                layer: layer.name.clone(),
                path: layer_dir,
            });
        }
    }

    Ok(())
}

/// Check if a string is a valid slug (lowercase alphanumeric with hyphens).
fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

fn build_metadata(
    builder: ModpkgBuilder,
    mod_project: &ModProject,
) -> Result<ModpkgBuilder, PackError> {
    let version = semver::Version::parse(&mod_project.version)
        .map_err(|e| PackError::InvalidVersion(e.to_string()))?;

    let builder = builder
        .with_metadata(ModpkgMetadata {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: mod_project.name.clone(),
            display_name: mod_project.display_name.clone(),
            description: Some(mod_project.description.clone()),
            version,
            distributor: None,
            authors: mod_project
                .authors
                .iter()
                .map(convert_project_author)
                .collect(),
            license: convert_project_license(mod_project.license.as_ref()),
            tags: mod_project.tags.iter().map(|t| t.to_string()).collect(),
            champions: mod_project.champions.clone(),
            maps: mod_project.maps.iter().map(|m| m.to_string()).collect(),
            layers: build_metadata_layers(mod_project),
        })
        .map_err(PackError::Builder)?;

    Ok(builder)
}

/// Convert a project author to modpkg metadata author format.
fn convert_project_author(author: &ModProjectAuthor) -> crate::ModpkgAuthor {
    match author {
        ModProjectAuthor::Name(name) => crate::ModpkgAuthor {
            name: name.clone(),
            role: None,
        },
        ModProjectAuthor::Role { name, role } => crate::ModpkgAuthor {
            name: name.clone(),
            role: Some(role.clone()),
        },
    }
}

/// Convert a project license to modpkg metadata license format.
fn convert_project_license(license: Option<&ModProjectLicense>) -> crate::ModpkgLicense {
    match license {
        None => crate::ModpkgLicense::None,
        Some(ModProjectLicense::Spdx(id)) => crate::ModpkgLicense::Spdx {
            spdx_id: id.clone(),
        },
        Some(ModProjectLicense::Custom { name, url }) => crate::ModpkgLicense::Custom {
            name: name.clone(),
            url: url.clone(),
        },
    }
}

/// Build the per-layer metadata section.
fn build_metadata_layers(mod_project: &ModProject) -> Vec<ModpkgLayerMetadata> {
    let mut layers = Vec::new();

    // Base layer: always present, even if omitted from config
    let base_from_config = mod_project.layers.iter().find(|l| l.name == "base");
    let base_description = base_from_config
        .and_then(|l| l.description.clone())
        .or_else(|| Some("Base layer of the mod".to_string()));
    let base_string_overrides = base_from_config
        .map(|l| l.string_overrides.clone())
        .unwrap_or_default();

    layers.push(ModpkgLayerMetadata {
        name: "base".to_string(),
        priority: 0,
        description: base_description,
        string_overrides: base_string_overrides,
    });

    // Non-base layers
    for layer in mod_project.layers.iter().filter(|l| l.name != "base") {
        layers.push(ModpkgLayerMetadata {
            name: layer.name.clone(),
            priority: layer.priority,
            description: layer.description.clone(),
            string_overrides: layer.string_overrides.clone(),
        });
    }

    layers
}

fn build_layers(
    mut builder: ModpkgBuilder,
    content_dir: &Utf8Path,
    mod_project: &ModProject,
    chunk_filepaths: &mut HashMap<(u64, u64), Utf8PathBuf>,
) -> Result<ModpkgBuilder, PackError> {
    // Process base layer
    builder = build_layer_from_dir(
        builder,
        content_dir,
        &ModProjectLayer::base(),
        chunk_filepaths,
    )?;

    // Process additional layers
    for layer in &mod_project.layers {
        if layer.name == "base" {
            continue;
        }

        builder =
            builder.with_layer(ModpkgLayerBuilder::new(&layer.name).with_priority(layer.priority));
        builder = build_layer_from_dir(builder, content_dir, layer, chunk_filepaths)?;
    }

    Ok(builder)
}

fn build_layer_from_dir(
    mut builder: ModpkgBuilder,
    content_dir: &Utf8Path,
    layer: &ModProjectLayer,
    chunk_filepaths: &mut HashMap<(u64, u64), Utf8PathBuf>,
) -> Result<ModpkgBuilder, PackError> {
    let layer_dir = content_dir.join(&layer.name);
    let pattern = layer_dir.join("**/*");

    for entry in glob::glob(pattern.as_str())?
        .filter_map(Result::ok)
        .filter(|e| e.is_file())
    {
        let entry = Utf8PathBuf::from_path_buf(entry)
            .map_err(|p| PackError::InvalidUtf8Path(p.display().to_string()))?;

        let layer_hash = hash_layer_name(&layer.name);
        let (new_builder, path_hash) = build_chunk_from_file(builder, layer, &entry, &layer_dir)?;

        chunk_filepaths
            .entry((path_hash, layer_hash))
            .or_insert(entry);

        builder = new_builder;
    }

    Ok(builder)
}

fn build_chunk_from_file(
    builder: ModpkgBuilder,
    layer: &ModProjectLayer,
    file_path: &Utf8Path,
    layer_dir: &Utf8Path,
) -> Result<(ModpkgBuilder, u64), PackError> {
    let relative_path = file_path
        .strip_prefix(layer_dir)
        .map_err(|e| PackError::Io(io::Error::other(e.to_string())))?;

    let chunk_builder = ModpkgChunkBuilder::new()
        .with_path(relative_path.as_str())
        .map_err(PackError::Builder)?
        .with_compression(ModpkgCompression::Zstd)
        .with_layer(&layer.name);

    let path_hash = chunk_builder.path_hash();
    Ok((builder.with_chunk(chunk_builder), path_hash))
}

fn add_meta_chunks(
    mut builder: ModpkgBuilder,
    project_root: &Utf8Path,
    mod_project: &ModProject,
) -> Result<ModpkgBuilder, PackError> {
    // README.md as meta chunk (optional)
    let readme_path = project_root.join("README.md");
    if readme_path.exists() {
        let readme_content = fs::read_to_string(&readme_path)?;
        builder = builder
            .with_readme(&readme_content)
            .map_err(PackError::Builder)?;
    }

    // Thumbnail as meta chunk (optional)
    let thumbnail_path = mod_project
        .thumbnail
        .as_ref()
        .map(|p| project_root.join(p))
        .unwrap_or_else(|| project_root.join("thumbnail.webp"));

    if thumbnail_path.exists() {
        let thumbnail_data = load_thumbnail(&thumbnail_path)?;
        builder = builder
            .with_thumbnail(thumbnail_data)
            .map_err(PackError::Builder)?;
    }

    Ok(builder)
}

/// Maximum thumbnail file size: 5MB
pub const MAX_THUMBNAIL_SIZE: u64 = 5 * 1024 * 1024;

/// Load and convert a thumbnail image to WebP format.
///
/// Supports all common image formats (PNG, JPEG, GIF, BMP, TIFF, ICO, WebP).
/// Animated GIFs are converted to animated WebP.
/// Validates file size (max 5MB).
///
/// # Arguments
///
/// * `path` - Path to the thumbnail image file
///
/// # Returns
///
/// WebP-encoded image data as bytes
pub fn load_thumbnail(path: &Utf8Path) -> Result<Vec<u8>, PackError> {
    let metadata = fs::metadata(path).map_err(PackError::Io)?;
    if metadata.len() > MAX_THUMBNAIL_SIZE {
        return Err(PackError::ThumbnailError(format!(
            "Thumbnail file size ({} bytes) exceeds maximum allowed size ({} bytes / 5MB)",
            metadata.len(),
            MAX_THUMBNAIL_SIZE
        )));
    }

    let extension = path
        .extension()
        .map(|ext| ext.to_lowercase())
        .unwrap_or_default();

    if extension == "webp" {
        let data = fs::read(path).map_err(PackError::Io)?;
        // Validate WebP magic bytes
        if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
            return Ok(data);
        }
        return Err(PackError::ThumbnailError(
            "Invalid WebP file format".to_string(),
        ));
    }

    if extension == "gif" {
        return convert_gif_to_webp(path);
    }

    let img = image::open(path)
        .map_err(|e| PackError::ThumbnailError(format!("Failed to open image: {}", e)))?;

    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, ImageFormat::WebP)
        .map_err(|e| PackError::ThumbnailError(format!("Failed to convert to WebP: {}", e)))?;

    Ok(buffer.into_inner())
}

fn convert_gif_to_webp(path: &Utf8Path) -> Result<Vec<u8>, PackError> {
    let file = File::open(path).map_err(PackError::Io)?;
    let reader = BufReader::new(file);
    let decoder = image::codecs::gif::GifDecoder::new(reader)
        .map_err(|e| PackError::ThumbnailError(format!("Failed to decode GIF: {}", e)))?;

    let frames: Vec<_> = image::AnimationDecoder::into_frames(decoder)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| PackError::ThumbnailError(format!("Failed to read GIF frames: {}", e)))?;

    if frames.is_empty() {
        return Err(PackError::ThumbnailError("GIF has no frames".to_string()));
    }

    if frames.len() == 1 {
        let frame = &frames[0];
        let img = frame.buffer();
        let mut buffer = Cursor::new(Vec::new());
        img.write_to(&mut buffer, ImageFormat::WebP).map_err(|e| {
            PackError::ThumbnailError(format!("Failed to convert GIF to WebP: {}", e))
        })?;
        return Ok(buffer.into_inner());
    }

    encode_animated_webp(&frames)
}

fn encode_animated_webp(frames: &[image::Frame]) -> Result<Vec<u8>, PackError> {
    use webp_animation::prelude::*;

    if frames.is_empty() {
        return Err(PackError::ThumbnailError("No frames to encode".to_string()));
    }

    let first_frame = frames[0].buffer();
    let (width, height) = first_frame.dimensions();

    let mut encoder = Encoder::new((width, height)).map_err(|e| {
        PackError::ThumbnailError(format!("Failed to create WebP encoder: {:?}", e))
    })?;

    let mut timestamp_ms = 0i32;
    for frame in frames {
        let img_buffer = frame.buffer();
        let delay = frame.delay();
        let rgba_data = img_buffer.as_raw();

        encoder
            .add_frame(rgba_data, timestamp_ms)
            .map_err(|e| PackError::ThumbnailError(format!("Failed to add frame: {:?}", e)))?;

        let delay_ms = delay.numer_denom_ms();
        timestamp_ms += delay_ms.0 as i32;
    }

    let webp_data = encoder
        .finalize(timestamp_ms)
        .map_err(|e| PackError::ThumbnailError(format!("Failed to finalize animation: {:?}", e)))?;

    Ok(webp_data.to_vec())
}

fn write_chunk_payload(
    chunk_builder: &ModpkgChunkBuilder,
    cursor: &mut Cursor<Vec<u8>>,
    chunk_filepaths: &HashMap<(u64, u64), Utf8PathBuf>,
) -> io::Result<()> {
    // Content chunks - look up file path from the map
    let key = (
        chunk_builder.path_hash(),
        hash_layer_name(chunk_builder.layer()),
    );
    if let Some(file_path) = chunk_filepaths.get(&key) {
        let mut file = File::open(file_path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        cursor.write_all(&buffer)?;
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "Missing file path for chunk: {} (layer: '{}')",
            chunk_builder.path,
            chunk_builder.layer()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_file_name() {
        let project = ModProject {
            name: "my-mod".to_string(),
            display_name: "My Mod".to_string(),
            version: "1.2.3".to_string(),
            description: String::new(),
            authors: vec![],
            license: None,
            tags: vec![],
            champions: vec![],
            maps: vec![],
            thumbnail: None,
            layers: vec![],
            transformers: vec![],
        };

        assert_eq!(create_file_name(&project, None), "my-mod_1.2.3.modpkg");
        assert_eq!(
            create_file_name(&project, Some("custom".to_string())),
            "custom.modpkg"
        );
        assert_eq!(
            create_file_name(&project, Some("custom.modpkg".to_string())),
            "custom.modpkg"
        );
    }

    #[test]
    fn test_is_valid_slug() {
        assert!(is_valid_slug("base"));
        assert!(is_valid_slug("my-layer"));
        assert!(is_valid_slug("layer123"));
        assert!(!is_valid_slug(""));
        assert!(!is_valid_slug("-invalid"));
        assert!(!is_valid_slug("invalid-"));
        assert!(!is_valid_slug("UPPERCASE"));
        assert!(!is_valid_slug("has spaces"));
    }
}
