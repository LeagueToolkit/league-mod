//! [`ProjectPacker`]: scans a mod project directory and builds a `.modpkg` archive.

use super::thumbnail::load_thumbnail;
use super::PackError;
use crate::{
    builder::{ModpkgBuilder, ModpkgBuilderError, ModpkgChunkBuilder, ModpkgLayerBuilder},
    metadata::CURRENT_SCHEMA_VERSION,
    utils::{hash_layer_name, PathBufExt, Utf8PathExt},
    ModpkgCompression, ModpkgLayerMetadata, ModpkgMetadata, Slug,
};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{
    find_license_file, ModIgnore, ModProject, ModProjectAuthor, ModProjectLayer, ModProjectLicense,
    MODIGNORE_FILE_NAME,
};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, Write};

use super::PackResult;

/// Maps `(path_hash, layer_hash)` to the source file on disk for each chunk.
type ChunkFileMap = HashMap<(u64, u64), Utf8PathBuf>;

/// Options for constructing a [`ProjectPacker`].
///
/// Scanning happens inside the constructor, so options must be supplied
/// there: see [`ProjectPacker::new_with_options`] and
/// [`ProjectPacker::with_mod_project_and_options`].
///
/// # Example
///
/// ```no_run
/// use camino::Utf8PathBuf;
/// use ltk_modpkg::project::{IgnoreMode, PackOptions, ProjectPacker};
///
/// // Pack everything, whatever `.modignore` says.
/// let packer = ProjectPacker::new_with_options(
///     Utf8PathBuf::from("path/to/my-mod"),
///     PackOptions::default().with_ignore(IgnoreMode::Disabled),
/// )?;
/// # Ok::<(), ltk_modpkg::project::PackError>(())
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PackOptions {
    /// Where the `.modignore` filter comes from.
    pub ignore: IgnoreMode,
}

impl PackOptions {
    /// Replace the ignore mode. The struct is `#[non_exhaustive]`, so this
    /// is how callers outside the crate set it.
    pub fn with_ignore(mut self, ignore: IgnoreMode) -> Self {
        self.ignore = ignore;
        self
    }
}

/// Where a packer's `.modignore` filter comes from.
#[derive(Debug, Clone, Default)]
pub enum IgnoreMode {
    /// Load `<project_root>/.modignore`; a missing file filters nothing.
    #[default]
    FromProject,
    /// Pack everything, whatever `.modignore` says.
    Disabled,
    /// Use a filter the caller already built. It must have been constructed
    /// with the same project root the packer is given; construction fails
    /// with [`PackError::IgnoreRootMismatch`] otherwise, rather than letting
    /// the filter silently match nothing.
    Explicit(ModIgnore),
}

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
    ignore: ModIgnore,
    chunks: Vec<ChunkEntry>,
    /// Entries `.modignore` excluded during the scan. A pruned directory is
    /// recorded once; the files beneath it are not enumerated.
    ignored: Vec<Utf8PathBuf>,
    readme: Option<Vec<u8>>,
    license_text: Option<Vec<u8>>,
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
}

impl ProjectPacker {
    /// Create a new packer by loading the mod project config from a directory.
    ///
    /// Looks for `mod.config.json` or `mod.config.toml` in `project_root`,
    /// validates the project, and scans all layer directories for content,
    /// filtered by the project's `.modignore` if one exists.
    pub fn new(project_root: Utf8PathBuf) -> Result<Self, PackError> {
        Self::new_with_options(project_root, PackOptions::default())
    }

    /// [`new`](Self::new), with explicit [`PackOptions`].
    pub fn new_with_options(
        project_root: Utf8PathBuf,
        options: PackOptions,
    ) -> Result<Self, PackError> {
        let mod_project = ModProject::load(&project_root)?;

        Self::with_mod_project_and_options(mod_project, project_root, options)
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
        Self::with_mod_project_and_options(mod_project, project_root, PackOptions::default())
    }

    /// [`with_mod_project`](Self::with_mod_project), with explicit
    /// [`PackOptions`].
    pub fn with_mod_project_and_options(
        mod_project: ModProject,
        project_root: Utf8PathBuf,
        options: PackOptions,
    ) -> Result<Self, PackError> {
        validate_project(&mod_project, &project_root)?;

        let ignore = match options.ignore {
            IgnoreMode::FromProject => ModIgnore::load(&project_root)?,
            IgnoreMode::Disabled => ModIgnore::empty(&project_root),
            IgnoreMode::Explicit(ignore) => {
                // A filter rooted elsewhere would relate every scanned path
                // to the wrong content dir and quietly filter wrong.
                let expected = project_root.join("content");
                if ignore.content_dir() != expected {
                    return Err(PackError::IgnoreRootMismatch {
                        filter_root: ignore.content_dir().to_owned(),
                        project_content_dir: expected,
                    });
                }
                ignore
            }
        };

        let mut packer = Self {
            mod_project,
            project_root,
            ignore,
            chunks: Vec::new(),
            ignored: Vec::new(),
            readme: None,
            license_text: None,
            thumbnail: None,
        };

        packer.scan_layers()?;
        packer.scan_meta_files()?;

        Ok(packer)
    }

