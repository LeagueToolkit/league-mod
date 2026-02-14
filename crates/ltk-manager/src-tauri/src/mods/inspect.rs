use crate::error::{AppError, AppResult};
use ltk_modpkg::Modpkg;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Information returned by `inspect_modpkg`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpkgInfo {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub layers: Vec<LayerInfo>,
    pub file_count: u64,
    pub total_size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    pub name: String,
    pub priority: i32,
    pub description: Option<String>,
    pub file_count: u64,
}

pub fn inspect_modpkg_file(file_path: &str) -> AppResult<ModpkgInfo> {
    let file_path = Path::new(file_path);
    let file = std::fs::File::open(file_path)?;
    let mut modpkg =
        Modpkg::mount_from_reader(file).map_err(|e| AppError::Modpkg(e.to_string()))?;

    let metadata = modpkg
        .load_metadata()
        .map_err(|e| AppError::Modpkg(e.to_string()))?;

    let authors = metadata
        .authors
        .iter()
        .map(|a| a.name.clone())
        .collect::<Vec<_>>();
    let mut file_count: u64 = 0;
    let mut total_size: u64 = 0;

    // Count content chunks (exclude meta folder paths for file_count/size)
    for ((path_hash, _layer_hash), chunk) in &modpkg.chunks {
        let path = modpkg
            .chunk_paths
            .get(path_hash)
            .map(String::as_str)
            .unwrap_or("");
        if path.starts_with("_meta_/") {
            continue;
        }
        file_count += 1;
        total_size += chunk.uncompressed_size;
    }

    // Layer counts: derive from header layer list.
    let mut layer_counts: BTreeMap<String, u64> = BTreeMap::new();
    for (path_hash, layer_hash) in modpkg.chunks.keys() {
        let path = modpkg
            .chunk_paths
            .get(path_hash)
            .map(String::as_str)
            .unwrap_or("");
        if path.starts_with("_meta_/") {
            continue;
        }
        if let Some(layer) = modpkg.layers.get(layer_hash) {
            *layer_counts.entry(layer.name.clone()).or_insert(0) += 1;
        }
    }

    let mut layers = Vec::new();
    for layer in modpkg.layers.values() {
        let count = layer_counts.get(&layer.name).copied().unwrap_or(0);
        let desc = metadata
            .layers
            .iter()
            .find(|l| l.name == layer.name)
            .and_then(|l| l.description.clone());
        layers.push(LayerInfo {
            name: layer.name.clone(),
            priority: layer.priority,
            description: desc,
            file_count: count,
        });
    }
    layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

    Ok(ModpkgInfo {
        name: metadata.name,
        display_name: metadata.display_name,
        version: metadata.version.to_string(),
        description: metadata.description,
        authors,
        layers,
        file_count,
        total_size,
    })
}
