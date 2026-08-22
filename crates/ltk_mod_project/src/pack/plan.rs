//! [`PackPlan`]: the driver's resolved view of what a pack will write.

use camino::{Utf8Path, Utf8PathBuf};

use crate::{ModProject, ModProjectLayer};

/// Everything a `PackFormat` needs to write an archive: the project's
/// metadata, its content files after `.modignore` filtering, and the resolved
/// metadata file paths.
///
/// Built by `ProjectPacker`; formats only read it.
#[derive(Debug, Clone, PartialEq)]
pub struct PackPlan<'a> {
    project: &'a ModProject,
    project_root: &'a Utf8Path,
    layers: Vec<PlannedLayer>,
    readme: Option<Utf8PathBuf>,
    license: Option<PlannedLicense>,
    thumbnail: Option<Utf8PathBuf>,
}

impl<'a> PackPlan<'a> {
    pub(crate) fn new(
        project: &'a ModProject,
        project_root: &'a Utf8Path,
        layers: Vec<PlannedLayer>,
        readme: Option<Utf8PathBuf>,
        license: Option<PlannedLicense>,
        thumbnail: Option<Utf8PathBuf>,
    ) -> Self {
        Self {
            project,
            project_root,
            layers,
            readme,
            license,
            thumbnail,
        }
    }

    /// The project being packed.
    pub fn project(&self) -> &'a ModProject {
        self.project
    }

    /// The project's root directory.
    pub fn project_root(&self) -> &'a Utf8Path {
        self.project_root
    }

    /// The layers to pack, base layer first, with their content files.
    pub fn layers(&self) -> &[PlannedLayer] {
        &self.layers
    }

    /// The base layer.
    ///
    /// Every plan has one: the driver synthesizes the base layer when the
    /// config omits it.
    pub fn base_layer(&self) -> &PlannedLayer {
        self.layers
            .iter()
            .find(|layer| layer.layer().is_base())
            .expect("every plan carries a base layer")
    }

    /// The project's `README.md`, if it exists.
    pub fn readme(&self) -> Option<&Utf8Path> {
        self.readme.as_deref()
    }

    /// The project's license file, if it ships one.
    pub fn license(&self) -> Option<&PlannedLicense> {
        self.license.as_ref()
    }

    /// The project's thumbnail image, if it exists: the configured path, or
    /// `thumbnail.webp` at the project root when none is configured.
    pub fn thumbnail(&self) -> Option<&Utf8Path> {
        self.thumbnail.as_deref()
    }
}

/// A layer and the content files scanned for it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedLayer {
    layer: ModProjectLayer,
    files: Vec<PlannedFile>,
}

impl PlannedLayer {
    pub(crate) fn new(layer: ModProjectLayer, files: Vec<PlannedFile>) -> Self {
        Self { layer, files }
    }

    /// The layer's configuration. For an unconfigured base layer this is the
    /// synthesized default.
    pub fn layer(&self) -> &ModProjectLayer {
        &self.layer
    }

    /// The layer's content files after `.modignore` filtering.
    pub fn files(&self) -> &[PlannedFile] {
        &self.files
    }
}

/// A single content file a pack will write.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedFile {
    source: Utf8PathBuf,
    rel_path: String,
    wad: Option<String>,
}

impl PlannedFile {
    pub(crate) fn new(source: Utf8PathBuf, rel_path: String, wad: Option<String>) -> Self {
        Self {
            source,
            rel_path,
            wad,
        }
    }

    /// Absolute path of the file on disk.
    pub fn source(&self) -> &Utf8Path {
        &self.source
    }

    /// The file's path inside the archive, with `/` separators: relative to
    /// its WAD directory when [`wad`](Self::wad) is set, relative to the
    /// layer directory otherwise.
    pub fn rel_path(&self) -> &str {
        &self.rel_path
    }

    /// The `.wad.client` directory the file belongs to, in the author's
    /// spelling, if it sits inside one.
    pub fn wad(&self) -> Option<&str> {
        self.wad.as_deref()
    }
}

/// The project's license file, resolved for packing.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedLicense {
    source: Utf8PathBuf,
    canonical_name: &'static str,
}

impl PlannedLicense {
    pub(crate) fn new(source: Utf8PathBuf, canonical_name: &'static str) -> Self {
        Self {
            source,
            canonical_name,
        }
    }

    /// Absolute path of the license file on disk.
    pub fn source(&self) -> &Utf8Path {
        &self.source
    }

    /// The canonical spelling of the file's name (`license.txt` on disk is
    /// `LICENSE.txt` here), so archive entries do not drift with the casing
    /// the author happened to type.
    pub fn canonical_name(&self) -> &'static str {
        self.canonical_name
    }
}
