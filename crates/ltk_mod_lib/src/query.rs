use std::collections::HashSet;

use camino::Utf8Path;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::LibraryResult;
use crate::index::LibraryIndex;
use crate::install::read_installed_mod;

/// A mod entry with full metadata, ready for display or JSON output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub enabled: bool,
    pub installed_at: DateTime<Utc>,
    pub layers: Vec<ModLayer>,
    pub tags: Vec<String>,
    pub champions: Vec<String>,
    pub maps: Vec<String>,
    pub mod_dir: String,
}

/// A mod layer with current enabled state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLayer {
    pub name: String,
    pub priority: i32,
    pub enabled: bool,
}

impl LibraryIndex {
    /// Get all installed mods with their status in the active profile.
    pub fn get_installed_mods(&self, storage_dir: &Utf8Path) -> LibraryResult<Vec<InstalledMod>> {
        let active_profile = self.active_profile()?;

        let enabled_set: HashSet<&str> = active_profile
            .enabled_mods
            .iter()
            .map(String::as_str)
            .collect();

        let mut result = Vec::new();
        for mod_id in &active_profile.mod_order {
            let Some(entry) = self.mods.iter().find(|m| &m.id == mod_id) else {
                continue;
            };
            let enabled = enabled_set.contains(mod_id.as_str());
            let layer_states = active_profile.layer_states.get(mod_id.as_str());
            match read_installed_mod(entry, enabled, storage_dir, layer_states) {
                Ok(m) => result.push(m),
                Err(e) => {
                    tracing::warn!("Skipping broken mod entry {}: {}", entry.id, e);
                }
            }
        }

        Ok(result)
    }
}
