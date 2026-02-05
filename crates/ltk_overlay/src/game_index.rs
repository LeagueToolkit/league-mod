//! Game file indexing for efficient WAD lookup.

use crate::error::{Error, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Index of game WAD files for efficient lookup.
///
/// This index provides two main lookups:
/// 1. By filename (case-insensitive) -> list of WAD file paths
/// 2. By path hash -> list of WAD files containing that chunk
pub struct GameIndex {
    /// Maps lowercase WAD filename to list of full paths.
    /// This handles the case where the same WAD name might exist in multiple locations.
    wad_index: HashMap<String, Vec<PathBuf>>,

    /// Maps chunk path hash to list of WAD files (relative to game dir) containing it.
    /// This enables cross-WAD matching - finding all WADs that contain a specific file.
    hash_index: HashMap<u64, Vec<PathBuf>>,

    /// Fingerprint of the game directory state.
    /// Used to detect when the game has been updated and the index needs rebuilding.
    game_fingerprint: u64,
}

impl GameIndex {
    /// Build a game index from the specified game directory.
    ///
    /// This scans all .wad.client files under `DATA/FINAL` and builds both indexes.
    ///
    /// # Arguments
    ///
    /// * `game_dir` - Path to the League of Legends Game directory
    pub fn build(game_dir: &Path) -> Result<Self> {
        let data_final_dir = game_dir.join("DATA").join("FINAL");

        if !data_final_dir.exists() {
            return Err(Error::InvalidGameDir(format!(
                "DATA/FINAL not found in {}",
                game_dir.display()
            )));
        }

        tracing::info!("Building game index from {}", data_final_dir.display());

        let wad_index = build_wad_filename_index(&data_final_dir)?;
        let hash_index = build_game_hash_index(game_dir, &data_final_dir)?;
        let game_fingerprint = calculate_game_fingerprint(&data_final_dir)?;

        tracing::info!(
            "Game index built: {} WAD filenames, {} unique hashes, fingerprint: {:016x}",
            wad_index.len(),
            hash_index.len(),
            game_fingerprint
        );

        Ok(Self {
            wad_index,
            hash_index,
            game_fingerprint,
        })
    }

    /// Load index from cache if valid, otherwise rebuild.
    ///
    /// # Arguments
    ///
    /// * `game_dir` - Path to the League of Legends Game directory
    /// * `cache_path` - Path to the cached index file
    pub fn load_or_build(game_dir: &Path, _cache_path: &Path) -> Result<Self> {
        // TODO: Implement cache loading
        // For now, always rebuild
        Self::build(game_dir)
    }

    /// Save the index to a cache file.
    ///
    /// # Arguments
    ///
    /// * `cache_path` - Path where the index should be saved
    pub fn save(&self, _cache_path: &Path) -> Result<()> {
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
    pub fn find_wad(&self, filename: &str) -> Result<&PathBuf> {
        let key = filename.to_ascii_lowercase();
        let candidates = self
            .wad_index
            .get(&key)
            .ok_or_else(|| Error::WadNotFound(PathBuf::from(filename)))?;

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
    pub fn find_wads_with_hash(&self, path_hash: u64) -> Option<&[PathBuf]> {
        self.hash_index.get(&path_hash).map(|v| v.as_slice())
    }

    /// Get the game fingerprint.
    pub fn game_fingerprint(&self) -> u64 {
        self.game_fingerprint
    }
}

/// Build an index of WAD filenames to their full paths.
fn build_wad_filename_index(root: &Path) -> Result<HashMap<String, Vec<PathBuf>>> {
    let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
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

/// Build a reverse index of path hashes to WAD files.
///
/// This scans all WAD files and builds a map of `path_hash -> [wad_paths]`.
fn build_game_hash_index(
    game_dir: &Path,
    data_final_dir: &Path,
) -> Result<HashMap<u64, Vec<PathBuf>>> {
    use ltk_wad::Wad;

    let mut hash_to_wads: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut wad_count = 0;
    let mut chunk_count = 0;

    let mut stack = vec![data_final_dir.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
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
            let file = match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("Failed to open WAD '{}': {}", path.display(), e);
                    continue;
                }
            };

            let wad = match Wad::mount(file) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to mount WAD '{}': {}", path.display(), e);
                    continue;
                }
            };

            wad_count += 1;
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

    Ok(hash_to_wads)
}

/// Calculate a fingerprint of the game directory.
///
/// This is used to detect when the game has been patched and the index needs rebuilding.
fn calculate_game_fingerprint(data_final_dir: &Path) -> Result<u64> {
    use xxhash_rust::xxh3::xxh3_64;

    let mut hasher_input = Vec::new();

    let mut stack = vec![data_final_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };

            if !name.to_ascii_lowercase().ends_with(".wad.client") {
                continue;
            }

            // Include path and metadata in fingerprint
            hasher_input.extend_from_slice(path.to_string_lossy().as_bytes());

            if let Ok(metadata) = std::fs::metadata(&path) {
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
    #[test]
    fn test_wad_index_creation() {
        // This would require a test fixture with actual WAD files
        // For now, just test that the types compile
    }
}
