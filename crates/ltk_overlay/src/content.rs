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

use crate::error::{Error, Result};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModIgnore, ModProject};
use xxhash_rust::xxh3::xxh3_64;

/// Compute a content fingerprint from an archive file's size and modification time.
///
/// This is a cheap way to detect when an archive has changed without reading its
/// contents. Suitable for immutable archive files (`.fantome`, `.modpkg`).
pub fn archive_fingerprint(path: &Utf8Path) -> Result<Option<u64>> {
    let meta = match std::fs::metadata(path.as_std_path()) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };

    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut buf = Vec::with_capacity(16);
    buf.extend_from_slice(&size.to_le_bytes());
    buf.extend_from_slice(&mtime.to_le_bytes());

    Ok(Some(xxh3_64(&buf)))
}

/// Abstracts how mod content is accessed during overlay building.
///
/// Implementors provide access to mod project metadata, layer structure,
/// and WAD override data without prescribing how content is stored or read.
///
/// All mod WAD content is treated as **overlays** - individual file overrides
/// that get patched on top of the original game WADs. There is no concept of
/// full WAD replacement; every mod contributes individual chunks.
///
/// # Implementing
///
/// Implementations must be [`Send`] + [`Sync`]:
///
/// - **`Send`** - the builder moves providers across threads.
/// - **`Sync`** - [`content_fingerprint`](Self::content_fingerprint) takes `&self`
///   and may be called concurrently via `par_iter()`. This method should only read
///   filesystem metadata (stat calls), so it should not require mutation.
///
/// Most content-reading methods still take `&mut self` to allow stateful readers
/// (e.g., seeking within a ZIP archive). If an implementation needs interior
/// mutability for `content_fingerprint`, it can use synchronization primitives
/// like `Mutex` or `RwLock`.
///
/// The returned `Vec<(PathBuf, Vec<u8>)>` from [`read_wad_overrides`](Self::read_wad_overrides)
/// uses paths that are resolved to `u64` hashes by [`resolve_chunk_hash`](crate::utils::resolve_chunk_hash):
/// - **Named paths** (e.g., `data/characters/aatrox/skin0.bin`) are hashed as a
///   [`ltk_modpkg::ChunkPath`].
/// - **Hex-hash filenames** (e.g., `0123456789abcdef.bin`) are parsed directly as
///   `u64` values. This is used by packed WAD content where original paths are lost.
pub trait ModContentProvider: Send + Sync {
    /// Return the mod's project configuration.
    ///
    /// This provides the mod name, version, description, author list, and - most
    /// importantly - the layer definitions that control how overrides are applied.
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

    /// Read all RAW override files from the mod.
    ///
    /// RAW overrides are files identified by their game asset path (e.g.,
    /// `assets/characters/aatrox/skin0.bin`) rather than being pre-organized
    /// into WAD target directories. These files are routed to the correct WADs
    /// at overlay build time using the GameIndex hash lookup.
    ///
    /// Returns `(relative_path, file_bytes)` pairs where the relative path is
    /// the game asset path used to compute the chunk path hash.
    ///
    /// The default implementation returns an empty list.
    fn read_raw_overrides(&mut self) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        Ok(Vec::new())
    }

    /// Compute a fingerprint that changes when any mod content changes.
    ///
    /// Used by the metadata cache to detect stale entries. Returns `None` if
    /// the provider cannot efficiently compute a fingerprint (cache will be
    /// skipped for this mod).
    ///
    /// Takes `&self` (not `&mut self`) because fingerprinting is a read-only
    /// operation and may be called in parallel across mods. Implementations
    /// should only inspect filesystem metadata (file sizes, modification times),
    /// not read file contents.
    ///
    /// For filesystem providers: hash of `(path, size, mtime)` tuples.
    /// For archive providers: archive file size + mtime.
    fn content_fingerprint(&self) -> Result<Option<u64>> {
        Ok(None)
    }

    /// Read a single override file from a WAD in a layer.
    ///
    /// Used in pass 2 to re-read only the bytes needed for WADs being rebuilt,
    /// rather than loading all overrides into memory at once.
    fn read_wad_override_file(
        &mut self,
        layer: &str,
        wad_name: &str,
        rel_path: &Utf8Path,
    ) -> Result<Vec<u8>>;

    /// Read a single raw override file by its relative path.
    ///
    /// Used in pass 2 to re-read only the bytes needed for WADs being rebuilt.
    fn read_raw_override_file(&mut self, rel_path: &Utf8Path) -> Result<Vec<u8>>;
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
///
/// `.modignore` files (at the mod directory root and nested inside
/// `content/`) filter the content exactly as packing does: files they
/// exclude are not injected into the overlay, so what an author tests is
/// what the package ships. The
/// [fingerprint](ModContentProvider::content_fingerprint) applies the same
/// filter, so edits to an ignored file do not invalidate the rebuild cache.
///
/// The filter is loaded once per provider and shared by every method, so
/// one build reads the ignore files (and walks the tree to find nested
/// ones) exactly once, and the fingerprint decides against the same filter
/// the reads use. Create a provider per build; a long-lived one would keep
/// serving the snapshot it loaded first and miss `.modignore` edits.
pub struct FsModContent {
    mod_dir: Utf8PathBuf,
    ignore: std::sync::OnceLock<ModIgnore>,
}

