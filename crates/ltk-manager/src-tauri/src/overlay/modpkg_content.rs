use ltk_mod_project::{ModProject, ModProjectAuthor, ModProjectLayer};
use ltk_modpkg::Modpkg;
use ltk_overlay::content::ModContentProvider;
use ltk_overlay::error::Result;
use std::collections::HashSet;
use std::io::{Read, Seek};
use std::path::PathBuf;

/// Content provider that reads directly from a mounted `.modpkg` archive.
pub struct ModpkgContent<R: Read + Seek> {
    modpkg: Modpkg<R>,
}

impl<R: Read + Seek> ModpkgContent<R> {
    pub fn new(modpkg: Modpkg<R>) -> Self {
        Self { modpkg }
    }
}

impl<R: Read + Seek + Send> ModContentProvider for ModpkgContent<R> {
    fn mod_project(&mut self) -> Result<ModProject> {
        let metadata = self.modpkg.load_metadata().map_err(|e| {
            ltk_overlay::Error::Other(format!("Failed to load modpkg metadata: {}", e))
        })?;

        let mut layers: Vec<ModProjectLayer> = self
            .modpkg
            .layers
            .values()
            .map(|l| {
                let meta_layer = metadata.layers.iter().find(|ml| ml.name == l.name);
                ModProjectLayer {
                    name: l.name.clone(),
                    priority: l.priority,
                    description: meta_layer.and_then(|ml| ml.description.clone()),
                    string_overrides: meta_layer
                        .map(|ml| ml.string_overrides.clone())
                        .unwrap_or_default(),
                }
            })
            .collect();
        layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

        if !layers.iter().any(|l| l.name == "base") {
            layers.insert(0, ModProjectLayer::base());
        }

        Ok(ModProject {
            name: metadata.name,
            display_name: metadata.display_name,
            version: metadata.version.to_string(),
            description: metadata.description.unwrap_or_default(),
            authors: metadata
                .authors
                .into_iter()
                .map(|a| ModProjectAuthor::Name(a.name))
                .collect(),
            license: None,
            transformers: Vec::new(),
            layers,
            thumbnail: None,
        })
    }

    fn list_layer_wads(&mut self, layer: &str) -> Result<Vec<String>> {
        let layer_hash = ltk_modpkg::hash_layer_name(layer);

        let mut wad_names: HashSet<String> = HashSet::new();

        for &(path_hash, chunk_layer_hash) in self.modpkg.chunks.keys() {
            if chunk_layer_hash != layer_hash {
                continue;
            }

            let path = match self.modpkg.chunk_paths.get(&path_hash) {
                Some(p) => p,
                None => continue,
            };
            if path.starts_with("_meta_/") {
                continue;
            }

            // The WAD name is the first path component (e.g., "Aatrox.wad.client/data/...")
            if let Some(wad_name) = path.split('/').next() {
                if wad_name.to_ascii_lowercase().ends_with(".wad.client") {
                    wad_names.insert(wad_name.to_string());
                }
            }
        }

        Ok(wad_names.into_iter().collect())
    }

    fn read_wad_overrides(
        &mut self,
        layer: &str,
        wad_name: &str,
    ) -> Result<Vec<(PathBuf, Vec<u8>)>> {
        let layer_hash = ltk_modpkg::hash_layer_name(layer);
        let wad_prefix = format!("{}/", wad_name);

        // Collect (path_hash, layer_hash, relative_path) for matching chunks
        let matching: Vec<(u64, u64, String)> = self
            .modpkg
            .chunks
            .keys()
            .filter_map(|&(path_hash, chunk_layer_hash)| {
                if chunk_layer_hash != layer_hash {
                    return None;
                }
                let path = self.modpkg.chunk_paths.get(&path_hash)?;
                if path.starts_with("_meta_/") {
                    return None;
                }
                let rel = path.strip_prefix(&wad_prefix)?;
                Some((path_hash, chunk_layer_hash, rel.to_string()))
            })
            .collect();

        let mut results = Vec::with_capacity(matching.len());
        for (path_hash, layer_hash, rel_path) in matching {
            let bytes = self
                .modpkg
                .load_chunk_decompressed_by_hash(path_hash, layer_hash)
                .map_err(|e| {
                    ltk_overlay::Error::Other(format!(
                        "Failed to decompress modpkg chunk {:016x}: {}",
                        path_hash, e
                    ))
                })?;
            results.push((PathBuf::from(rel_path), bytes.into_vec()));
        }

        Ok(results)
    }
}
