//! Game file indexing for WAD and chunk lookup.
//!
//! The [`GameIndex`] is built once at the start of every overlay build by scanning
//! all `.wad.client` files under the game's `DATA/FINAL` directory. It provides two
//! kinds of lookups that the builder relies on:
//!
//! 1. **Filename lookup** ([`find_wad`](GameIndex::find_wad)) — Resolve a WAD name
//!    like `"Aatrox.wad.client"` (as listed by a mod) to its full filesystem path.
//!    The lookup is case-insensitive. If the same filename appears in multiple
//!    locations, an [`AmbiguousWad`](crate::Error::AmbiguousWad) error is returned.
//!
//! 2. **Hash lookup** ([`find_wads_with_hash`](GameIndex::find_wads_with_hash)) —
//!    Given a chunk path hash (`u64`), return *all* WAD files that contain a chunk
//!    with that hash. This powers cross-WAD matching: a single mod override can be
//!    distributed to every game WAD that shares the same asset.
//!
//! A **game fingerprint** is also computed from the file sizes and modification times
//! of all WADs. This fingerprint is persisted in [`OverlayState`](crate::state::OverlayState)
//! and used to detect game patches that invalidate the overlay.
//!
//! The index can be cached to disk as MessagePack via [`save`](GameIndex::save) /
//! [`load_or_build`](GameIndex::load_or_build) to avoid re-mounting every WAD on
//! subsequent builds when the game hasn't been patched.

use crate::error::{Error, Result};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use walkdir::WalkDir;

/// Version tag for the cache format.
const CACHE_VERSION: u32 = 3;

/// Serializable representation of a [`GameIndex`] for disk caching.
///
/// Uses MessagePack (via `rmp-serde`) for fast binary serialization with native
/// `u64` keys — no hex encoding needed.
#[derive(Serialize, Deserialize)]
struct GameIndexCache {
    version: u32,
    game_fingerprint: u64,
    /// WAD filename (lowercased) -> full filesystem paths.
    wad_index: HashMap<String, Vec<Utf8PathBuf>>,
    /// Chunk path hash -> WAD relative paths.
    hash_index: HashMap<u64, Vec<Utf8PathBuf>>,
    subchunktoc_blocked: Vec<u64>,
}

/// Index of all WAD files in a League of Legends game directory.
///
/// Built by scanning `DATA/FINAL` and mounting every `.wad.client` file to
/// record its chunk hashes. The index can be cached to disk to skip the
/// expensive WAD-mounting step on subsequent builds when the game hasn't changed.
pub struct GameIndex {
    /// WAD filename (lowercased) -> full filesystem paths.
    ///
    /// Most filenames map to a single path, but the structure supports duplicates
    /// so we can error on ambiguity rather than silently picking one.
    wad_index: HashMap<String, Vec<Utf8PathBuf>>,

    /// Chunk path hash -> WAD file paths (relative to game dir) that contain it.
    ///
    /// This is the core data structure for cross-WAD matching. A typical League
    /// installation has ~500k unique chunk hashes across ~200 WAD files.
    hash_index: HashMap<u64, Vec<Utf8PathBuf>>,

    /// Fingerprint derived from WAD file sizes and modification times.
    ///
    /// Used to detect game patches — if the fingerprint changes between builds,
    /// the overlay must be fully rebuilt even if the enabled mod list hasn't changed.
    game_fingerprint: u64,

    /// Path hashes for SubChunkTOC entries that mods must not override.
    ///
    /// For each `.wad.client` file, the corresponding `.wad.SubChunkTOC` path is
    /// computed and hashed. Mod overrides matching these hashes are stripped during
    /// the build to prevent mods from corrupting the game's sub-chunk loading.
    subchunktoc_blocked: HashSet<u64>,
}

