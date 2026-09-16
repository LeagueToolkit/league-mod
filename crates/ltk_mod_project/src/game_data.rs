//! Layer declaration loading and build-resource classification.

use std::{collections::HashSet, io::Cursor};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_game_data::{
    Declarations, Error, OverridePath, ReferencedInputs, Selector, MANIFEST_NAMES,
};

use crate::{ModIgnore, ModProjectLayer};

/// An override file a layer's declarations name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideFile {
    /// The layer-relative path the declarations name it by.
    pub path: OverridePath,
    /// The file on disk.
    pub source: Utf8PathBuf,
}

/// A layer's declaration result and classified input files.
pub struct LayerDeclarations {
    pub declarations: Result<Option<Declarations>, Error>,
    inputs: HashSet<Utf8PathBuf>,
    override_files: Vec<OverrideFile>,
}

impl LayerDeclarations {
    /// Whether a file is a declaration input rather than game content.
    pub fn is_declaration_input(&self, path: &Utf8Path) -> bool {
        if self.inputs.is_empty() {
            return false;
        }
        self.inputs.contains(path)
            || path
                .canonicalize_utf8()
                .is_ok_and(|p| self.inputs.contains(&p))
    }

    /// The override files of accepted declarations, one per distinct path in
    /// first-reference order. Empty for refused or absent declarations.
    pub fn override_files(&self) -> &[OverrideFile] {
        &self.override_files
    }
}

/// Loads declarations and classifies inputs for one layer.
/// Input classification remains available on declaration errors.
pub fn load_layer(project_root: &Utf8Path, layer: &str, ignore: &ModIgnore) -> LayerDeclarations {
    Loader::new(ModProjectLayer::content_path(project_root, layer), ignore).load()
}

/// One layer's declaration load: the manifest, its sources, and its override files, each
/// read through the layer's containment and ignore rules and recorded as an input.
struct Loader<'a> {
    root: Utf8PathBuf,
    ignore: &'a ModIgnore,
    inputs: HashSet<Utf8PathBuf>,
    override_files: Vec<OverrideFile>,
}

impl<'a> Loader<'a> {
    fn new(root: Utf8PathBuf, ignore: &'a ModIgnore) -> Self {
        Self {
            root,
            ignore,
            inputs: HashSet::new(),
            override_files: Vec::new(),
        }
    }

    fn load(mut self) -> LayerDeclarations {
        let declarations = self.declarations();
        if declarations.is_err() {
            self.override_files.clear();
        }
        LayerDeclarations {
            declarations,
            inputs: self.inputs,
            override_files: self.override_files,
        }
    }

    /// The manifests present in the layer, by candidate name order.
    fn manifests(&self) -> Vec<Utf8PathBuf> {
        MANIFEST_NAMES
            .iter()
            .map(|name| self.root.join(name))
            .filter(|path| path.try_exists().unwrap_or(true))
            .collect()
    }

    /// Records a layer-relative reference as an input, in its joined and canonical spellings.
    fn record_input(&mut self, directory: &Utf8Path, reference: &str) {
        let reference = Utf8Path::new(reference);
        if reference.is_absolute() {
            return;
        }
        let path = directory.join(reference);
        if let Ok(canonical) = path.canonicalize_utf8() {
            self.inputs.insert(canonical);
        }
        self.inputs.insert(path);
    }

    /// Records every source and override file the manifests reference, whether or not the
    /// declarations load.
    fn discover(&mut self, manifests: &[Utf8PathBuf]) {
        for manifest in manifests {
            let Ok(text) = std::fs::read_to_string(manifest) else {
                continue;
            };
            let Ok(inputs) = ReferencedInputs::discover(manifest.as_str(), &text) else {
                continue;
            };
            let root = self.root.clone();
            for reference in &inputs.overrides {
                self.record_input(&root, reference);
            }
            for source in &inputs.sources {
                self.record_input(&root, source);
                let source_path = root.join(source);
                let Ok(text) = std::fs::read_to_string(&source_path) else {
                    continue;
                };
                let Ok(inputs) = ReferencedInputs::discover(source, &text) else {
                    continue;
                };
                let directory = source_path.parent().unwrap_or(&root).to_owned();
                for reference in &inputs.overrides {
                    self.record_input(&directory, reference);
                }
            }
        }
    }

