//! [`ProjectPacker`]: the format-neutral packing driver.

use std::fs;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};

use super::plan::{PackPlan, PlannedFile, PlannedLayer, PlannedLicense};
use super::{IgnoreMode, PackFormat, PackOptions};
use crate::{
    canonical_license_file_name, find_license_file, ContentWalkError, ModIgnore, ModIgnoreError,
    ModProject, ModProjectError, ModProjectLayer, MODIGNORE_FILE_NAME,
};

/// Failure to pack a mod project.
///
/// The variants here are the driver's: they can occur whatever format is
/// being packed. Format-specific failures arrive through the transparent
/// [`Format`](Self::Format) variant, so matching on a concrete format's
/// error is one level deep: `PackError::Format(inner)`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PackError<E> {
    /// A project directory could not be read during the scan.
    #[error("Failed to scan {path}")]
    Scan {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    /// A scanned path is not valid UTF-8, which no archive format stores.
    #[error("Path is not valid UTF-8: {}", .0.display())]
    NonUtf8Path(std::path::PathBuf),

    /// A configured layer has no directory under `content/`.
    #[error("Layer directory missing: {layer} at {path}")]
    LayerDirMissing { layer: String, path: Utf8PathBuf },

    /// The base layer is configured with a priority other than 0.
    #[error("Base layer must have priority 0, got: {0}")]
    InvalidBaseLayerPriority(i32),

    /// The project's `.modignore` could not be loaded: the file is
    /// unreadable, or a pattern in it does not parse. A broken pattern fails
    /// the pack rather than silently shipping files the author excluded.
    #[error(transparent)]
    Ignore(#[from] ModIgnoreError),

    /// A content directory or file could not be read during the scan.
    #[error(transparent)]
    Walk(#[from] ContentWalkError),

    /// An [`IgnoreMode::Explicit`] filter was built for a different project
    /// root than the packer was given, so it would relate every scanned path
    /// to the wrong content dir and quietly filter wrong.
    #[error("Ignore filter is rooted at {filter_root}, but the project's content dir is {project_content_dir}")]
    IgnoreRootMismatch {
        filter_root: Utf8PathBuf,
        project_content_dir: Utf8PathBuf,
    },

    /// The format failed to write the archive; see the format's own error
    /// type for the cases.
    #[error(transparent)]
    Format(E),
}

impl<E> PackError<E> {
    fn scan(path: impl Into<Utf8PathBuf>, source: io::Error) -> Self {
        Self::Scan {
            path: path.into(),
            source,
        }
    }
}

/// Result of a successful pack operation.
#[derive(Debug, Clone, PartialEq)]
pub struct PackReport {
    ignored: Vec<Utf8PathBuf>,
}

impl PackReport {
    /// The entries `.modignore` excluded from the archive, in traversal
    /// order. A pruned directory appears once; the files beneath it are not
    /// enumerated.
    pub fn ignored_files(&self) -> &[Utf8PathBuf] {
        &self.ignored
    }

    /// `ignored_files().len()`, for reporting.
    pub fn ignored_count(&self) -> usize {
        self.ignored.len()
    }
}

/// Packs a mod project directory into an archive format.
///
/// The packer is format-neutral: it owns everything about reading the
/// project (config, validation, the single `.modignore`-filtered walk of
/// `content/`), and hands the result to a [`PackFormat`] implementation,
/// which owns everything about writing its archive. See the
/// [module docs](crate::pack) for an example.
#[derive(Debug, Clone)]
pub struct ProjectPacker {
    project: ModProject,
    project_root: Utf8PathBuf,
    options: PackOptions,
}

impl ProjectPacker {
    /// Create a packer by loading the mod project config from a directory.
    ///
    /// Looks for `mod.config.json` or `mod.config.toml` in `project_root`.
    ///
    /// # Errors
    ///
    /// Fails with [`ModProjectError`] when the directory holds no config
    /// file, or the config does not parse.
    pub fn from_dir(project_root: impl Into<Utf8PathBuf>) -> Result<Self, ModProjectError> {
        let project_root = project_root.into();
        let project = ModProject::load(&project_root)?;
        Ok(Self::new(project, project_root))
    }

    /// Create a packer with an already-loaded mod project config.
    ///
    /// Use this when you have a [`ModProject`] from another source (e.g. an
    /// in-memory config or a workshop import). For the common case of packing
    /// from a project directory, prefer [`from_dir`](Self::from_dir).
    pub fn new(project: ModProject, project_root: impl Into<Utf8PathBuf>) -> Self {
        Self {
            project,
            project_root: project_root.into(),
            options: PackOptions::default(),
        }
    }

    /// Replace the pack options.
    #[must_use]
    pub fn with_options(mut self, options: PackOptions) -> Self {
        self.options = options;
        self
    }

    /// The project being packed.
    pub fn project(&self) -> &ModProject {
        &self.project
    }

    /// The project's root directory.
    pub fn project_root(&self) -> &Utf8Path {
        &self.project_root
    }

    /// The options the pack will run with.
    pub fn options(&self) -> &PackOptions {
        &self.options
    }

    /// Pack the project into `format`.
    ///
    /// Validates the layer layout, walks `content/` once with the
    /// `.modignore` filter, and hands the resulting [`PackPlan`] to the
    /// format. The report carries what the filter excluded.
    ///
    /// # Errors
    ///
    /// Driver failures (scanning, `.modignore`, layer layout) surface as
    /// [`PackError`]'s own variants; a failure inside `format` surfaces as
    /// [`PackError::Format`].
    pub fn pack<F: PackFormat>(&self, format: F) -> Result<PackReport, PackError<F::Error>> {
        let layers = self.layer_set();
        self.validate_layers(&layers)?;

        // Owned storage for the modes that build their filter here; the
        // explicit mode borrows the caller's.
        let loaded;
        let ignore = match &self.options.ignore {
            IgnoreMode::FromProject => {
                loaded = ModIgnore::load(&self.project_root)?;
                &loaded
            }
            IgnoreMode::Disabled => {
                loaded = ModIgnore::empty(&self.project_root);
                &loaded
            }
            IgnoreMode::Explicit(ignore) => {
                let expected = self.project_root.join("content");
                if ignore.content_dir() != expected {
                    return Err(PackError::IgnoreRootMismatch {
                        filter_root: ignore.content_dir().to_owned(),
                        project_content_dir: expected,
                    });
                }
                ignore
            }
        };

        let content_dir = self.project_root.join("content");
        let mut ignored = Vec::new();
        let mut planned = Vec::with_capacity(layers.len());
        for layer in layers {
            let files = scan_layer(&content_dir, &layer, ignore, &mut ignored)?;
            planned.push(PlannedLayer::new(layer, files));
        }

        let plan = PackPlan::new(
            &self.project,
            &self.project_root,
            planned,
            self.find_readme(),
            self.find_license(),
            self.find_thumbnail(),
        );

        format.pack(&plan).map_err(PackError::Format)?;

        Ok(PackReport { ignored })
    }

    /// The layers a pack covers: the base layer first (synthesized when the
    /// config omits it), then the configured non-base layers in config order.
    fn layer_set(&self) -> Vec<ModProjectLayer> {
        let base = self
            .project
            .layers
            .iter()
            .find(|layer| layer.is_base())
            .cloned()
            .unwrap_or_else(ModProjectLayer::base);

        std::iter::once(base)
            .chain(
                self.project
                    .layers
                    .iter()
                    .filter(|layer| !layer.is_base())
                    .cloned(),
            )
            .collect()
    }

    fn validate_layers<E>(&self, layers: &[ModProjectLayer]) -> Result<(), PackError<E>> {
        for layer in layers {
            if layer.is_base() && layer.priority != 0 {
                return Err(PackError::InvalidBaseLayerPriority(layer.priority));
            }
            let layer_dir = self.project_root.join("content").join(&layer.name);
            if !layer_dir.exists() {
                return Err(PackError::LayerDirMissing {
                    layer: layer.name.clone(),
                    path: layer_dir,
                });
            }
        }
        Ok(())
    }

    fn find_readme(&self) -> Option<Utf8PathBuf> {
        let path = self.project_root.join("README.md");
        path.exists().then_some(path)
    }

    fn find_license(&self) -> Option<PlannedLicense> {
        let path = find_license_file(&self.project_root)?;
        let canonical = path
            .file_name()
            .and_then(canonical_license_file_name)
            .unwrap_or("LICENSE");
        Some(PlannedLicense::new(path, canonical))
    }

    fn find_thumbnail(&self) -> Option<Utf8PathBuf> {
        let path = match &self.project.thumbnail {
            Some(configured) => self.project_root.join(configured),
            None => self.project_root.join("thumbnail.webp"),
        };
        path.exists().then_some(path)
    }
}

// -- scanning ---------------------------------------------------------------

fn scan_layer<E>(
    content_dir: &Utf8Path,
    layer: &ModProjectLayer,
    ignore: &ModIgnore,
    ignored: &mut Vec<Utf8PathBuf>,
) -> Result<Vec<PlannedFile>, PackError<E>> {
    let layer_dir = content_dir.join(&layer.name);
    let mut files = Vec::new();

    let entries = fs::read_dir(layer_dir.as_std_path())
        .map_err(|source| PackError::scan(&layer_dir, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| PackError::scan(&layer_dir, source))?;
        let entry_path =
            Utf8PathBuf::from_path_buf(entry.path()).map_err(PackError::NonUtf8Path)?;

        // Ignore files are filter metadata, never content.
        if entry_path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(MODIGNORE_FILE_NAME))
        {
            continue;
        }

        if entry_path.is_dir() {
            scan_directory(&layer_dir, &entry_path, ignore, ignored, &mut files)?;
        } else if entry_path.is_file() {
            // Parents matter here: ignoring the layer directory itself must
            // drop its loose files too.
            if ignore.matched_with_parents(&entry_path, false).is_ignored() {
                ignored.push(entry_path);
                continue;
            }

            let rel_path = rel_path_normalized(&entry_path, &layer_dir);
            files.push(PlannedFile::new(entry_path, rel_path, None));
        }
    }

    Ok(files)
}

fn scan_directory<E>(
    layer_dir: &Utf8Path,
    dir_path: &Utf8Path,
    ignore: &ModIgnore,
    ignored: &mut Vec<Utf8PathBuf>,
    files: &mut Vec<PlannedFile>,
) -> Result<(), PackError<E>> {
    // read_dir entries always carry a final component.
    let dir_name = dir_path.file_name().expect("directory entry has a name");

    // Case-insensitive: `Aatrox.WAD.Client` is still a WAD directory.
    let is_wad = dir_name.to_ascii_lowercase().ends_with(".wad.client");
    let wad_name = is_wad.then(|| dir_name.to_string());

    // WAD content is addressed relative to the WAD directory (the WAD name
    // travels separately); other directories keep their name in the path.
    let strip_base = if is_wad { dir_path } else { layer_dir };

    let mut walk = ignore.walk(dir_path);
    for file in walk.by_ref() {
        let file_path = file?;
        let rel_path = rel_path_normalized(&file_path, strip_base);
        files.push(PlannedFile::new(file_path, rel_path, wad_name.clone()));
    }

    ignored.extend(walk.into_skipped());

    Ok(())
}

fn rel_path_normalized(path: &Utf8Path, base: &Utf8Path) -> String {
    path.strip_prefix(base)
        .expect("scanned path must sit under the scanned directory")
        .as_str()
        .replace('\\', "/")
}