impl GameIndex {
    /// Build a game index from the specified game directory.
    ///
    /// This scans all .wad.client files under `DATA/FINAL` and builds both indexes.
    ///
    /// # Arguments
    ///
    /// * `game_dir` - Path to the League of Legends Game directory
    pub fn build(game_dir: &Utf8Path) -> Result<Self> {
        let data_final_dir = game_dir.join("DATA").join("FINAL");

        if !data_final_dir.as_std_path().exists() {
            return Err(Error::InvalidGameDir(format!(
                "DATA/FINAL not found in {}",
                game_dir
            )));
        }

        tracing::info!("Building game index from {}", data_final_dir);

        // Single directory walk — reused by all three index-building functions.
        let wad_paths = collect_wad_paths_sorted(&data_final_dir)?;

        let wad_index = build_wad_filename_index(&wad_paths);
        let (hash_index, wad_relative_paths) = build_game_hash_index(game_dir, &wad_paths);
        let game_fingerprint = calculate_game_fingerprint(&wad_paths);
        let subchunktoc_blocked = build_subchunktoc_blocked(&wad_relative_paths);

        tracing::info!(
            "Game index built: {} WAD filenames, {} unique hashes, {} SubChunkTOC blocked, fingerprint: {:016x}",
            wad_index.len(),
            hash_index.len(),
            subchunktoc_blocked.len(),
            game_fingerprint
        );

        Ok(Self {
            wad_index,
            hash_index,
            game_fingerprint,
            subchunktoc_blocked,
        })
    }

    /// Load index from cache if valid, otherwise rebuild.
    ///
    /// The cache is considered valid when:
    /// 1. The cache file exists and deserializes successfully
    /// 2. The cache version matches the current format
    /// 3. The game fingerprint (derived from WAD file sizes/timestamps) matches
    ///
    /// If the cache is stale or missing, a fresh index is built and saved to
    /// the cache path (best-effort — save failures are logged but not fatal).
    ///
    /// # Arguments
    ///
    /// * `game_dir` - Path to the League of Legends Game directory
    /// * `cache_path` - Path to the cached index file
    pub fn load_or_build(game_dir: &Utf8Path, cache_path: &Utf8Path) -> Result<Self> {
        // Try loading from cache
        match Self::load_cache(cache_path) {
            Ok(Some(cached)) => {
                // Verify the game hasn't been patched by computing a fresh fingerprint
                let data_final_dir = game_dir.join("DATA").join("FINAL");
                let current_fp = calculate_game_fingerprint_from_dir(&data_final_dir)?;

                if cached.game_fingerprint == current_fp {
                    tracing::info!(
                        "Game index loaded from cache (fingerprint {:016x} matched)",
                        current_fp
                    );
                    return Ok(cached);
                }
                tracing::info!(
                    "Game index cache stale (fingerprint {:016x} != {:016x}), rebuilding",
                    cached.game_fingerprint,
                    current_fp
                );
            }
            Ok(None) => {
                tracing::debug!("No game index cache found at {}", cache_path);
            }
            Err(e) => {
                tracing::warn!("Failed to load game index cache: {}", e);
            }
        }

        // Build fresh
        let index = Self::build(game_dir)?;

        // Save to cache (best-effort)
        if let Err(e) = index.save(cache_path) {
            tracing::warn!("Failed to save game index cache: {}", e);
        }

        Ok(index)
    }

