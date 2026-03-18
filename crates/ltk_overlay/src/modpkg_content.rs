//! Content provider for `.modpkg` archives.
//!
//! Reads mod content directly from a mounted [`Modpkg`] archive, providing
//! access to layer structure, WAD targets, and override file data without
//! extracting to disk.

use crate::content::{archive_fingerprint, ModContentProvider};
use crate::error::{Error, Result};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectAuthor, ModProjectLayer};
use ltk_modpkg::Modpkg;
use std::collections::HashSet;
use std::io::{Read, Seek};

/// Content provider that reads directly from a mounted `.modpkg` archive.
pub struct ModpkgContent<R: Read + Seek> {
    modpkg: Modpkg<R>,
    archive_path: Option<Utf8PathBuf>,
}

impl<R: Read + Seek> ModpkgContent<R> {
    pub fn new(modpkg: Modpkg<R>) -> Self {
        Self {
            modpkg,
            archive_path: None,
        }
    }

    /// Set the archive file path, enabling content fingerprinting for the metadata cache.
    pub fn with_archive_path(mut self, path: Utf8PathBuf) -> Self {
        self.archive_path = Some(path);
        self
    }
}

impl<R: Read + Seek + Send + Sync> ModContentProvider for ModpkgContent<R> {
    fn mod_project(&mut self) -> Result<ModProject> {
        let metadata = self
            .modpkg
            .load_metadata()
            .map_err(|e| Error::Other(format!("Failed to load modpkg metadata: {}", e)))?;

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
            tags: Vec::new(),
            champions: Vec::new(),
            maps: Vec::new(),
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
    ) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
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
                    Error::Other(format!(
                        "Failed to decompress modpkg chunk {:016x}: {}",
                        path_hash, e
                    ))
                })?;
            results.push((Utf8PathBuf::from(rel_path), bytes.into_vec()));
        }

        Ok(results)
    }

    fn read_wad_override_file(
        &mut self,
        layer: &str,
        wad_name: &str,
        rel_path: &Utf8Path,
    ) -> Result<Vec<u8>> {
        let layer_hash = ltk_modpkg::hash_layer_name(layer);
        let full_path =
            ltk_modpkg::utils::normalize_chunk_path(&format!("{}/{}", wad_name, rel_path));

        // Compute the path hash from the full WAD-relative path
        let path_hash = ltk_modpkg::utils::hash_chunk_name(&full_path);
        if self.modpkg.chunks.contains_key(&(path_hash, layer_hash)) {
            let bytes = self
                .modpkg
                .load_chunk_decompressed_by_hash(path_hash, layer_hash)
                .map_err(|e| {
                    Error::Other(format!(
                        "Failed to decompress modpkg chunk {:016x}: {}",
                        path_hash, e
                    ))
                })?;
            return Ok(bytes.into_vec());
        }

        Err(Error::Other(format!(
            "Override file not found in modpkg: {}/{}/{}",
            layer, wad_name, rel_path
        )))
    }

    fn read_raw_override_file(&mut self, _rel_path: &Utf8Path) -> Result<Vec<u8>> {
        // Modpkg format doesn't have raw overrides — all content is organized by WAD
        Err(Error::Other(
            "Modpkg format does not support raw overrides".to_string(),
        ))
    }

    fn content_fingerprint(&self) -> Result<Option<u64>> {
        match &self.archive_path {
            Some(path) => archive_fingerprint(path),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ModContentProvider;
    use ltk_modpkg::builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder};
    use ltk_modpkg::{Modpkg, ModpkgCompression};
    use std::io::{Cursor, Write};

    /// Build a modpkg in memory with chunks whose paths use backslashes
    /// (simulating a Windows-packed archive) and verify that the overlay
    /// content provider can discover WADs and read overrides correctly.
    #[test]
    fn list_layer_wads_with_backslash_paths() {
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    // Simulate a path produced by Windows glob
                    .with_path("Graves.wad.client\\data\\characters\\graves\\skin0.bin")
                    .unwrap()
                    .with_compression(ModpkgCompression::None)
                    .with_layer("base"),
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("Graves.wad.client\\assets\\texture.tex")
                    .unwrap()
                    .with_compression(ModpkgCompression::None)
                    .with_layer("base"),
            );

        builder
            .build_to_writer(&mut cursor, |_chunk, cursor| {
                cursor.write_all(&[0xAA; 10])?;
                Ok(())
            })
            .unwrap();

        cursor.set_position(0);
        let modpkg = Modpkg::mount_from_reader(cursor).unwrap();
        let mut content = ModpkgContent::new(modpkg);

        let wads = content.list_layer_wads("base").unwrap();
        assert_eq!(wads.len(), 1);
        assert_eq!(wads[0], "graves.wad.client");
    }

    #[test]
    fn read_wad_overrides_with_backslash_paths() {
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);
        let file_data = b"hello world";

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("Graves.wad.client\\data\\skin0.bin")
                    .unwrap()
                    .with_compression(ModpkgCompression::None)
                    .with_layer("base"),
            );

        builder
            .build_to_writer(&mut cursor, |_chunk, cursor| {
                cursor.write_all(file_data)?;
                Ok(())
            })
            .unwrap();

        cursor.set_position(0);
        let modpkg = Modpkg::mount_from_reader(cursor).unwrap();
        let mut content = ModpkgContent::new(modpkg);

        let overrides = content
            .read_wad_overrides("base", "graves.wad.client")
            .unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].0.as_str(), "data/skin0.bin");
        assert_eq!(overrides[0].1, file_data);
    }

    #[test]
    fn multi_layer_backslash_paths() {
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_layer(ModpkgLayerBuilder::new("loading-screen").with_priority(1))
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("Graves.wad.client\\data\\base_file.bin")
                    .unwrap()
                    .with_compression(ModpkgCompression::None)
                    .with_layer("base"),
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("Graves.wad.client\\data\\loading.bin")
                    .unwrap()
                    .with_compression(ModpkgCompression::None)
                    .with_layer("loading-screen"),
            );

        builder
            .build_to_writer(&mut cursor, |chunk, cursor| {
                if chunk.layer() == "base" {
                    cursor.write_all(b"base_data")?;
                } else {
                    cursor.write_all(b"loading_data")?;
                }
                Ok(())
            })
            .unwrap();

        cursor.set_position(0);
        let modpkg = Modpkg::mount_from_reader(cursor).unwrap();
        let mut content = ModpkgContent::new(modpkg);

        // Both layers should find the same WAD
        let base_wads = content.list_layer_wads("base").unwrap();
        assert_eq!(base_wads, vec!["graves.wad.client"]);

        let loading_wads = content.list_layer_wads("loading-screen").unwrap();
        assert_eq!(loading_wads, vec!["graves.wad.client"]);

        // Each layer returns only its own overrides
        let base_overrides = content
            .read_wad_overrides("base", "graves.wad.client")
            .unwrap();
        assert_eq!(base_overrides.len(), 1);
        assert_eq!(base_overrides[0].0.as_str(), "data/base_file.bin");
        assert_eq!(base_overrides[0].1, b"base_data");

        let loading_overrides = content
            .read_wad_overrides("loading-screen", "graves.wad.client")
            .unwrap();
        assert_eq!(loading_overrides.len(), 1);
        assert_eq!(loading_overrides[0].0.as_str(), "data/loading.bin");
        assert_eq!(loading_overrides[0].1, b"loading_data");
    }
}
