//! [`ModpkgFormat`]: encodes a pack plan as a `.modpkg` archive.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, Write};

use camino::Utf8PathBuf;
use ltk_modpkg::builder::{
    ModpkgBuilder, ModpkgBuilderError, ModpkgChunkBuilder, ModpkgLayerBuilder,
};
use ltk_modpkg::{
    hash_layer_name, InvalidSlugError, ModpkgAuthor, ModpkgCompression, ModpkgLayerMetadata,
    ModpkgLicense, ModpkgMetadata, Slug, CURRENT_SCHEMA_VERSION,
};

use super::thumbnail::{load_thumbnail, ThumbnailError};
use crate::{ModProjectAuthor, ModProjectLicense, PackFormat, PackPlan, PlannedLayer};

/// Failure to encode a pack plan as a `.modpkg` archive.
///
/// Driver failures (scanning, `.modignore`, layout validation) are not here;
/// they surface as the shared variants of
/// [`PackError`](crate::PackError).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ModpkgPackError {
    /// A file in the project could not be read.
    #[error("Failed to read {path}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    /// The archive could not be built or written.
    #[error(transparent)]
    Builder(#[from] ModpkgBuilderError),

    /// A layer name is not a valid modpkg slug.
    #[error("Invalid layer name")]
    InvalidLayerName(#[source] InvalidSlugError),

    /// The project's version is not valid semver, which modpkg metadata
    /// requires.
    #[error("Invalid mod version")]
    InvalidVersion(#[from] semver::Error),

    /// The thumbnail could not be read or re-encoded for embedding.
    #[error(transparent)]
    Thumbnail(#[from] ThumbnailError),
}

impl ModpkgPackError {
    fn read(path: impl Into<Utf8PathBuf>, source: io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }
}

/// Packs a mod project into a `.modpkg` archive; the modpkg backend for
/// [`ProjectPacker`](crate::ProjectPacker).
///
/// All layers are stored. See the
/// [`pack` module docs](crate::pack) for how formats plug into the driver.
///
/// # Example
///
/// ```no_run
/// use ltk_mod_project::modpkg::ModpkgFormat;
/// use ltk_mod_project::ProjectPacker;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let packer = ProjectPacker::from_dir("path/to/my-mod")?;
/// let file = std::fs::File::create("build/my-mod_1.0.0.modpkg")?;
/// let report = packer.pack(ModpkgFormat::new(file))?;
/// println!("{} entries ignored", report.ignored_count());
/// # Ok(())
/// # }
/// ```
pub struct ModpkgFormat<W> {
    writer: W,
}

impl<W: Write + Seek> ModpkgFormat<W> {
    /// Create a format writing the archive to `writer`.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W> fmt::Debug for ModpkgFormat<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModpkgFormat").finish_non_exhaustive()
    }
}

impl<W: Write + Seek> PackFormat for ModpkgFormat<W> {
    type Error = ModpkgPackError;

    fn pack(mut self, plan: &PackPlan<'_>) -> Result<(), Self::Error> {
        let (builder, file_map) = configure_builder(plan)?;

        builder
            .build_to_writer(&mut self.writer, |chunk_builder, cursor| {
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
            .map_err(ModpkgPackError::Builder)?;

        Ok(())
    }
}

/// Maps `(path_hash, layer_hash)` to the source file on disk for each chunk.
type ChunkFileMap = HashMap<(u64, u64), Utf8PathBuf>;

/// Turn the plan into a configured `ModpkgBuilder` plus a map from chunk keys
/// to source file paths.
fn configure_builder(
    plan: &PackPlan<'_>,
) -> Result<(ModpkgBuilder, ChunkFileMap), ModpkgPackError> {
    let mut builder = ModpkgBuilder::default();

    // Layers
    for planned in plan.layers() {
        let layer = planned.layer();
        Slug::new(&layer.name).map_err(ModpkgPackError::InvalidLayerName)?;

        builder = builder.with_layer(if layer.is_base() {
            ModpkgLayerBuilder::base()
        } else {
            ModpkgLayerBuilder::new(&layer.name)
                .map_err(ModpkgPackError::Builder)?
                .with_priority(layer.priority)
        });
    }

    // Metadata
    builder = builder.with_metadata(build_metadata(plan)?);

    // Content chunks
    let mut file_map = ChunkFileMap::new();
    for planned in plan.layers() {
        let layer_name = planned.layer().name.as_str();
        for entry in planned.files() {
            let mut cb = ModpkgChunkBuilder::new()
                .with_path(entry.rel_path())
                .map_err(ModpkgPackError::Builder)?
                .with_compression(ModpkgCompression::for_extension(entry.source().extension()))
                .with_layer(layer_name);

            if let Some(wad) = entry.wad() {
                cb = cb.with_wad(wad);
            }

            let key = (cb.path_hash(), hash_layer_name(layer_name));
            file_map
                .entry(key)
                .or_insert_with(|| entry.source().to_owned());
            builder = builder.with_chunk(cb);
        }
    }

    // Meta chunks
    if let Some(readme) = plan.readme() {
        let bytes =
            std::fs::read(readme).map_err(|source| ModpkgPackError::read(readme, source))?;
        builder = builder.with_readme(bytes);
    }
    if let Some(license) = plan.license() {
        let bytes = std::fs::read(license.source())
            .map_err(|source| ModpkgPackError::read(license.source(), source))?;
        builder = builder.with_license_text(bytes);
    }
    if let Some(thumbnail) = plan.thumbnail() {
        builder = builder.with_thumbnail(load_thumbnail(thumbnail)?);
    }

    Ok((builder, file_map))
}

fn build_metadata(plan: &PackPlan<'_>) -> Result<ModpkgMetadata, ModpkgPackError> {
    let project = plan.project();
    let version = semver::Version::parse(&project.version)?;

    Ok(ModpkgMetadata {
        schema_version: CURRENT_SCHEMA_VERSION,
        name: project.name.clone(),
        display_name: project.display_name.clone(),
        description: Some(project.description.clone()),
        version,
        distributor: None,
        authors: project.authors.iter().map(convert_author).collect(),
        license: convert_license(project.license.as_ref()),
        tags: project.tags.iter().map(|t| t.to_string()).collect(),
        champions: project.champions.clone(),
        maps: project.maps.iter().map(|m| m.to_string()).collect(),
        layers: plan.layers().iter().map(convert_layer_metadata).collect(),
    })
}

// -- metadata conversion ----------------------------------------------------

fn convert_author(author: &ModProjectAuthor) -> ModpkgAuthor {
    match author {
        ModProjectAuthor::Name(name) => ModpkgAuthor {
            name: name.clone(),
            role: None,
        },
        ModProjectAuthor::Role { name, role } => ModpkgAuthor {
            name: name.clone(),
            role: Some(role.clone()),
        },
    }
}

fn convert_license(license: Option<&ModProjectLicense>) -> ModpkgLicense {
    match license {
        None => ModpkgLicense::None,
        Some(ModProjectLicense::Spdx(id)) => ModpkgLicense::Spdx {
            spdx_id: id.clone(),
        },
        Some(ModProjectLicense::Custom { name, url }) => ModpkgLicense::Custom {
            name: name.clone(),
            url: url.clone(),
        },
    }
}

fn convert_layer_metadata(planned: &PlannedLayer) -> ModpkgLayerMetadata {
    let layer = planned.layer();

    ModpkgLayerMetadata {
        name: layer.name.clone(),
        display_name: layer.display_name.clone(),
        priority: layer.priority,
        description: if layer.is_base() {
            layer
                .description
                .clone()
                .or_else(|| Some("Base layer of the mod".to_string()))
        } else {
            layer.description.clone()
        },
        string_overrides: layer.string_overrides.clone(),
    }
}