    /// Save the index to a cache file using MessagePack.
    ///
    /// # Arguments
    ///
    /// * `cache_path` - Path where the index should be saved
    pub fn save(&self, cache_path: &Utf8Path) -> Result<()> {
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent.as_std_path())?;
        }

        let cache = self.to_cache();
        let bytes = rmp_serde::to_vec_named(&cache)
            .map_err(|e| Error::Other(format!("Failed to serialize game index cache: {}", e)))?;
        std::fs::write(cache_path.as_std_path(), bytes)?;

        tracing::debug!("Game index cache saved to {}", cache_path);
        Ok(())
    }

    /// Find a WAD file by its filename (case-insensitive).
    ///
    /// Returns `None` if the WAD is not found, or an error if multiple candidates exist.
    ///
    /// # Arguments
    ///
    /// * `filename` - The WAD filename to search for (e.g., "Aatrox.wad.client")
    pub fn find_wad(&self, filename: &str) -> Result<&Utf8PathBuf> {
        let key = filename.to_ascii_lowercase();
        let candidates = self
            .wad_index
            .get(&key)
            .ok_or_else(|| Error::WadNotFound(Utf8PathBuf::from(filename)))?;

        if candidates.len() == 1 {
            Ok(&candidates[0])
        } else {
            Err(Error::AmbiguousWad {
                name: filename.to_string(),
                count: candidates.len(),
            })
        }
    }

    /// Find all WAD files that contain a specific path hash.
    ///
    /// This is used for cross-WAD matching - distributing mod overrides to all
    /// WAD files that contain the target chunk.
    ///
    /// # Arguments
    ///
    /// * `path_hash` - The hash of the file path
    pub fn find_wads_with_hash(&self, path_hash: u64) -> Option<&[Utf8PathBuf]> {
        self.hash_index.get(&path_hash).map(|v| v.as_slice())
    }

    /// Get the game fingerprint.
    pub fn game_fingerprint(&self) -> u64 {
        self.game_fingerprint
    }

    /// Get the set of SubChunkTOC path hashes that mods must not override.
    pub fn subchunktoc_blocked(&self) -> &HashSet<u64> {
        &self.subchunktoc_blocked
    }

    /// Compute content hashes on-demand for a batch of path hashes.
    ///
    /// Groups the requested `path_hashes` by the WAD files that contain them
    /// (using the hash index), opens each WAD once, and decompresses only the
    /// needed chunks to compute `xxh3_64(uncompressed_bytes)`.
    ///
    /// # Arguments
    ///
    /// * `game_dir` - Path to the League of Legends Game directory
    /// * `path_hashes` - The set of chunk path hashes to compute content hashes for
    pub fn compute_content_hashes_batch(
        &self,
        game_dir: &Utf8Path,
        path_hashes: &HashSet<u64>,
    ) -> HashMap<u64, u64> {
        use ltk_wad::Wad;
        use xxhash_rust::xxh3::xxh3_64;

        // Group requested hashes by WAD file (pick the first WAD for each hash).
        let mut wad_to_hashes: HashMap<&Utf8PathBuf, Vec<u64>> = HashMap::new();
        for &ph in path_hashes {
            if let Some(wad_paths) = self.hash_index.get(&ph) {
                if let Some(first_wad) = wad_paths.first() {
                    wad_to_hashes.entry(first_wad).or_default().push(ph);
                }
            }
        }

        let mut result: HashMap<u64, u64> = HashMap::with_capacity(path_hashes.len());

        for (wad_rel_path, hashes) in &wad_to_hashes {
            let abs_path = game_dir.join(wad_rel_path);
            let needed: HashSet<u64> = hashes.iter().copied().collect();

            let file = match std::fs::File::open(abs_path.as_std_path()) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("Failed to open WAD '{}': {}", abs_path, e);
                    continue;
                }
            };

            let mut wad = match Wad::mount(file) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to mount WAD '{}': {}", abs_path, e);
                    continue;
                }
            };

            let chunks: Vec<_> = wad
                .chunks()
                .iter()
                .filter(|c| needed.contains(&c.path_hash))
                .cloned()
                .collect();

            for chunk in &chunks {
                match wad.load_chunk_decompressed(chunk) {
                    Ok(data) => {
                        result.insert(chunk.path_hash, xxh3_64(&data));
                    }
                    Err(e) => {
                        tracing::trace!(
                            "Failed to decompress chunk {:016x} in '{}': {}",
                            chunk.path_hash,
                            abs_path,
                            e
                        );
                    }
                }
            }
        }

        tracing::info!(
            "Computed {} content hashes on-demand (from {} WADs)",
            result.len(),
            wad_to_hashes.len()
        );

        result
    }

    /// Deserialize a [`GameIndex`] from a cache file.
    fn load_cache(cache_path: &Utf8Path) -> Result<Option<Self>> {
        if !cache_path.as_std_path().exists() {
            return Ok(None);
        }

        let bytes = std::fs::read(cache_path.as_std_path())?;
        let cache: GameIndexCache = match rmp_serde::from_slice(&bytes) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to deserialize game index cache: {}", e);
                return Ok(None);
            }
        };

        if cache.version != CACHE_VERSION {
            tracing::info!(
                "Game index cache version mismatch ({} != {}), ignoring",
                cache.version,
                CACHE_VERSION
            );
            return Ok(None);
        }

        Ok(Some(Self::from_cache(cache)))
    }

    /// Convert from the cache representation to the runtime format.
    fn from_cache(cache: GameIndexCache) -> Self {
        Self {
            wad_index: cache.wad_index,
            hash_index: cache.hash_index,
            game_fingerprint: cache.game_fingerprint,
            subchunktoc_blocked: cache.subchunktoc_blocked.into_iter().collect(),
        }
    }

    /// Convert to the cache representation for serialization.
    fn to_cache(&self) -> GameIndexCache {
        GameIndexCache {
            version: CACHE_VERSION,
            game_fingerprint: self.game_fingerprint,
            wad_index: self.wad_index.clone(),
            hash_index: self.hash_index.clone(),
            subchunktoc_blocked: self.subchunktoc_blocked.iter().copied().collect(),
        }
    }
}

