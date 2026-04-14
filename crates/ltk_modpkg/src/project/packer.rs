//! [`ProjectPacker`] — scans a mod project directory and builds a `.modpkg` archive.

use super::thumbnail::load_thumbnail;
use super::PackError;
use crate::{
    builder::{ModpkgBuilder, ModpkgBuilderError, ModpkgChunkBuilder, ModpkgLayerBuilder},
    metadata::CURRENT_SCHEMA_VERSION,
    utils::hash_layer_name,
    ModpkgCompression, ModpkgLayerMetadata, ModpkgMetadata,
};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectAuthor, ModProjectLayer, ModProjectLicense};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, Write};

use super::PackResult;

/// Maps `(path_hash, layer_hash)` to the source file on disk for each chunk.
type ChunkFileMap = HashMap<(u64, u64), Utf8PathBuf>;

/// Packs a mod project directory into a `.modpkg` archive.
///
/// The packer validates the project structure, scans the content directory for
/// files, and collects all the information needed to build the archive. Call
/// [`pack`](Self::pack) to write to a file or
/// [`pack_to_writer`](Self::pack_to_writer) to write to an arbitrary output.
///
/// # Example
///
/// ```ignore
/// use ltk_modpkg::project::ProjectPacker;
/// use camino::Utf8Path;
///
/// // Pack to a file on disk
/// let result = ProjectPacker::new(project_root)?
///     .pack(Utf8Path::new("build/my-mod_1.0.0.modpkg"))?;
///
/// // Pack to an in-memory buffer (with a pre-loaded config)
/// let mut buffer = std::io::Cursor::new(Vec::new());
/// ProjectPacker::with_mod_project(mod_project, project_root)?
///     .pack_to_writer(&mut buffer)?;
/// ```
#[derive(Debug)]
pub struct ProjectPacker {
    mod_project: ModProject,
    project_root: Utf8PathBuf,
    chunks: Vec<ChunkEntry>,
    readme: Option<String>,
    thumbnail: Option<Vec<u8>>,
}

/// An individual content file collected during the project scan.
#[derive(Debug)]
struct ChunkEntry {
    /// Path relative to the WAD directory (or layer directory for non-WAD content).
    rel_path: String,
    /// Layer this chunk belongs to.
    layer_name: String,
    /// WAD association, if the file is inside a `.wad.client` directory.
    wad_name: Option<String>,
    /// Absolute path to the source file on disk.
    file_path: Utf8PathBuf,
    /// Compression strategy based on file extension.
    compression: ModpkgCompression,
}

impl ProjectPacker {
    /// Create a new packer by loading the mod project config from a directory.
    ///
    /// Looks for `mod.config.json` or `mod.config.toml` in `project_root`,
    /// validates the project, and scans all layer directories for content.
    pub fn new(project_root: Utf8PathBuf) -> Result<Self, PackError> {
        let mod_project = ModProject::load(project_root.as_std_path())
            .map_err(|e| PackError::ConfigError(e.to_string()))?;

        Self::with_mod_project(mod_project, project_root)
    }

    /// Create a new packer with an already-loaded mod project config.
    ///
    /// Use this when you have a [`ModProject`] from another source (e.g. an
    /// in-memory config or a workshop import). For the common case of packing
    /// from a project directory, prefer [`new`](Self::new).
    pub fn with_mod_project(
        mod_project: ModProject,
        project_root: Utf8PathBuf,
    ) -> Result<Self, PackError> {
        validate_project(&mod_project, &project_root)?;

        let mut packer = Self {
            mod_project,
            project_root,
            chunks: Vec::new(),
            readme: None,
            thumbnail: None,
        };

        packer.scan_layers()?;
        packer.scan_meta_files()?;

        Ok(packer)
    }

    /// Pack to a file on disk, creating parent directories if needed.
    ///
    /// Returns [`PackResult`] with the output path on success.
    pub fn pack(self, output_path: &Utf8Path) -> Result<PackResult, PackError> {
        if let Some(parent) = output_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let mut writer = BufWriter::new(File::create(output_path)?);
        self.pack_to_writer(&mut writer)?;

        Ok(PackResult {
            output_path: output_path.to_owned(),
        })
    }