    /// The entries `.modignore` excluded during the scan, in traversal order.
    ///
    /// A pruned directory appears once; the files beneath it are not
    /// enumerated. Read this before [`pack_to_writer`](Self::pack_to_writer),
    /// which consumes the packer; [`pack`](Self::pack) carries the list over
    /// to its [`PackResult`].
    pub fn ignored_files(&self) -> &[Utf8PathBuf] {
        &self.ignored
    }

    /// Pack to a file on disk, creating parent directories if needed.
    ///
    /// Returns [`PackResult`] with the output path on success.
    pub fn pack(mut self, output_path: &Utf8Path) -> Result<PackResult, PackError> {
        if let Some(parent) = output_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let ignored = std::mem::take(&mut self.ignored);

        let mut writer = BufWriter::new(File::create(output_path)?);
        self.pack_to_writer(&mut writer)?;

        Ok(PackResult::new(output_path, ignored))
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
            let entry_path = entry.path().into_utf8()?;

            // Ignore files are filter metadata, never content.
            if entry_path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(MODIGNORE_FILE_NAME))
            {
                continue;
            }

            if entry_path.is_dir() {
                self.scan_directory(&layer_dir, &entry_path, layer)?;
            } else if entry_path.is_file() {
                // Parents matter here: ignoring the layer directory itself
                // must drop its loose files too.
                if self
                    .ignore
                    .matched_with_parents(&entry_path, false)
                    .is_ignored()
                {
                    self.ignored.push(entry_path);
                    continue;
                }

                let rel_path = entry_path.strip_prefix_normalized(&layer_dir)?;
                self.chunks.push(ChunkEntry {
                    rel_path,
                    layer_name: layer.name.clone(),
                    wad_name: None,
                    file_path: entry_path,
                });
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
            .ok_or_else(|| PackError::MissingFileName(dir_path.to_owned()))?;

        let is_wad = dir_name.to_ascii_lowercase().ends_with(".wad.client");
        let wad_name = is_wad.then(|| dir_name.to_string());

        // For WAD directories: chunk path is relative to the WAD dir (WAD name
        // stored separately). For other directories: relative to the layer dir
        // so the directory name is preserved in the chunk path.
        let strip_base = if is_wad { dir_path } else { layer_dir };

        let mut walk = self.ignore.walk(dir_path);
        for file in walk.by_ref() {
            let file_path = file?;
            let rel_path = file_path.strip_prefix_normalized(strip_base)?;
            self.chunks.push(ChunkEntry {
                rel_path,
                layer_name: layer.name.clone(),
                wad_name: wad_name.clone(),
                file_path,
            });
        }

        self.ignored.extend(walk.into_skipped());

        Ok(())
    }

    fn scan_meta_files(&mut self) -> Result<(), PackError> {
        let readme_path = self.project_root.join("README.md");
        if readme_path.exists() {
            self.readme = Some(readme_path.read_bytes()?);
        }

        if let Some(license_path) = find_license_file(&self.project_root) {
            self.license_text = Some(license_path.read_bytes()?);
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
            builder = builder.with_layer(
                ModpkgLayerBuilder::new(&layer.name)
                    .map_err(PackError::Builder)?
                    .with_priority(layer.priority),
            );
        }

        // Metadata
        builder = builder.with_metadata(self.build_metadata()?);

        // Content chunks
        let mut file_map = ChunkFileMap::new();
        for entry in &self.chunks {
            let mut cb = ModpkgChunkBuilder::new()
                .with_path(&entry.rel_path)
                .map_err(PackError::Builder)?
                .with_compression(ModpkgCompression::for_extension(
                    entry.file_path.extension(),
                ))
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
        if let Some(readme) = self.readme {
            builder = builder.with_readme(readme);
        }
        if let Some(license_text) = self.license_text {
            builder = builder.with_license_text(license_text);
        }
        if let Some(thumbnail) = self.thumbnail {
            builder = builder.with_thumbnail(thumbnail);
        }

        Ok((builder, file_map))
    }

    fn build_metadata(&self) -> Result<ModpkgMetadata, PackError> {
        let version = semver::Version::parse(&self.mod_project.version)?;

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
        Slug::new(&layer.name).map_err(PackError::InvalidLayerName)?;
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