/// Collect all `.wad.client` file paths under `root`, sorted for deterministic ordering.
fn collect_wad_paths_sorted(root: &Utf8Path) -> Result<Vec<Utf8PathBuf>> {
    let mut paths: Vec<Utf8PathBuf> = WalkDir::new(root.as_std_path())
        .into_iter()
        .filter_map(|entry| {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Skipping unreadable entry: {}", e);
                    return None;
                }
            };

            if entry.file_type().is_dir() {
                return None;
            }

            let path = match Utf8PathBuf::from_path_buf(entry.into_path()) {
                Ok(p) => p,
                Err(p) => {
                    tracing::warn!("Skipping non-UTF-8 path: {}", p.display());
                    return None;
                }
            };

            let name = path.file_name()?;
            if !name.to_ascii_lowercase().ends_with(".wad.client") {
                return None;
            }

            Some(path)
        })
        .collect();

    paths.sort();
    Ok(paths)
}

/// Index pre-collected WAD paths by lowercase filename.
fn build_wad_filename_index(wad_paths: &[Utf8PathBuf]) -> HashMap<String, Vec<Utf8PathBuf>> {
    let mut index: HashMap<String, Vec<Utf8PathBuf>> = HashMap::new();

    for path in wad_paths {
        let name = path.file_name().unwrap();
        index
            .entry(name.to_ascii_lowercase())
            .or_default()
            .push(path.clone());
    }

    index
}

/// Hash index result: chunk path hashes -> WAD paths, plus the list of WAD relative paths.
type HashIndexResult = (HashMap<u64, Vec<Utf8PathBuf>>, Vec<Utf8PathBuf>);

/// Per-WAD result from mounting: the chunk path hashes found inside.
struct WadMountResult {
    relative_path: Utf8PathBuf,
    chunk_hashes: Vec<u64>,
}

/// Mount a single WAD and extract chunk path hashes (TOC only — no data I/O).
fn mount_and_extract_hashes(
    abs_path: &Utf8Path,
    relative_path: Utf8PathBuf,
) -> Option<WadMountResult> {
    use ltk_wad::Wad;

    let file = match std::fs::File::open(abs_path.as_std_path()) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Failed to open WAD '{}': {}", abs_path, e);
            return None;
        }
    };

    let wad = match Wad::mount(file) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("Failed to mount WAD '{}': {}", abs_path, e);
            return None;
        }
    };

    let chunk_hashes: Vec<u64> = wad.chunks().iter().map(|c| c.path_hash).collect();

    Some(WadMountResult {
        relative_path,
        chunk_hashes,
    })
}

