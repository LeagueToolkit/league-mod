//! Layer declaration loading and build-resource classification.

use std::collections::HashSet;

use camino::{Utf8Path, Utf8PathBuf};
use ltk_game_data::{Declarations, Error, MANIFEST_NAMES};

use crate::{ModIgnore, ModProjectLayer};

/// A layer's declaration result and classified input files.
pub struct LayerDeclarations {
    pub declarations: Result<Option<Declarations>, Error>,
    inputs: HashSet<Utf8PathBuf>,
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
}

/// Loads declarations and classifies inputs for one layer.
/// Input classification remains available on declaration errors.
pub fn load_layer(project_root: &Utf8Path, layer: &str, ignore: &ModIgnore) -> LayerDeclarations {
    let root = ModProjectLayer::content_path(project_root, layer);
    let mut inputs = HashSet::new();
    let declarations = load(&root, ignore, &mut inputs);
    LayerDeclarations {
        declarations,
        inputs,
    }
}

fn load(
    root: &Utf8Path,
    ignore: &ModIgnore,
    inputs: &mut HashSet<Utf8PathBuf>,
) -> Result<Option<Declarations>, Error> {
    let manifests: Vec<_> = MANIFEST_NAMES
        .iter()
        .map(|name| root.join(name))
        .filter(|path| path.try_exists().unwrap_or(true))
        .collect();
    inputs.extend(manifests.iter().cloned());
    for manifest in &manifests {
        if let Ok(text) = std::fs::read_to_string(manifest) {
            if let Ok(sources) = ltk_game_data::referenced_sources(manifest.as_str(), &text) {
                for source in sources {
                    let source = Utf8Path::new(&source);
                    if source.is_absolute() {
                        continue;
                    }
                    let path = root.join(source);
                    inputs.insert(path.clone());
                    if let Ok(canonical) = path.canonicalize_utf8() {
                        inputs.insert(canonical);
                    }
                }
            }
        }
    }
    if manifests.is_empty() {
        return Ok(None);
    }
    if manifests.len() != 1 {
        return Err(Error::new(root.as_str(), "multiple game-data manifests"));
    }
    let root_canonical = root
        .canonicalize_utf8()
        .map_err(|e| Error::new(root.as_str(), e))?;
    let read = |path: &Utf8Path, inputs: &mut HashSet<Utf8PathBuf>| -> Result<String, Error> {
        inputs.insert(path.to_owned());
        let canonical = path
            .canonicalize_utf8()
            .map_err(|e| Error::new(path.as_str(), e))?;
        if !canonical.starts_with(&root_canonical) {
            return Err(Error::new(
                path.as_str(),
                "declaration input escapes its layer",
            ));
        }
        inputs.insert(canonical);
        if ignore.is_ignored(path, false) {
            return Err(Error::new(
                path.as_str(),
                "required declaration input is excluded by .modignore",
            ));
        }
        std::fs::read_to_string(path).map_err(|e| Error::new(path.as_str(), e))
    };
    let manifest = &manifests[0];
    let text = read(manifest, inputs)?;
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
            read(&resolved, inputs)
        })?;
    let mut seen = HashSet::new();
    for module in &declarations.modules {
        if let (Some(source), ltk_game_data::Selector::Target { target, .. }) =
            (&module.origin.source, &module.selector)
        {
            let canonical = root
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
    }
    Ok(Some(declarations))
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
