//! Mod content provider abstraction.
//!
//! This module defines the [`ModContentProvider`] trait that decouples the overlay
//! builder from any particular mod storage format. Implementations provide access to:
//!
//! - Mod project metadata (name, version, layers)
//! - WAD target names per layer
//! - Override file data for each WAD
//!
//! The crate ships [`FsModContent`] for reading from standard filesystem directories.
//! Archive-backed implementations (`.modpkg`, `.fantome`) live in the `ltk-manager`
//! crate where the archive format dependencies are available.

use crate::error::Result;
use camino::Utf8PathBuf;
use ltk_mod_project::ModProject;

/// Abstracts how mod content is accessed during overlay building.
///
/// Implementors provide access to mod project metadata, layer structure,
/// and WAD override data without prescribing how content is stored or read.
///
/// All mod WAD content is treated as **overlays** — individual file overrides
/// that get patched on top of the original game WADs. There is no concept of
/// full WAD replacement; every mod contributes individual chunks.
///
/// # Implementing
///
/// Implementations must be [`Send`] so the builder can be used across threads.
/// Methods take `&mut self` to allow stateful readers (e.g., seeking within an
/// archive).
///
/// The returned `Vec<(PathBuf, Vec<u8>)>` from [`read_wad_overrides`](Self::read_wad_overrides)
/// uses paths that are resolved to `u64` hashes by [`resolve_chunk_hash`](crate::utils::resolve_chunk_hash):
/// - **Named paths** (e.g., `data/characters/aatrox/skin0.bin`) are hashed via
///   [`ltk_modpkg::utils::hash_chunk_name`].
/// - **Hex-hash filenames** (e.g., `0123456789abcdef.bin`) are parsed directly as
///   `u64` values. This is used by packed WAD content where original paths are lost.
pub trait ModContentProvider: Send {
    /// Return the mod's project configuration.
    ///
    /// This provides the mod name, version, description, author list, and — most
    /// importantly — the layer definitions that control how overrides are applied.
    fn mod_project(&mut self) -> Result<ModProject>;

    /// List WAD targets that have override content in the given layer.
    ///
    /// Returns WAD filenames such as `"Aatrox.wad.client"` or `"Map11.wad.client"`.
    /// The builder uses these names to look up the corresponding game WAD via
    /// [`GameIndex::find_wad`](crate::game_index::GameIndex::find_wad).
    fn list_layer_wads(&mut self, layer: &str) -> Result<Vec<String>>;

    /// Read all override files for a WAD in a layer.
    ///
    /// Returns `(relative_path, file_bytes)` pairs. The relative path is the file's
    /// location *within* the WAD (e.g., `data/characters/aatrox/skin0.bin`), used to
    /// compute the chunk path hash. The bytes are the uncompressed file content that
    /// will replace the corresponding chunk in the game WAD.
    fn read_wad_overrides(
        &mut self,
        layer: &str,
        wad_name: &str,
    ) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>>;
}

/// Filesystem-backed mod content provider.
///
/// Reads mod content from a standard on-disk directory layout used during
/// mod development and by the `league-mod` CLI:
///
/// ```text
/// mod_dir/
///   mod.config.json              # Project metadata and layer definitions
///   content/
///     base/                      # Layer name (matches a layer in mod.config.json)
///       Aatrox.wad.client/       # WAD target directory
///         data/
///           characters/
///             aatrox/
///               skin0.bin        # Override file (path = chunk hash key)
///     high_res/                  # Optional additional layer
///       Aatrox.wad.client/
///         ...
/// ```
///
/// Only subdirectories under each layer whose name ends in `.wad.client`
/// (case-insensitive) are recognized as WAD targets.
pub struct FsModContent {
    mod_dir: Utf8PathBuf,
}

impl FsModContent {
    /// Create a new filesystem content provider rooted at the given mod directory.
    ///
    /// The directory must contain a `mod.config.json` and a `content/` subdirectory.
    pub fn new(mod_dir: Utf8PathBuf) -> Self {
        Self { mod_dir }
    }
}

impl ModContentProvider for FsModContent {
    fn mod_project(&mut self) -> Result<ModProject> {
        let config_path = self.mod_dir.join("mod.config.json");
        let contents = std::fs::read_to_string(config_path.as_std_path())?;
        Ok(serde_json::from_str(&contents)?)
    }

