use xxhash_rust::{xxh3, xxh64};

pub fn is_hex_chunk_name(chunk_name: &str) -> bool {
    // Reject 0x prefix
    if chunk_name.starts_with("0x") {
        return false;
    }

    // Validate the base name (before extension)
    let base = chunk_name.split('.').next().unwrap_or(chunk_name);
    if base.len() != 16 {
        return false;
    }

    base.chars().all(|c| c.is_ascii_hexdigit())
}

/// Normalize a chunk path for storage and hashing.
///
/// Lowercases the path and converts backslashes to forward slashes so that
/// the same logical path is represented identically on all platforms.
/// Call this once before storing or hashing a chunk path.
pub fn normalize_chunk_path(path: &str) -> String {
    path.to_lowercase().replace('\\', "/")
}

/// Hash a layer name using xxhash3.
pub fn hash_layer_name(name: &str) -> u64 {
    xxh3::xxh3_64(name.to_lowercase().as_bytes())
}

/// Hash a chunk name using xxhash64.
///
/// Expects a pre-normalized path (lowercase, forward slashes).
/// Use [`normalize_chunk_path`] before calling this if the input may
/// contain uppercase characters or backslashes.
pub fn hash_chunk_name(name: &str) -> u64 {
    xxh64::xxh64(name.as_bytes(), 0)
}

/// Hash a wad name using xxhash3.
pub fn hash_wad_name(name: &str) -> u64 {
    xxh3::xxh3_64(name.to_lowercase().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_chunk_path_lowercases() {
        assert_eq!(
            normalize_chunk_path("Graves.wad.client/Data/File.bin"),
            "graves.wad.client/data/file.bin"
        );
    }

    #[test]
    fn normalize_chunk_path_converts_backslashes() {
        assert_eq!(
            normalize_chunk_path("graves.wad.client\\data\\characters\\graves"),
            "graves.wad.client/data/characters/graves"
        );
    }

    #[test]
    fn normalize_chunk_path_handles_mixed_separators() {
        assert_eq!(
            normalize_chunk_path("Graves.wad.client\\Data/Characters\\Graves"),
            "graves.wad.client/data/characters/graves"
        );
    }

    #[test]
    fn normalize_chunk_path_noop_on_normalized() {
        let path = "graves.wad.client/data/file.bin";
        assert_eq!(normalize_chunk_path(path), path);
    }

    #[test]
    fn hash_chunk_name_consistent_after_normalization() {
        let forward = normalize_chunk_path("graves.wad.client/data/file.bin");
        let back = normalize_chunk_path("graves.wad.client\\data\\file.bin");
        let mixed = normalize_chunk_path("Graves.wad.client\\Data/File.bin");

        assert_eq!(hash_chunk_name(&forward), hash_chunk_name(&back));
        assert_eq!(hash_chunk_name(&forward), hash_chunk_name(&mixed));
    }
}
