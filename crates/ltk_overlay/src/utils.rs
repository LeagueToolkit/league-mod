//! Utility functions for overlay building.

use crate::error::Result;
use std::path::Path;

/// Normalize a relative path for hash calculation.
///
/// This handles special cases like .ltk suffixes added by extractors.
pub fn normalize_rel_path_for_hash(rel_path: &Path, _bytes: &[u8]) -> String {
    let mut parts = rel_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return String::new();
    }

    // Special case: strip `.ltk` suffix patterns from the filename
    let last = parts.pop().unwrap();
    let stripped = if let Some(idx) = last.to_ascii_lowercase().find(".ltk.") {
        // Remove .ltk - e.g., "file.ltk.bin" -> "file.bin"
        // idx points to the '.', we want to keep it and append from after '.ltk'
        format!("{}{}", &last[..idx], &last[idx + 4..])
    } else if last.to_ascii_lowercase().ends_with(".ltk") {
        // Remove .ltk suffix - e.g., "file.ltk" -> "file"
        last[..last.len().saturating_sub(4)].to_string()
    } else {
        last
    };
    parts.push(stripped);

    // Join using '/'
    let joined = parts.join("/");

    // If we stripped to empty (rare), fall back to original filename
    if joined.is_empty() {
        return rel_path.to_string_lossy().replace('\\', "/");
    }

    joined.replace('\\', "/")
}

/// Resolve chunk hash for a file.
///
/// If the file is named with a hex hash (16 hex digits), use that directly.
/// Otherwise, compute the hash from the normalized path.
pub fn resolve_chunk_hash(rel_path: &Path, bytes: &[u8]) -> Result<u64> {
    let file_name = rel_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let file_stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // If this is a hex-hash filename (as emitted by HexPathResolver), use it directly
    if file_stem.len() == 16 && file_stem.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(v) = u64::from_str_radix(file_stem, 16) {
            return Ok(v);
        }
    }

    // Otherwise, compute from normalized path
    let normalized_rel = normalize_rel_path_for_hash(rel_path, bytes);
    Ok(ltk_modpkg::utils::hash_chunk_name(&normalized_rel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_normalize_ltk_suffix() {
        let path = PathBuf::from("data/characters/aatrox/aatrox.ltk.bin");
        let normalized = normalize_rel_path_for_hash(&path, b"");
        assert_eq!(normalized, "data/characters/aatrox/aatrox.bin");
    }

    #[test]
    fn test_normalize_ltk_extension() {
        let path = PathBuf::from("data/characters/aatrox/aatrox.ltk");
        let normalized = normalize_rel_path_for_hash(&path, b"");
        assert_eq!(normalized, "data/characters/aatrox/aatrox");
    }

    #[test]
    fn test_normalize_regular_path() {
        let path = PathBuf::from("data/characters/aatrox/aatrox.bin");
        let normalized = normalize_rel_path_for_hash(&path, b"");
        assert_eq!(normalized, "data/characters/aatrox/aatrox.bin");
    }

    #[test]
    fn test_resolve_hex_hash() {
        let path = PathBuf::from("0123456789abcdef.bin");
        let hash = resolve_chunk_hash(&path, b"").unwrap();
        assert_eq!(hash, 0x0123456789abcdef);
    }
}