impl FsModContent {
    /// Create a new filesystem content provider rooted at the given mod directory.
    ///
    /// The directory must contain a `mod.config.json` and a `content/` subdirectory.
    pub fn new(mod_dir: Utf8PathBuf) -> Self {
        Self {
            mod_dir,
            ignore: std::sync::OnceLock::new(),
        }
    }

    /// The `.modignore` filter, loaded on first use and cached for the
    /// provider's lifetime. A load failure is returned each call rather
    /// than cached, so a fixed file is picked up on retry.
    fn ignore(&self) -> Result<&ModIgnore> {
        if let Some(ignore) = self.ignore.get() {
            return Ok(ignore);
        }

        let loaded = ModIgnore::load(&self.mod_dir)?;
        Ok(self.ignore.get_or_init(|| loaded))
    }
}

impl ModContentProvider for FsModContent {
    fn mod_project(&mut self) -> Result<ModProject> {
        let config_path = self.mod_dir.join("mod.config.json");
        let contents = std::fs::read_to_string(config_path.as_std_path())
            .map_err(|source| Error::read(&config_path, source))?;
        Ok(serde_json::from_str(&contents)?)
    }

    fn list_layer_wads(&mut self, layer: &str) -> Result<Vec<String>> {
        let layer_dir = self.mod_dir.join("content").join(layer);
        if !layer_dir.as_std_path().exists() {
            return Ok(Vec::new());
        }

        let ignore = self.ignore()?;

        let mut wads = Vec::new();
        let entries = std::fs::read_dir(layer_dir.as_std_path())
            .map_err(|source| Error::read(&layer_dir, source))?;
        for entry in entries {
            let entry = entry.map_err(|source| Error::read(&layer_dir, source))?;
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
            if ignore.is_ignored(&layer_dir.join(name), true) {
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
        let ignore = self.ignore()?;

        let mut results = Vec::new();
        for file in ignore.walk(&wad_dir) {
            let utf8_path = file.map_err(|error| {
                let (path, source) = error.into_parts();
                Error::Read { path, source }
            })?;

            let rel = utf8_path
                .strip_prefix(&wad_dir)
                .unwrap_or(&utf8_path)
                .as_str()
                .replace('\\', "/");
            let bytes = std::fs::read(utf8_path.as_std_path())
                .map_err(|source| Error::read(&utf8_path, source))?;
            results.push((Utf8PathBuf::from(rel), bytes));
        }
        Ok(results)
    }

    fn content_fingerprint(&self) -> Result<Option<u64>> {
        use xxhash_rust::xxh3::xxh3_64;

        fn mtime_secs(meta: &std::fs::Metadata) -> u64 {
            meta.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0)
        }

        // Collect (path, size, mtime) for all files under content/, plus the
        // project config - string overrides live in mod.config.json/.toml, so
        // config edits must change the fingerprint even when content/ doesn't.
        let mut entries: Vec<(String, u64, u64)> = Vec::new();

        for config_name in ["mod.config.json", "mod.config.toml"] {
            let config_path = self.mod_dir.join(config_name);
            let Ok(meta) = std::fs::metadata(config_path.as_std_path()) else {
                continue;
            };
            entries.push((config_name.to_string(), meta.len(), mtime_secs(&meta)));
        }

        // The same filter as `read_wad_overrides`, or an edit to an ignored
        // file would invalidate the rebuild cache for no reason.
        let ignore = self.ignore()?;

        // Every ignore file shapes which content is read, so each one is
        // part of the cache key: the walk never yields them, and an edited,
        // added, or deleted `.modignore` must trigger a rebuild.
        for path in ignore.source_files() {
            let Ok(meta) = std::fs::metadata(path.as_std_path()) else {
                continue;
            };
            let rel = path
                .strip_prefix(&self.mod_dir)
                .unwrap_or(path)
                .as_str()
                .replace('\\', "/");
            entries.push((rel, meta.len(), mtime_secs(&meta)));
        }

        let content_dir = self.mod_dir.join("content");
        if !content_dir.as_std_path().exists() && entries.is_empty() {
            return Ok(Some(0));
        }

        if content_dir.as_std_path().exists() {
            for file in ignore.walk(&content_dir) {
                // Fingerprinting is opportunistic: an unreadable directory is
                // skipped here, as before, and surfaces on the read path.
                let Ok(utf8_path) = file else {
                    continue;
                };

                let meta = std::fs::metadata(utf8_path.as_std_path())
                    .map_err(|source| Error::read(&utf8_path, source))?;

                let rel = utf8_path
                    .strip_prefix(&self.mod_dir)
                    .unwrap_or(&utf8_path)
                    .as_str()
                    .replace('\\', "/");
                entries.push((rel, meta.len(), mtime_secs(&meta)));
            }
        }

        entries.sort();

        let mut buf = Vec::with_capacity(entries.len() * 32);
        for (path, size, mtime) in &entries {
            buf.extend_from_slice(path.as_bytes());
            buf.extend_from_slice(&size.to_le_bytes());
            buf.extend_from_slice(&mtime.to_le_bytes());
        }

        Ok(Some(xxh3_64(&buf)))
    }

    fn read_wad_override_file(
        &mut self,
        layer: &str,
        wad_name: &str,
        rel_path: &Utf8Path,
    ) -> Result<Vec<u8>> {
        let file_path = self
            .mod_dir
            .join("content")
            .join(layer)
            .join(wad_name)
            .join(rel_path);
        std::fs::read(file_path.as_std_path()).map_err(|source| Error::read(&file_path, source))
    }

    fn read_raw_override_file(&mut self, rel_path: &Utf8Path) -> Result<Vec<u8>> {
        let file_path = self.mod_dir.join("content").join(rel_path);
        std::fs::read(file_path.as_std_path()).map_err(|source| Error::read(&file_path, source))
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

    #[test]
    fn test_modignore_filters_wad_overrides() {
        let dir = create_test_mod_dir();
        let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

        fs::write(
            mod_dir.join("content/base/Test.wad.client/source.psd"),
            b"working file",
        )
        .unwrap();
        fs::write(mod_dir.join(".modignore"), "*.psd\n").unwrap();

        let mut provider = FsModContent::new(mod_dir);
        let overrides = provider
            .read_wad_overrides("base", "Test.wad.client")
            .unwrap();

        let paths: Vec<String> = overrides
            .iter()
            .map(|(p, _)| p.as_str().replace('\\', "/"))
            .collect();
        assert!(paths.contains(&"file1.bin".to_string()));
        assert!(paths.contains(&"subdir/file2.bin".to_string()));
        assert!(!paths.contains(&"source.psd".to_string()));
    }

    #[test]
    fn test_modignore_hides_a_fully_ignored_wad() {
        let dir = create_test_mod_dir();
        let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

        fs::write(mod_dir.join(".modignore"), "Test.wad.client/\n").unwrap();

        let mut provider = FsModContent::new(mod_dir);
        assert!(provider.list_layer_wads("base").unwrap().is_empty());
        assert!(
            provider
                .read_wad_overrides("base", "Test.wad.client")
                .unwrap()
                .is_empty()
        );
    }

    /// Editing an ignored file must not invalidate the rebuild cache: the
    /// fingerprint uses the same filter as the read.
    #[test]
    fn test_fingerprint_is_stable_across_ignored_file_edits() {
        let dir = create_test_mod_dir();
        let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

        let psd = mod_dir.join("content/base/Test.wad.client/source.psd");
        fs::write(&psd, b"v1").unwrap();
        fs::write(mod_dir.join(".modignore"), "*.psd\n").unwrap();

        let provider = FsModContent::new(mod_dir);
        let before = provider.content_fingerprint().unwrap();

        // A different size, so this does not depend on mtime granularity.
        fs::write(&psd, b"version two, considerably larger").unwrap();
        let after = provider.content_fingerprint().unwrap();

        assert_eq!(before, after);
    }

    #[test]
    fn test_fingerprint_changes_when_modignore_changes() {
        let dir = create_test_mod_dir();
        let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

        // One provider per build: the filter is a per-provider snapshot.
        let before = FsModContent::new(mod_dir.clone())
            .content_fingerprint()
            .unwrap();

        fs::write(mod_dir.join(".modignore"), "*.psd\n").unwrap();
        let after = FsModContent::new(mod_dir.clone())
            .content_fingerprint()
            .unwrap();

        assert_ne!(before, after);
    }

    #[test]
    fn test_nested_modignore_filters_wad_overrides() {
        let dir = create_test_mod_dir();
        let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

        fs::write(
            mod_dir.join("content/base/Test.wad.client/source.psd"),
            b"working file",
        )
        .unwrap();
        fs::write(
            mod_dir.join("content/base/Test.wad.client/.modignore"),
            "*.psd\n",
        )
        .unwrap();

        let mut provider = FsModContent::new(mod_dir);
        let overrides = provider
            .read_wad_overrides("base", "Test.wad.client")
            .unwrap();

        let paths: Vec<String> = overrides
            .iter()
            .map(|(p, _)| p.as_str().replace('\\', "/"))
            .collect();
        assert!(paths.contains(&"file1.bin".to_string()));
        assert!(!paths.contains(&"source.psd".to_string()));
        // The ignore file itself is filter metadata, not an override.
        assert!(!paths.contains(&".modignore".to_string()));
    }

    #[test]
    fn test_fingerprint_changes_when_a_nested_modignore_appears() {
        let dir = create_test_mod_dir();
        let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

        let before = FsModContent::new(mod_dir.clone())
            .content_fingerprint()
            .unwrap();

        // The walk never yields ignore files, so without explicit statting
        // this edit would be invisible to the cache.
        fs::write(
            mod_dir.join("content/base/Test.wad.client/.modignore"),
            "*.psd\n",
        )
        .unwrap();
        let after = FsModContent::new(mod_dir.clone())
            .content_fingerprint()
            .unwrap();

        assert_ne!(before, after);
    }
}
