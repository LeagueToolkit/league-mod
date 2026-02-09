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

use crate::error::{Error, Result};
use camino::{Utf8Path, Utf8PathBuf};
use std::collections::{HashMap, HashSet};

/// Index of all WAD files in a League of Legends game directory.
///
/// Built by scanning `DATA/FINAL` and mounting every `.wad.client` file to
/// record its chunk hashes. The index is ephemeral (rebuilt each overlay build)
/// — cache serialization is stubbed out for future implementation.
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

        let wad_index = build_wad_filename_index(&data_final_dir)?;
        let (hash_index, wad_relative_paths) = build_game_hash_index(game_dir, &data_final_dir)?;
        let game_fingerprint = calculate_game_fingerprint(&data_final_dir)?;
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
    /// # Arguments
    ///
    /// * `game_dir` - Path to the League of Legends Game directory
    /// * `cache_path` - Path to the cached index file
    pub fn load_or_build(game_dir: &Utf8Path, _cache_path: &Utf8Path) -> Result<Self> {
        // TODO: Implement cache loading
        // For now, always rebuild
        Self::build(game_dir)
    }

    /// Save the index to a cache file.
    ///
    /// # Arguments
    ///
    /// * `cache_path` - Path where the index should be saved
    pub fn save(&self, _cache_path: &Utf8Path) -> Result<()> {
        // TODO: Implement cache saving
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
}

/// Recursively scan `root` for `.wad.client` files and index them by lowercase filename.
fn build_wad_filename_index(root: &Utf8Path) -> Result<HashMap<String, Vec<Utf8PathBuf>>> {
    let mut index: HashMap<String, Vec<Utf8PathBuf>> = HashMap::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir.as_std_path())? {
            let entry = entry?;
            let path = match Utf8PathBuf::from_path_buf(entry.path()) {
                Ok(p) => p,
                Err(p) => {
                    tracing::warn!("Skipping non-UTF-8 path: {}", p.display());
                    continue;
                }
            };

            if path.as_std_path().is_dir() {
                stack.push(path);
                continue;
            }

            let Some(name) = path.file_name() else {
                continue;
            };

            if !name.to_ascii_lowercase().ends_with(".wad.client") {
                continue;
            }

            index
                .entry(name.to_ascii_lowercase())
                .or_default()
                .push(path);
        }
    }

    Ok(index)
}

/// Hash index mapping chunk path hashes to WAD paths, plus the list of WAD relative paths.
type HashIndexResult = (HashMap<u64, Vec<Utf8PathBuf>>, Vec<Utf8PathBuf>);

/// Mount every WAD file and build a reverse index: `chunk_path_hash -> [relative_wad_paths]`.
///
/// Also returns the set of all WAD relative paths (for SubChunkTOC computation).
/// WAD files that fail to open or mount are skipped with a warning.
fn build_game_hash_index(
    game_dir: &Utf8Path,
    data_final_dir: &Utf8Path,
) -> Result<HashIndexResult> {
    use ltk_wad::Wad;

    let mut hash_to_wads: HashMap<u64, Vec<Utf8PathBuf>> = HashMap::new();
    let mut wad_relative_paths: Vec<Utf8PathBuf> = Vec::new();
    let mut wad_count = 0;
    let mut chunk_count = 0;

    let mut stack = vec![data_final_dir.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir.as_std_path())? {
            let entry = entry?;
            let path = match Utf8PathBuf::from_path_buf(entry.path()) {
                Ok(p) => p,
                Err(p) => {
                    tracing::warn!("Skipping non-UTF-8 path: {}", p.display());
                    continue;
                }
            };

            if path.as_std_path().is_dir() {
                stack.push(path);
                continue;
            }

            let Some(name) = path.file_name() else {
                continue;
            };

            if !name.to_ascii_lowercase().ends_with(".wad.client") {
                continue;
            }

            // Get relative path from game_dir
            let relative_path = match path.strip_prefix(game_dir) {
                Ok(p) => p.to_path_buf(),
                Err(_) => continue,
            };

            // Mount WAD and index all chunk hashes
            let file = match std::fs::File::open(path.as_std_path()) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("Failed to open WAD '{}': {}", path, e);
                    continue;
                }
            };

            let wad = match Wad::mount(file) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to mount WAD '{}': {}", path, e);
                    continue;
                }
            };

            wad_count += 1;
            wad_relative_paths.push(relative_path.clone());
            for chunk in wad.chunks().iter() {
                hash_to_wads
                    .entry(chunk.path_hash)
                    .or_default()
                    .push(relative_path.clone());
                chunk_count += 1;
            }
        }
    }

    tracing::info!(
        "Game hash index built: {} WADs, {} total chunk entries, {} unique hashes",
        wad_count,
        chunk_count,
        hash_to_wads.len()
    );

    Ok((hash_to_wads, wad_relative_paths))
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

/// Compute an xxHash3 fingerprint from all WAD file paths, sizes, and modification times.
///
/// Any change to WAD files (game patch, file corruption) will produce a different
/// fingerprint, triggering a full overlay rebuild.
fn calculate_game_fingerprint(data_final_dir: &Utf8Path) -> Result<u64> {
    use xxhash_rust::xxh3::xxh3_64;

    let mut hasher_input = Vec::new();

    let mut stack = vec![data_final_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir.as_std_path())? {
            let entry = entry?;
            let path = match Utf8PathBuf::from_path_buf(entry.path()) {
                Ok(p) => p,
                Err(p) => {
                    tracing::warn!("Skipping non-UTF-8 path: {}", p.display());
                    continue;
                }
            };

            if path.as_std_path().is_dir() {
                stack.push(path);
                continue;
            }

            let Some(name) = path.file_name() else {
                continue;
            };

            if !name.to_ascii_lowercase().ends_with(".wad.client") {
                continue;
            }

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
    }

    Ok(xxh3_64(&hasher_input))
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
}
