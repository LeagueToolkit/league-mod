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

/// Hash a layer name using xxhash3.
pub fn hash_layer_name(name: &str) -> u64 {
    xxh3::xxh3_64(name.to_lowercase().as_bytes())
}

/// Hash a chunk name using xxhash64.
pub fn hash_chunk_name(name: &str) -> u64 {
    xxh64::xxh64(name.to_lowercase().as_bytes(), 0)
}

/// Hash a wad name using xxhash3.
pub fn hash_wad_name(name: &str) -> u64 {
    xxh3::xxh3_64(name.to_lowercase().as_bytes())
}
