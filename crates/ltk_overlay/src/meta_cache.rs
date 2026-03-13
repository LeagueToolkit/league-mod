//! Persistent metadata cache for the two-pass overlay builder.
//!
//! Stores lightweight override metadata (hashes, sizes, source locations) from
//! pass 1 so that subsequent builds can skip re-reading unchanged mods entirely.
//! The cache is invalidated per-mod based on [`content_fingerprint`](crate::content::ModContentProvider::content_fingerprint).

use crate::builder::{OverrideMeta, OverrideSource};
use crate::error::{Error, Result};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::HashMap;

/// Current cache format version. Bump when the serialized format changes.
const CACHE_VERSION: u32 = 2;

/// Serializable cache entry for a single override.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CachedOverride {
    /// xxHash3 path hash (the WAD chunk key).
    pub path_hash: u64,
    /// xxHash3 of the uncompressed override bytes.
    pub content_hash: u64,
    /// Size of the uncompressed override bytes.
    pub uncompressed_size: usize,
    /// Relative WAD path this override targets (if known from directory structure).
    /// `None` for raw overrides that are routed by hash matching only.
    pub target_wad: Option<String>,
    /// Source layer name. `None` for raw overrides.
    pub source_layer: Option<String>,
    /// Source WAD directory name. `None` for raw overrides.
    pub source_wad_name: Option<String>,
    /// Relative file path within the WAD dir or raw content dir.
    pub source_rel_path: String,
}

/// Cached metadata for a single mod.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CachedModMeta {
    /// Fingerprint that was current when this cache entry was written.
    pub content_fingerprint: u64,
    /// All overrides from this mod.
    pub overrides: Vec<CachedOverride>,
}

impl CachedModMeta {
    /// Reconstruct `OverrideMeta` entries from cached data.
    ///
    /// Converts each [`CachedOverride`] back into the builder's in-memory
    /// representation, restoring source locations for pass 2 re-reading.
    pub fn reconstruct(&self, mod_id: &str) -> HashMap<u64, OverrideMeta> {
        let mut mod_meta: HashMap<u64, OverrideMeta> = HashMap::new();

        for entry in &self.overrides {
            let source = if let (Some(layer), Some(wad_name)) =
                (&entry.source_layer, &entry.source_wad_name)
            {
                OverrideSource::LayerWad {
                    mod_id: mod_id.to_string(),
                    layer: layer.clone(),
                    wad_name: wad_name.clone(),
                    rel_path: Utf8PathBuf::from(&entry.source_rel_path),
                }
            } else {
                OverrideSource::Raw {
                    mod_id: mod_id.to_string(),
                    rel_path: Utf8PathBuf::from(&entry.source_rel_path),
                }
            };

            mod_meta.insert(
                entry.path_hash,
                OverrideMeta {
                    content_hash: entry.content_hash,
                    uncompressed_size: entry.uncompressed_size,
                    source,
                    fallback_wad: entry.target_wad.as_ref().map(Utf8PathBuf::from),
                },
            );
        }

        mod_meta
    }

    /// Build a `CachedModMeta` from freshly-collected override metadata.
    ///
    /// This is the inverse of [`reconstruct`](Self::reconstruct): it converts
    /// the builder's in-memory representation into the serializable cache format.
    pub fn from_override_meta(fingerprint: u64, mod_meta: &HashMap<u64, OverrideMeta>) -> Self {
        let overrides = mod_meta
            .iter()
            .map(|(&path_hash, meta)| {
                let (source_layer, source_wad_name, source_rel_path) = match &meta.source {
                    OverrideSource::LayerWad {
                        layer,
                        wad_name,
                        rel_path,
                        ..
                    } => (
                        Some(layer.clone()),
                        Some(wad_name.clone()),
                        rel_path.as_str().to_string(),
                    ),
                    OverrideSource::Raw { rel_path, .. } => {
                        (None, None, rel_path.as_str().to_string())
                    }
                };

                CachedOverride {
                    path_hash,
                    content_hash: meta.content_hash,
                    uncompressed_size: meta.uncompressed_size,
                    target_wad: meta.fallback_wad.as_ref().map(|p| p.as_str().to_string()),
                    source_layer,
                    source_wad_name,
                    source_rel_path,
                }
            })
            .collect();

        Self {
            content_fingerprint: fingerprint,
            overrides,
        }
    }
}

/// Full metadata cache, persisted as MessagePack.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OverrideMetaCache {
    /// Format version for forward compatibility.
    version: u32,
    /// Game fingerprint at the time this cache was written.
    /// The cache is invalidated when the game is patched (fingerprint changes),
    /// because cached `target_wad` paths may reference a stale game layout.
    game_fingerprint: u64,
    /// Per-mod cached metadata, keyed by mod ID.
    pub mods: BTreeMap<String, CachedModMeta>,
}

impl OverrideMetaCache {
    /// Create a new empty cache.
    pub fn new(game_fingerprint: u64) -> Self {
        Self {
            version: CACHE_VERSION,
            game_fingerprint,
            mods: BTreeMap::new(),
        }
    }