/// Mount every WAD file and build a reverse index: `chunk_path_hash -> [relative_wad_paths]`.
///
/// Also returns the set of all WAD relative paths (for SubChunkTOC computation).
/// WAD files that fail to open or mount are skipped with a warning.
/// WADs are mounted concurrently using rayon.
fn build_game_hash_index(game_dir: &Utf8Path, wad_paths: &[Utf8PathBuf]) -> HashIndexResult {
    // Compute relative paths
    let wad_abs_rel: Vec<(&Utf8PathBuf, Utf8PathBuf)> = wad_paths
        .iter()
        .filter_map(|abs_path| {
            let rel = abs_path.strip_prefix(game_dir).ok()?.to_path_buf();
            Some((abs_path, rel))
        })
        .collect();

    // Mount all WADs in parallel and extract their chunk hashes (TOC only)
    use rayon::prelude::*;
    let mount_results: Vec<WadMountResult> = wad_abs_rel
        .into_par_iter()
        .filter_map(|(abs, rel)| mount_and_extract_hashes(abs, rel))
        .collect();

    // Merge results into the hash index
    let mut hash_to_wads: HashMap<u64, Vec<Utf8PathBuf>> = HashMap::new();
    let mut wad_relative_paths: Vec<Utf8PathBuf> = Vec::with_capacity(mount_results.len());
    let mut chunk_count = 0usize;

    for result in mount_results {
        wad_relative_paths.push(result.relative_path.clone());
        for hash in &result.chunk_hashes {
            hash_to_wads
                .entry(*hash)
                .or_default()
                .push(result.relative_path.clone());
            chunk_count += 1;
        }
    }

    tracing::info!(
        "Game hash index built: {} WADs, {} total chunk entries, {} unique hashes",
        wad_relative_paths.len(),
        chunk_count,
        hash_to_wads.len()
    );

    (hash_to_wads, wad_relative_paths)
}

/// Compute SubChunkTOC path hashes for all WAD relative paths.
///
/// For each WAD relative path like `DATA/FINAL/Champions/Aatrox.wad.client`, replaces
/// the final `.client` extension with `.SubChunkTOC`, normalizes to forward slashes and
/// lowercase, and hashes with XXH64 (seed 0).
fn build_subchunktoc_blocked(wad_relative_paths: &[Utf8PathBuf]) -> HashSet<u64> {
    use xxhash_rust::xxh64::xxh64;

    let mut blocked = HashSet::new();

    for rel_path in wad_relative_paths {
        let path_str = rel_path.as_str();

        // Replace the last extension (.client) with .SubChunkTOC
        // e.g., "DATA/FINAL/Champions/Aatrox.wad.client" -> "DATA/FINAL/Champions/Aatrox.wad.SubChunkTOC"
        let toc_path = if let Some(stripped) = path_str.strip_suffix(".client") {
            format!("{}.SubChunkTOC", stripped)
        } else {
            // Shouldn't happen since we only scan .wad.client files, but handle gracefully
            continue;
        };

        // Normalize: forward slashes, lowercase
        let normalized = toc_path.replace('\\', "/").to_lowercase();
        let hash = xxh64(normalized.as_bytes(), 0);
        blocked.insert(hash);

        tracing::trace!("SubChunkTOC blocked: {} -> {:016x}", normalized, hash);
    }

    blocked
}

/// Compute an xxHash3 fingerprint from pre-collected WAD paths, sizes, and modification times.
fn calculate_game_fingerprint(wad_paths: &[Utf8PathBuf]) -> u64 {
    use xxhash_rust::xxh3::xxh3_64;

    let mut hasher_input = Vec::new();

    for path in wad_paths {
        // Include path and metadata in fingerprint
        hasher_input.extend_from_slice(path.as_str().as_bytes());

        if let Ok(metadata) = std::fs::metadata(path.as_std_path()) {
            // Include file size and modification time
            hasher_input.extend_from_slice(&metadata.len().to_le_bytes());
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                    hasher_input.extend_from_slice(&duration.as_secs().to_le_bytes());
                }
            }
        }
    }

    xxh3_64(&hasher_input)
}