    fn list_layer_wads(&mut self, layer: &str) -> Result<Vec<String>> {
        let layer_dir = self.mod_dir.join("content").join(layer);
        if !layer_dir.as_std_path().exists() {
            return Ok(Vec::new());
        }

        let mut wads = Vec::new();
        for entry in std::fs::read_dir(layer_dir.as_std_path())? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".wad.client") {
                continue;
            }
            wads.push(name.to_string());
        }
        Ok(wads)
    }

    fn read_wad_overrides(
        &mut self,
        layer: &str,
        wad_name: &str,
    ) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        let wad_dir = self.mod_dir.join("content").join(layer).join(wad_name);
        let mut results = Vec::new();
        let mut stack = vec![wad_dir.clone()];

        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir.as_std_path())? {
                let entry = entry?;
                let path = entry.path();

                let utf8_path = match Utf8PathBuf::from_path_buf(path) {
                    Ok(p) => p,
                    Err(p) => {
                        tracing::warn!("Skipping non-UTF-8 path: {}", p.display());
                        continue;
                    }
                };

                if utf8_path.as_std_path().is_dir() {
                    stack.push(utf8_path);
                    continue;
                }

                let rel = utf8_path
                    .strip_prefix(&wad_dir)
                    .unwrap_or(&utf8_path)
                    .to_path_buf();
                let bytes = std::fs::read(utf8_path.as_std_path())?;
                results.push((rel, bytes));
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use std::fs;
    use tempfile::tempdir;

    fn create_test_mod_dir() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let mod_dir = dir.path();

        // Create mod.config.json
        let project = ltk_mod_project::ModProject {
            name: "test-mod".to_string(),
            display_name: "Test Mod".to_string(),
            version: "1.0.0".to_string(),
            description: "A test mod".to_string(),
            authors: vec![],
            license: None,
            tags: vec![],
            champions: vec![],
            maps: vec![],
            transformers: vec![],
            layers: ltk_mod_project::default_layers(),
            thumbnail: None,
        };
        fs::write(
            mod_dir.join("mod.config.json"),
            serde_json::to_string_pretty(&project).unwrap(),
        )
        .unwrap();

        // Create content/base/Test.wad.client/ with some files
        let wad_dir = mod_dir.join("content/base/Test.wad.client");
        fs::create_dir_all(&wad_dir).unwrap();
        fs::write(wad_dir.join("file1.bin"), b"data1").unwrap();

        let sub_dir = wad_dir.join("subdir");
        fs::create_dir_all(&sub_dir).unwrap();
        fs::write(sub_dir.join("file2.bin"), b"data2").unwrap();

        dir
    }

    #[test]
    fn test_fs_mod_project() {
        let dir = create_test_mod_dir();
        let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let mut provider = FsModContent::new(mod_dir);

        let project = provider.mod_project().unwrap();
        assert_eq!(project.name, "test-mod");
        assert_eq!(project.display_name, "Test Mod");
    }

    #[test]
    fn test_fs_list_layer_wads() {
        let dir = create_test_mod_dir();
        let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let mut provider = FsModContent::new(mod_dir);

        let wads = provider.list_layer_wads("base").unwrap();
        assert_eq!(wads.len(), 1);
        assert_eq!(wads[0], "Test.wad.client");
    }

    #[test]
    fn test_fs_list_layer_wads_missing_layer() {
        let dir = create_test_mod_dir();
        let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let mut provider = FsModContent::new(mod_dir);

        let wads = provider.list_layer_wads("nonexistent").unwrap();
        assert!(wads.is_empty());
    }

    #[test]
    fn test_fs_read_wad_overrides() {
        let dir = create_test_mod_dir();
        let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let mut provider = FsModContent::new(mod_dir);

        let overrides = provider
            .read_wad_overrides("base", "Test.wad.client")
            .unwrap();
        assert_eq!(overrides.len(), 2);

        // Check that both files are present (order may vary)
        let paths: Vec<String> = overrides
            .iter()
            .map(|(p, _)| p.as_str().replace('\\', "/"))
            .collect();
        assert!(paths.contains(&"file1.bin".to_string()));
        assert!(paths.contains(&"subdir/file2.bin".to_string()));
    }
}