    /// Load cache from disk. Returns `None` if the file doesn't exist, is
    /// invalid/stale (wrong version), or was built against a different game
    /// version (fingerprint mismatch).
    pub fn load(path: &Utf8Path, game_fingerprint: u64) -> Option<Self> {
        if !path.as_std_path().exists() {
            return None;
        }

        let bytes = std::fs::read(path.as_std_path()).ok()?;
        let cache: Self = rmp_serde::from_slice(&bytes).ok()?;

        if cache.version != CACHE_VERSION {
            tracing::info!(
                "Override meta cache version mismatch ({} != {}), ignoring",
                cache.version,
                CACHE_VERSION
            );
            return None;
        }

        if cache.game_fingerprint != game_fingerprint {
            tracing::info!(
                "Override meta cache game fingerprint mismatch ({:016x} != {:016x}), invalidating",
                cache.game_fingerprint,
                game_fingerprint
            );
            return None;
        }

        Some(cache)
    }

    /// Save cache to disk.
    pub fn save(&self, path: &Utf8Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent.as_std_path())?;
        }

        let bytes = rmp_serde::to_vec_named(self)
            .map_err(|e| Error::Other(format!("Failed to serialize override meta cache: {}", e)))?;
        std::fs::write(path.as_std_path(), bytes)?;

        tracing::debug!("Override meta cache saved to {}", path);
        Ok(())
    }

    /// Check if a mod's cached metadata is fresh (fingerprint matches).
    /// Returns the cached data if valid, `None` if stale or missing.
    pub fn get_mod_meta(&self, mod_id: &str, fingerprint: u64) -> Option<&CachedModMeta> {
        let entry = self.mods.get(mod_id)?;
        if entry.content_fingerprint == fingerprint {
            Some(entry)
        } else {
            None
        }
    }

    /// Insert or update a mod's cached metadata.
    pub fn set_mod_meta(&mut self, mod_id: String, meta: CachedModMeta) {
        self.mods.insert(mod_id, meta);
    }

    /// Remove mods from the cache that are no longer in the enabled list.
    pub fn retain_mods(&mut self, mod_ids: &[String]) {
        let id_set: std::collections::HashSet<&str> = mod_ids.iter().map(|s| s.as_str()).collect();
        self.mods.retain(|k, _| id_set.contains(k.as_str()));
    }
}

impl Default for OverrideMetaCache {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_cache_roundtrip() {
        let temp = NamedTempFile::new().unwrap();
        let path = Utf8Path::from_path(temp.path()).unwrap();

        let mut cache = OverrideMetaCache::new(0xABCD);
        cache.set_mod_meta(
            "test-mod".to_string(),
            CachedModMeta {
                content_fingerprint: 0xDEAD,
                overrides: vec![CachedOverride {
                    path_hash: 0x1234,
                    content_hash: 0x5678,
                    uncompressed_size: 100,
                    target_wad: Some("DATA/FINAL/test.wad.client".to_string()),
                    source_layer: Some("base".to_string()),
                    source_wad_name: Some("Test.wad.client".to_string()),
                    source_rel_path: "data/file.bin".to_string(),
                }],
            },
        );

        cache.save(path).unwrap();

        let loaded = OverrideMetaCache::load(path, 0xABCD).unwrap();
        assert_eq!(loaded.mods.len(), 1);

        let meta = loaded.get_mod_meta("test-mod", 0xDEAD).unwrap();
        assert_eq!(meta.overrides.len(), 1);
        assert_eq!(meta.overrides[0].path_hash, 0x1234);
    }

    #[test]
    fn test_cache_stale_fingerprint() {
        let mut cache = OverrideMetaCache::new(0xABCD);
        cache.set_mod_meta(
            "test-mod".to_string(),
            CachedModMeta {
                content_fingerprint: 0xDEAD,
                overrides: Vec::new(),
            },
        );

        // Same fingerprint -> hit
        assert!(cache.get_mod_meta("test-mod", 0xDEAD).is_some());
        // Different fingerprint -> miss
        assert!(cache.get_mod_meta("test-mod", 0xBEEF).is_none());
        // Missing mod -> miss
        assert!(cache.get_mod_meta("other-mod", 0xDEAD).is_none());
    }

    #[test]
    fn test_retain_mods() {
        let mut cache = OverrideMetaCache::new(0xABCD);
        cache.set_mod_meta(
            "mod-a".to_string(),
            CachedModMeta {
                content_fingerprint: 1,
                overrides: Vec::new(),
            },
        );
        cache.set_mod_meta(
            "mod-b".to_string(),
            CachedModMeta {
                content_fingerprint: 2,
                overrides: Vec::new(),
            },
        );

        cache.retain_mods(&["mod-a".to_string()]);
        assert!(cache.mods.contains_key("mod-a"));
        assert!(!cache.mods.contains_key("mod-b"));
    }

    #[test]
    fn test_load_nonexistent() {
        let result = OverrideMetaCache::load(Utf8Path::new("/nonexistent/path.bin"), 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_invalidated_by_game_fingerprint() {
        let temp = NamedTempFile::new().unwrap();
        let path = Utf8Path::from_path(temp.path()).unwrap();

        let mut cache = OverrideMetaCache::new(0x1111);
        cache.set_mod_meta(
            "test-mod".to_string(),
            CachedModMeta {
                content_fingerprint: 0xDEAD,
                overrides: Vec::new(),
            },
        );
        cache.save(path).unwrap();

        // Same game fingerprint -> loaded
        assert!(OverrideMetaCache::load(path, 0x1111).is_some());
        // Different game fingerprint -> invalidated
        assert!(OverrideMetaCache::load(path, 0x2222).is_none());
    }
}