/// Wrapper that performs its own directory walk for cache validation in [`GameIndex::load_or_build`].
fn calculate_game_fingerprint_from_dir(data_final_dir: &Utf8Path) -> Result<u64> {
    let wad_paths = collect_wad_paths_sorted(data_final_dir)?;
    Ok(calculate_game_fingerprint(&wad_paths))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wad_index_creation() {
        // This would require a test fixture with actual WAD files
        // For now, just test that the types compile
    }

    #[test]
    fn test_subchunktoc_blocked() {
        let paths = vec![
            Utf8PathBuf::from("DATA/FINAL/Champions/Aatrox.wad.client"),
            Utf8PathBuf::from("DATA/FINAL/Maps/Map11.wad.client"),
        ];

        let blocked = build_subchunktoc_blocked(&paths);

        // Should have one entry per WAD path
        assert_eq!(blocked.len(), 2);

        // Verify the hash for a known path
        use xxhash_rust::xxh64::xxh64;
        let expected_hash = xxh64(b"data/final/champions/aatrox.wad.subchunktoc", 0);
        assert!(
            blocked.contains(&expected_hash),
            "Expected hash {:016x} for aatrox SubChunkTOC",
            expected_hash
        );
    }

    #[test]
    fn test_subchunktoc_blocked_backslash_normalization() {
        // Windows-style paths should be normalized to forward slashes
        let paths = vec![Utf8PathBuf::from(
            "DATA\\FINAL\\Champions\\Aatrox.wad.client",
        )];

        let blocked = build_subchunktoc_blocked(&paths);

        use xxhash_rust::xxh64::xxh64;
        let expected_hash = xxh64(b"data/final/champions/aatrox.wad.subchunktoc", 0);
        assert!(
            blocked.contains(&expected_hash),
            "Backslash paths should normalize to same hash"
        );
    }

    #[test]
    fn test_cache_roundtrip() {
        // Build a GameIndex with known data
        let mut wad_index = HashMap::new();
        wad_index.insert(
            "aatrox.wad.client".to_string(),
            vec![Utf8PathBuf::from(
                "/game/DATA/FINAL/Champions/Aatrox.wad.client",
            )],
        );

        let mut hash_index = HashMap::new();
        hash_index.insert(
            0xDEADBEEF_u64,
            vec![Utf8PathBuf::from("DATA/FINAL/Champions/Aatrox.wad.client")],
        );

        let mut subchunktoc_blocked = HashSet::new();
        subchunktoc_blocked.insert(0xCAFEBABE_u64);

        let index = GameIndex {
            wad_index,
            hash_index,
            game_fingerprint: 0x123456,
            subchunktoc_blocked,
        };

        // Convert to cache and back
        let cache = index.to_cache();
        assert_eq!(cache.version, CACHE_VERSION);
        assert_eq!(cache.game_fingerprint, 0x123456);

        let restored = GameIndex::from_cache(cache);
        assert_eq!(restored.game_fingerprint, 0x123456);
        assert_eq!(
            restored.find_wads_with_hash(0xDEADBEEF).map(|v| v.len()),
            Some(1)
        );
        assert!(restored.subchunktoc_blocked.contains(&0xCAFEBABE));
        assert!(restored.find_wad("aatrox.wad.client").is_ok());
    }

    #[test]
    fn test_cache_save_and_load() {
        let mut wad_index = HashMap::new();
        wad_index.insert(
            "test.wad.client".to_string(),
            vec![Utf8PathBuf::from("/game/DATA/FINAL/test.wad.client")],
        );

        let index = GameIndex {
            wad_index,
            hash_index: HashMap::new(),
            game_fingerprint: 0xABCDEF,
            subchunktoc_blocked: HashSet::new(),
        };

        let temp = tempfile::NamedTempFile::new().unwrap();
        let cache_path = Utf8Path::from_path(temp.path()).unwrap();

        // Save
        index.save(cache_path).unwrap();

        // Load
        let loaded = GameIndex::load_cache(cache_path).unwrap().unwrap();
        assert_eq!(loaded.game_fingerprint, 0xABCDEF);
        assert!(loaded.find_wad("test.wad.client").is_ok());
    }
}