    /// The canonical path of a required input inside the layer, recorded as an input.
    ///
    /// # Errors
    ///
    /// The path does not resolve, escapes the layer, or is excluded by `.modignore`.
    fn required(
        &mut self,
        path: &Utf8Path,
        root_canonical: &Utf8Path,
    ) -> Result<Utf8PathBuf, Error> {
        self.inputs.insert(path.to_owned());
        let canonical = path
            .canonicalize_utf8()
            .map_err(|e| Error::new(path.as_str(), e))?;
        if !canonical.starts_with(root_canonical) {
            return Err(Error::new(
                path.as_str(),
                "declaration input escapes its layer",
            ));
        }
        self.inputs.insert(canonical.clone());
        if self.ignore.is_ignored(path, false) {
            return Err(Error::new(
                path.as_str(),
                "required declaration input is excluded by .modignore",
            ));
        }
        Ok(canonical)
    }

    fn read_text(&mut self, path: &Utf8Path, root_canonical: &Utf8Path) -> Result<String, Error> {
        self.required(path, root_canonical)?;
        std::fs::read_to_string(path).map_err(|e| Error::new(path.as_str(), e))
    }

    /// Reads and checks one override file and records it, once per distinct path.
    fn override_file(
        &mut self,
        path: &OverridePath,
        root_canonical: &Utf8Path,
    ) -> Result<(), Error> {
        if self.override_files.iter().any(|file| file.path == *path) {
            return Ok(());
        }
        let source = self.root.join(path.as_str());
        self.required(&source, root_canonical)?;
        let bytes = std::fs::read(&source).map_err(|e| Error::new(path.as_str(), e))?;
        ltk_meta::BinOverride::from_reader(&mut Cursor::new(bytes))
            .map_err(|e| Error::new(path.as_str(), format!("override file is not a PTCH: {e}")))?;
        self.override_files.push(OverrideFile {
            path: path.clone(),
            source,
        });
        Ok(())
    }

    fn declarations(&mut self) -> Result<Option<Declarations>, Error> {
        let manifests = self.manifests();
        self.inputs.extend(manifests.iter().cloned());
        self.discover(&manifests);
        if manifests.is_empty() {
            return Ok(None);
        }
        if manifests.len() != 1 {
            return Err(Error::new(
                self.root.as_str(),
                "multiple game-data manifests",
            ));
        }
        let root_canonical = self
            .root
            .canonicalize_utf8()
            .map_err(|e| Error::new(self.root.as_str(), e))?;
        let manifest = &manifests[0];
        let text = self.read_text(manifest, &root_canonical)?;
        let root = self.root.clone();
        let declarations =
            ltk_game_data::load_declarations(manifest.file_name().unwrap(), &text, |source| {
                let path = Utf8Path::new(source);
                if path.is_absolute() || source.contains('\\') {
                    return Err(Error::new(
                        source,
                        "source requires a layer-relative path with forward slashes",
                    ));
                }
                let resolved = root.join(path);
                let canonical = resolved
                    .canonicalize_utf8()
                    .map_err(|e| Error::new(source, e))?;
                if manifests
                    .iter()
                    .any(|manifest| manifest.canonicalize_utf8().ok().as_ref() == Some(&canonical))
                {
                    return Err(Error::new(source, "manifest and source roles conflict"));
                }
                self.read_text(&resolved, &root_canonical)
            })?;
        let mut seen = HashSet::new();
        for module in &declarations.modules {
            let Selector::Target { target, edits } = &module.selector else {
                continue;
            };
            if let Some(source) = &module.origin.source {
                let canonical = self
                    .root
                    .join(source)
                    .canonicalize_utf8()
                    .map_err(|e| Error::new(source, e))?;
                if !seen.insert((target.chunk_hash(), canonical)) {
                    return Err(Error::new(
                        source,
                        "duplicate canonical target/source assignment",
                    ));
                }
            }
            for edit in edits {
                for path in &edit.overrides {
                    self.override_file(path, &root_canonical)?;
                }
            }
        }
        Ok(Some(declarations))
    }
}

/// Reconstructs an archive's declarations as a layer manifest.
pub fn write_manifest(
    project_root: &Utf8Path,
    layer: &str,
    document: &ltk_game_data::DeclarationDocument,
) -> Result<(), Error> {
    if layer.is_empty() || layer.contains(['/', '\\']) || matches!(layer, "." | "..") {
        return Err(Error::new(layer, "invalid layer name"));
    }
    let text = document.parse()?.manifest_json()?;
    let dir = ModProjectLayer::content_path(project_root, layer);
    std::fs::create_dir_all(&dir).map_err(|e| Error::new(dir.as_str(), e))?;
    let path = dir.join("game_data.json");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| Error::new(path.as_str(), e))?;
    file.write_all(text.as_bytes())
        .map_err(|e| Error::new(path.as_str(), e))
}