    /// Pack to an arbitrary writer.
    ///
    /// This is useful for writing to in-memory buffers (e.g. for tests) or
    /// streaming to a network socket.
    pub fn pack_to_writer<W: Write + Seek>(self, writer: &mut W) -> Result<(), PackError> {
        let (builder, file_map) = self.into_builder()?;

        builder
            .build_to_writer(writer, |chunk_builder, cursor| {
                let key = (
                    chunk_builder.path_hash(),
                    hash_layer_name(chunk_builder.layer()),
                );
                let file_path = file_map.get(&key).ok_or_else(|| {
                    ModpkgBuilderError::from(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "Missing file path for chunk: {} (layer: '{}')",
                            chunk_builder.path,
                            chunk_builder.layer()
                        ),
                    ))
                })?;

                let mut file = File::open(file_path)?;
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer)?;
                cursor.write_all(&buffer)?;
                Ok(())
            })
            .map_err(PackError::Builder)?;

        Ok(())
    }

    // -- scanning ----------------------------------------------------------

    fn scan_layers(&mut self) -> Result<(), PackError> {
        let content_dir = self.project_root.join("content");

        self.scan_layer_dir(&content_dir, &ModProjectLayer::base())?;

        // Clone layer list to avoid borrowing self.mod_project while mutating self.chunks
        let layers: Vec<_> = self
            .mod_project
            .layers
            .iter()
            .filter(|l| l.name != "base")
            .cloned()
            .collect();

        for layer in &layers {
            self.scan_layer_dir(&content_dir, layer)?;
        }

        Ok(())
    }

    fn scan_layer_dir(
        &mut self,
        content_dir: &Utf8Path,
        layer: &ModProjectLayer,
    ) -> Result<(), PackError> {
        let layer_dir = content_dir.join(&layer.name);

        for entry in fs::read_dir(layer_dir.as_std_path())? {
            let entry = entry?;
            let entry_path = utf8_path_from(entry.path())?;

            if entry_path.is_dir() {
                self.scan_directory(&layer_dir, &entry_path, layer)?;
            } else if entry_path.is_file() {
                let rel_path = strip_prefix(&entry_path, &layer_dir)?;
                self.push_chunk(rel_path, layer, None, entry_path);
            }
        }

        Ok(())
    }

    fn scan_directory(
        &mut self,
        layer_dir: &Utf8Path,
        dir_path: &Utf8Path,
        layer: &ModProjectLayer,
    ) -> Result<(), PackError> {
        let dir_name = dir_path
            .file_name()
            .ok_or_else(|| PackError::InvalidUtf8Path(dir_path.to_string()))?;

        let is_wad = dir_name.to_ascii_lowercase().ends_with(".wad.client");
        let wad_name = is_wad.then(|| dir_name.to_string());

        // For WAD directories: chunk path is relative to the WAD dir (WAD name
        // stored separately). For other directories: relative to the layer dir
        // so the directory name is preserved in the chunk path.
        let strip_base = if is_wad { dir_path } else { layer_dir };

        let pattern = dir_path.join("**/*");
        for file in glob::glob(pattern.as_str())?
            .filter_map(Result::ok)
            .filter(|e| e.is_file())
        {
            let file_path = utf8_path_from(file)?;
            let rel_path = strip_prefix(&file_path, strip_base)?;
            self.push_chunk(rel_path, layer, wad_name.clone(), file_path);
        }

        Ok(())
    }

    fn push_chunk(
        &mut self,
        rel_path: String,
        layer: &ModProjectLayer,
        wad_name: Option<String>,
        file_path: Utf8PathBuf,
    ) {
        let compression = compression_for_extension(file_path.extension());
        self.chunks.push(ChunkEntry {
            rel_path,
            layer_name: layer.name.clone(),
            wad_name,
            file_path,
            compression,
        });
    }

    fn scan_meta_files(&mut self) -> Result<(), PackError> {
        let readme_path = self.project_root.join("README.md");
        if readme_path.exists() {
            self.readme = Some(fs::read_to_string(&readme_path)?);
        }

        let thumbnail_path = self
            .mod_project
            .thumbnail
            .as_ref()
            .map(|p| self.project_root.join(p))
            .unwrap_or_else(|| self.project_root.join("thumbnail.webp"));

        if thumbnail_path.exists() {
            self.thumbnail = Some(load_thumbnail(&thumbnail_path)?);
        }

        Ok(())
    }

    // -- building ----------------------------------------------------------

    /// Consume the packer and produce a configured `ModpkgBuilder` plus a map
    /// from chunk keys to source file paths.
    fn into_builder(self) -> Result<(ModpkgBuilder, ChunkFileMap), PackError> {
        let mut builder = ModpkgBuilder::default().with_layer(ModpkgLayerBuilder::base());

        // Layers
        for layer in &self.mod_project.layers {
            if layer.name == "base" {
                continue;
            }
            builder = builder
                .with_layer(ModpkgLayerBuilder::new(&layer.name).with_priority(layer.priority));
        }

        // Metadata
        builder = builder
            .with_metadata(self.build_metadata()?)
            .map_err(PackError::Builder)?;

        // Content chunks
        let mut file_map = ChunkFileMap::new();
        for entry in &self.chunks {
            let mut cb = ModpkgChunkBuilder::new()
                .with_path(&entry.rel_path)
                .map_err(PackError::Builder)?
                .with_compression(entry.compression)
                .with_layer(&entry.layer_name);

            if let Some(wad) = &entry.wad_name {
                cb = cb.with_wad(wad);
            }

            let key = (cb.path_hash(), hash_layer_name(&entry.layer_name));
            file_map
                .entry(key)
                .or_insert_with(|| entry.file_path.clone());
            builder = builder.with_chunk(cb);
        }

        // Meta chunks
        if let Some(readme) = &self.readme {
            builder = builder.with_readme(readme).map_err(PackError::Builder)?;
        }
        if let Some(thumbnail) = self.thumbnail {
            builder = builder
                .with_thumbnail(thumbnail)
                .map_err(PackError::Builder)?;
        }

        Ok((builder, file_map))
    }

    fn build_metadata(&self) -> Result<ModpkgMetadata, PackError> {
        let version = semver::Version::parse(&self.mod_project.version)
            .map_err(|e| PackError::InvalidVersion(e.to_string()))?;

        Ok(ModpkgMetadata {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: self.mod_project.name.clone(),
            display_name: self.mod_project.display_name.clone(),
            description: Some(self.mod_project.description.clone()),
            version,
            distributor: None,
            authors: self
                .mod_project
                .authors
                .iter()
                .map(convert_author)
                .collect(),
            license: convert_license(self.mod_project.license.as_ref()),
            tags: self
                .mod_project
                .tags
                .iter()
                .map(|t| t.to_string())
                .collect(),
            champions: self.mod_project.champions.clone(),
            maps: self
                .mod_project
                .maps
                .iter()
                .map(|m| m.to_string())
                .collect(),
            layers: build_layer_metadata(&self.mod_project),
        })
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_project(mod_project: &ModProject, project_root: &Utf8Path) -> Result<(), PackError> {
    for layer in &mod_project.layers {
        if !is_valid_slug(&layer.name) {
            return Err(PackError::InvalidLayerName(layer.name.clone()));
        }
        if layer.name == "base" && layer.priority != 0 {
            return Err(PackError::InvalidBaseLayerPriority(layer.priority));
        }
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
pub(super) fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

// ---------------------------------------------------------------------------
// Metadata conversion
// ---------------------------------------------------------------------------

fn convert_author(author: &ModProjectAuthor) -> crate::ModpkgAuthor {
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

fn convert_license(license: Option<&ModProjectLicense>) -> crate::ModpkgLicense {
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

fn build_layer_metadata(mod_project: &ModProject) -> Vec<ModpkgLayerMetadata> {
    let mut layers = Vec::new();

    let base_from_config = mod_project.layers.iter().find(|l| l.name == "base");
    layers.push(ModpkgLayerMetadata {
        name: "base".to_string(),
        display_name: base_from_config.and_then(|l| l.display_name.clone()),
        priority: 0,
        description: base_from_config
            .and_then(|l| l.description.clone())
            .or_else(|| Some("Base layer of the mod".to_string())),
        string_overrides: base_from_config
            .map(|l| l.string_overrides.clone())
            .unwrap_or_default(),
    });

    for layer in mod_project.layers.iter().filter(|l| l.name != "base") {
        layers.push(ModpkgLayerMetadata {
            name: layer.name.clone(),
            display_name: layer.display_name.clone(),
            priority: layer.priority,
            description: layer.description.clone(),
            string_overrides: layer.string_overrides.clone(),
        });
    }

    layers
}

// ---------------------------------------------------------------------------
// Path utilities
// ---------------------------------------------------------------------------

/// Convert a `std::path::PathBuf` to a `Utf8PathBuf`, returning a `PackError` on failure.
fn utf8_path_from(path: std::path::PathBuf) -> Result<Utf8PathBuf, PackError> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|p| PackError::InvalidUtf8Path(p.display().to_string()))
}

/// Strip a prefix from a path and return the remainder as a normalized string
/// (forward slashes, for cross-platform consistency).
fn strip_prefix(path: &Utf8Path, base: &Utf8Path) -> Result<String, PackError> {
    let rel = path
        .strip_prefix(base)
        .map_err(|e| PackError::Io(io::Error::other(e.to_string())))?;
    Ok(rel.as_str().replace('\\', "/"))
}

/// Determine the best compression strategy based on file extension.
///
/// Pre-compressed formats (textures, audio) gain little from zstd and
/// waste CPU time at both compression and decompression.
pub(super) fn compression_for_extension(ext: Option<&str>) -> ModpkgCompression {
    match ext.map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("dds" | "tex" | "webp" | "png" | "jpg" | "jpeg") => ModpkgCompression::None,
        Some("bnk" | "wpk" | "wem" | "ogg") => ModpkgCompression::None,
        _ => ModpkgCompression::Zstd,
    }
}
