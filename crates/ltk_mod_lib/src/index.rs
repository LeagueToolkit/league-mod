use std::collections::{HashMap, HashSet};
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{LibraryError, LibraryResult};
use crate::profile::{Profile, ProfileSlug};
use crate::progress::ProgressReporter;

/// Root persistent data structure, serialized to `library.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryIndex {
    pub mods: Vec<LibraryModEntry>,
    pub profiles: Vec<Profile>,
    pub active_profile_id: String,
}

impl Default for LibraryIndex {
    fn default() -> Self {
        let default_profile = Profile {
            id: Uuid::new_v4().to_string(),
            name: "Default".to_string(),
            slug: ProfileSlug::from("default".to_string()),
            enabled_mods: Vec::new(),
            mod_order: Vec::new(),
            layer_states: HashMap::new(),
            created_at: Utc::now(),
            last_used: Utc::now(),
        };
        let active_profile_id = default_profile.id.clone();

        Self {
            mods: Vec::new(),
            profiles: vec![default_profile],
            active_profile_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Mod management
// ---------------------------------------------------------------------------

impl LibraryIndex {
    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Load the library index from disk. Returns a default index if the file doesn't exist.
    pub fn load(storage_dir: &Utf8Path) -> LibraryResult<Self> {
        fs::create_dir_all(storage_dir.as_std_path())?;
        let path = storage_dir.join("library.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path.as_std_path())?;
        let index: Self = serde_json::from_str(&content)?;
        Ok(index)
    }

    /// Save the library index to disk.
    pub fn save(&self, storage_dir: &Utf8Path) -> LibraryResult<()> {
        fs::create_dir_all(storage_dir.as_std_path())?;
        let path = storage_dir.join("library.json");
        let contents = serde_json::to_string_pretty(self)?;
        fs::write(path.as_std_path(), contents)?;
        Ok(())
    }

    /// Uninstall a mod by ID. Removes files from storage and updates the index.
    pub fn uninstall_mod(&mut self, storage_dir: &Utf8Path, mod_id: &str) -> LibraryResult<()> {
        let Some(pos) = self.mods.iter().position(|m| m.id == mod_id) else {
            return Err(LibraryError::ModNotFound(mod_id.to_string()));
        };

        let entry = self.mods.remove(pos);

        for profile in &mut self.profiles {
            profile.mod_order.retain(|id| id != mod_id);
            profile.enabled_mods.retain(|id| id != mod_id);
            profile.layer_states.remove(mod_id);
        }

        let metadata_dir = entry.metadata_dir(storage_dir);
        if metadata_dir.exists() {
            fs::remove_dir_all(metadata_dir.as_std_path())?;
        }

        let archive_path = entry.archive_path(storage_dir);
        if archive_path.exists() {
            fs::remove_file(archive_path.as_std_path())?;
        }

        Ok(())
    }

    /// Enable or disable a mod in the active profile.
    pub fn toggle_mod(&mut self, mod_id: &str, enabled: bool) -> LibraryResult<()> {
        if !self.mods.iter().any(|m| m.id == mod_id) {
            return Err(LibraryError::ModNotFound(mod_id.to_string()));
        }

        let profile = self.active_profile_mut()?;

        if enabled {
            if !profile.enabled_mods.contains(&mod_id.to_string()) {
                let insert_pos = profile.insertion_position_for(mod_id);
                profile.enabled_mods.insert(insert_pos, mod_id.to_string());
            }
        } else {
            profile.enabled_mods.retain(|id| id != mod_id);
        }

        Ok(())
    }

    /// Reorder all mods for the active profile.
    /// The provided `mod_ids` must exactly match the active profile's mod order.
    pub fn reorder_mods(&mut self, mod_ids: Vec<String>) -> LibraryResult<()> {
        let profile = self.active_profile_mut()?;

        let mut expected_sorted: Vec<&str> = profile.mod_order.iter().map(|s| s.as_str()).collect();
        expected_sorted.sort();
        let mut new_sorted: Vec<&str> = mod_ids.iter().map(|s| s.as_str()).collect();
        new_sorted.sort();

        if expected_sorted != new_sorted {
            return Err(LibraryError::ValidationFailed(
                "Provided mod IDs do not match the profile's mod order".to_string(),
            ));
        }

        let enabled_set: HashSet<&str> = profile.enabled_mods.iter().map(|s| s.as_str()).collect();
        profile.enabled_mods = mod_ids
            .iter()
            .filter(|id| enabled_set.contains(id.as_str()))
            .cloned()
            .collect();

        profile.mod_order = mod_ids;
        Ok(())
    }

    /// Set layer enabled/disabled states for a mod in the active profile.
    pub fn set_layer_states(
        &mut self,
        mod_id: &str,
        layer_states: HashMap<String, bool>,
    ) -> LibraryResult<()> {
        if !self.mods.iter().any(|m| m.id == mod_id) {
            return Err(LibraryError::ModNotFound(mod_id.to_string()));
        }

        let profile = self.active_profile_mut()?;
        profile
            .layer_states
            .insert(mod_id.to_string(), layer_states);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Overlay
    // -----------------------------------------------------------------------

    /// Build the overlay for the active profile.
    ///
    /// Returns the overlay root directory path on success.
    pub fn build_overlay(
        &self,
        storage_dir: &Utf8Path,
        config: &crate::overlay::OverlayConfig,
        reporter: std::sync::Arc<dyn ProgressReporter>,
    ) -> LibraryResult<camino::Utf8PathBuf> {
        crate::overlay::build_overlay(storage_dir, self, config, reporter)
    }

    // -----------------------------------------------------------------------
    // Profile management
    // -----------------------------------------------------------------------

    /// Create a new profile.
    pub fn create_profile(
        &mut self,
        storage_dir: &Utf8Path,
        name: String,
    ) -> LibraryResult<Profile> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(LibraryError::ValidationFailed(
                "Profile name cannot be empty".to_string(),
            ));
        }

        if self.profiles.iter().any(|p| p.name == name) {
            return Err(LibraryError::ValidationFailed(format!(
                "Profile '{}' already exists",
                name
            )));
        }

        let slug = ProfileSlug::from_name(&name).ok_or_else(|| {
            LibraryError::ValidationFailed(
                "Profile name must contain at least one alphanumeric character".to_string(),
            )
        })?;
        if !slug.is_unique_in(self, None) {
            return Err(LibraryError::ValidationFailed(format!(
                "Profile '{}' already exists",
                name
            )));
        }

        let mod_order: Vec<String> = self.mods.iter().map(|m| m.id.clone()).collect();

        let profile = Profile {
            id: Uuid::new_v4().to_string(),
            name,
            slug,
            enabled_mods: Vec::new(),
            mod_order,
            layer_states: HashMap::new(),
            created_at: Utc::now(),
            last_used: Utc::now(),
        };

        let (overlay_dir, cache_dir) = resolve_profile_dirs(storage_dir, &profile.slug);
        fs::create_dir_all(overlay_dir.as_std_path())?;
        fs::create_dir_all(cache_dir.as_std_path())?;

        self.profiles.push(profile.clone());
        Ok(profile)
    }

    /// Delete a profile by ID.
    pub fn delete_profile(
        &mut self,
        storage_dir: &Utf8Path,
        profile_id: &str,
    ) -> LibraryResult<()> {
        let profile = self.profile_by_id(profile_id)?;

        if profile.name == "Default" {
            return Err(LibraryError::ValidationFailed(
                "Cannot delete Default profile".to_string(),
            ));
        }
        if profile_id == self.active_profile_id {
            return Err(LibraryError::ValidationFailed(
                "Cannot delete active profile. Switch to another profile first.".to_string(),
            ));
        }

        let slug = profile.slug.clone();
        self.profiles.retain(|p| p.id != profile_id);

        let profile_dir = storage_dir.join("profiles").join(slug.as_str());
        if profile_dir.exists() {
            fs::remove_dir_all(profile_dir.as_std_path())?;
        }
        Ok(())
    }

    /// Switch to a different profile.
    pub fn switch_profile(&mut self, profile_id: &str) -> LibraryResult<Profile> {
        self.profile_by_id(profile_id)?;
        self.active_profile_id = profile_id.to_string();

        let profile = self.profile_by_id_mut(profile_id)?;
        profile.last_used = Utc::now();
        Ok(profile.clone())
    }

    /// Rename a profile.
    pub fn rename_profile(
        &mut self,
        storage_dir: &Utf8Path,
        profile_id: &str,
        new_name: String,
    ) -> LibraryResult<Profile> {
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() {
            return Err(LibraryError::ValidationFailed(
                "Profile name cannot be empty".to_string(),
            ));
        }

        let new_slug = ProfileSlug::from_name(&new_name).ok_or_else(|| {
            LibraryError::ValidationFailed(
                "Profile name must contain at least one alphanumeric character".to_string(),
            )
        })?;

        if self
            .profiles
            .iter()
            .any(|p| p.id != profile_id && p.name == new_name)
        {
            return Err(LibraryError::ValidationFailed(format!(
                "Profile '{}' already exists",
                new_name
            )));
        }

        if !new_slug.is_unique_in(self, Some(profile_id)) {
            return Err(LibraryError::ValidationFailed(format!(
                "Profile directory name '{}' conflicts with another profile",
                new_slug
            )));
        }

        let profile = self.profile_by_id_mut(profile_id)?;

        if profile.name == "Default" {
            return Err(LibraryError::ValidationFailed(
                "Cannot rename Default profile".to_string(),
            ));
        }

        if profile.slug != new_slug {
            let old_dir = storage_dir.join("profiles").join(profile.slug.as_str());
            let new_dir = storage_dir.join("profiles").join(new_slug.as_str());
            if old_dir.exists() {
                fs::rename(old_dir.as_std_path(), new_dir.as_std_path())?;
            }
        }

        profile.name = new_name;
        profile.slug = new_slug;
        Ok(profile.clone())
    }

    // -----------------------------------------------------------------------
    // Overlay
    // -----------------------------------------------------------------------

    /// Delete the active profile's `overlay.json` to force rebuild.
    pub fn invalidate_overlay(&self, storage_dir: &Utf8Path) -> LibraryResult<()> {
        let profile = self.active_profile()?;
        let overlay_json = storage_dir
            .join("profiles")
            .join(profile.slug.as_str())
            .join("overlay.json");
        if overlay_json.exists() {
            fs::remove_file(overlay_json.as_std_path())?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Get the active profile.
    pub fn active_profile(&self) -> LibraryResult<&Profile> {
        self.profiles
            .iter()
            .find(|p| p.id == self.active_profile_id)
            .ok_or_else(|| LibraryError::IndexCorrupt("Active profile not found".to_string()))
    }

    /// Get a mutable reference to the active profile.
    pub fn active_profile_mut(&mut self) -> LibraryResult<&mut Profile> {
        let id = self.active_profile_id.clone();
        self.profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| LibraryError::IndexCorrupt("Active profile not found".to_string()))
    }

    /// Get a profile by ID.
    pub fn profile_by_id(&self, id: &str) -> LibraryResult<&Profile> {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| LibraryError::Other(format!("Profile not found: {}", id)))
    }

    /// Get a mutable reference to a profile by ID.
    pub fn profile_by_id_mut(&mut self, id: &str) -> LibraryResult<&mut Profile> {
        self.profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| LibraryError::Other(format!("Profile not found: {}", id)))
    }
}

// ---------------------------------------------------------------------------
// LibraryModEntry
// ---------------------------------------------------------------------------

/// Per-mod record in the library index.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryModEntry {
    pub id: String,
    pub installed_at: DateTime<Utc>,
    pub format: ModArchiveFormat,
}

impl LibraryModEntry {
    /// Directory containing extracted metadata (mod.config.json, thumbnail, etc).
    pub fn metadata_dir(&self, storage_dir: &Utf8Path) -> Utf8PathBuf {
        storage_dir.join("mods").join(&self.id)
    }

    /// Path to the stored mod archive file.
    pub fn archive_path(&self, storage_dir: &Utf8Path) -> Utf8PathBuf {
        storage_dir
            .join("archives")
            .join(format!("{}.{}", self.id, self.format.extension()))
    }
}

/// Supported mod archive formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModArchiveFormat {
    Modpkg,
    Fantome,
}

impl ModArchiveFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ModArchiveFormat::Modpkg => "modpkg",
            ModArchiveFormat::Fantome => "fantome",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "modpkg" => Some(Self::Modpkg),
            "fantome" => Some(Self::Fantome),
            _ => None,
        }
    }
}

/// Resolve profile overlay and cache directories.
pub fn resolve_profile_dirs(
    storage_dir: &Utf8Path,
    slug: &ProfileSlug,
) -> (Utf8PathBuf, Utf8PathBuf) {
    let profile_dir = storage_dir.join("profiles").join(slug.as_str());
    (profile_dir.join("overlay"), profile_dir.join("cache"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage_dir() -> Utf8PathBuf {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        #[allow(deprecated)]
        let _ = dir.into_path();
        Utf8PathBuf::from_path_buf(path).unwrap()
    }

    fn make_index_with_mods(mod_ids: &[&str]) -> LibraryIndex {
        let mut index = LibraryIndex::default();
        for id in mod_ids {
            index.mods.push(LibraryModEntry {
                id: id.to_string(),
                installed_at: Utc::now(),
                format: ModArchiveFormat::Modpkg,
            });
        }
        let profile = index.active_profile_mut().unwrap();
        for id in mod_ids {
            profile.mod_order.push(id.to_string());
            profile.enabled_mods.push(id.to_string());
        }
        index
    }

    // -----------------------------------------------------------------------
    // Default index
    // -----------------------------------------------------------------------

    #[test]
    fn default_index_has_default_profile() {
        let index = LibraryIndex::default();
        assert_eq!(index.profiles.len(), 1);
        assert_eq!(index.profiles[0].name, "Default");
        assert_eq!(index.active_profile_id, index.profiles[0].id);
    }

    #[test]
    fn default_index_has_no_mods() {
        let index = LibraryIndex::default();
        assert!(index.mods.is_empty());
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    #[test]
    fn load_returns_default_when_no_file() {
        let dir = storage_dir();
        let index = LibraryIndex::load(&dir).unwrap();
        assert_eq!(index.profiles.len(), 1);
        assert_eq!(index.profiles[0].name, "Default");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = storage_dir();
        let original = LibraryIndex::default();
        original.save(&dir).unwrap();

        let loaded = LibraryIndex::load(&dir).unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].name, "Default");
        assert_eq!(loaded.active_profile_id, original.active_profile_id);
    }

    #[test]
    fn save_creates_directory() {
        let dir = storage_dir().join("deeply").join("nested");
        let index = LibraryIndex::default();
        index.save(&dir).unwrap();
        assert!(dir.join("library.json").exists());
    }

    #[test]
    fn persistence_preserves_mods() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        index.mods.push(LibraryModEntry {
            id: "test-mod".to_string(),
            installed_at: Utc::now(),
            format: ModArchiveFormat::Fantome,
        });
        index.save(&dir).unwrap();

        let loaded = LibraryIndex::load(&dir).unwrap();
        assert_eq!(loaded.mods.len(), 1);
        assert_eq!(loaded.mods[0].id, "test-mod");
        assert_eq!(loaded.mods[0].format, ModArchiveFormat::Fantome);
    }

    // -----------------------------------------------------------------------
    // ModArchiveFormat
    // -----------------------------------------------------------------------

    #[test]
    fn format_extension_roundtrip() {
        assert_eq!(ModArchiveFormat::Modpkg.extension(), "modpkg");
        assert_eq!(ModArchiveFormat::Fantome.extension(), "fantome");

        assert_eq!(
            ModArchiveFormat::from_extension("modpkg"),
            Some(ModArchiveFormat::Modpkg)
        );
        assert_eq!(
            ModArchiveFormat::from_extension("fantome"),
            Some(ModArchiveFormat::Fantome)
        );
    }

    #[test]
    fn format_from_extension_case_insensitive() {
        assert_eq!(
            ModArchiveFormat::from_extension("MODPKG"),
            Some(ModArchiveFormat::Modpkg)
        );
        assert_eq!(
            ModArchiveFormat::from_extension("Fantome"),
            Some(ModArchiveFormat::Fantome)
        );
    }

    #[test]
    fn format_from_extension_unknown() {
        assert_eq!(ModArchiveFormat::from_extension("zip"), None);
        assert_eq!(ModArchiveFormat::from_extension(""), None);
    }

    // -----------------------------------------------------------------------
    // LibraryModEntry paths
    // -----------------------------------------------------------------------

    #[test]
    fn mod_entry_metadata_dir() {
        let entry = LibraryModEntry {
            id: "abc-123".to_string(),
            installed_at: Utc::now(),
            format: ModArchiveFormat::Modpkg,
        };
        let dir = entry.metadata_dir(Utf8Path::new("/storage"));
        assert_eq!(dir, Utf8PathBuf::from("/storage/mods/abc-123"));
    }

    #[test]
    fn mod_entry_archive_path_modpkg() {
        let entry = LibraryModEntry {
            id: "abc-123".to_string(),
            installed_at: Utc::now(),
            format: ModArchiveFormat::Modpkg,
        };
        let path = entry.archive_path(Utf8Path::new("/storage"));
        assert_eq!(path, Utf8PathBuf::from("/storage/archives/abc-123.modpkg"));
    }

    #[test]
    fn mod_entry_archive_path_fantome() {
        let entry = LibraryModEntry {
            id: "abc-123".to_string(),
            installed_at: Utc::now(),
            format: ModArchiveFormat::Fantome,
        };
        let path = entry.archive_path(Utf8Path::new("/storage"));
        assert_eq!(path, Utf8PathBuf::from("/storage/archives/abc-123.fantome"));
    }

    // -----------------------------------------------------------------------
    // toggle_mod
    // -----------------------------------------------------------------------

    #[test]
    fn toggle_mod_enable() {
        let mut index = make_index_with_mods(&["a", "b", "c"]);
        {
            let profile = index.active_profile_mut().unwrap();
            profile.enabled_mods.clear();
        }

        index.toggle_mod("b", true).unwrap();
        let profile = index.active_profile().unwrap();
        assert_eq!(profile.enabled_mods, vec!["b"]);
    }

    #[test]
    fn toggle_mod_disable() {
        let mut index = make_index_with_mods(&["a", "b", "c"]);
        index.toggle_mod("b", false).unwrap();

        let profile = index.active_profile().unwrap();
        assert_eq!(profile.enabled_mods, vec!["a", "c"]);
    }

    #[test]
    fn toggle_mod_enable_preserves_order() {
        let mut index = make_index_with_mods(&["a", "b", "c"]);
        {
            let profile = index.active_profile_mut().unwrap();
            profile.enabled_mods = vec!["a".to_string(), "c".to_string()];
        }

        index.toggle_mod("b", true).unwrap();
        let profile = index.active_profile().unwrap();
        assert_eq!(profile.enabled_mods, vec!["a", "b", "c"]);
    }

    #[test]
    fn toggle_mod_enable_idempotent() {
        let mut index = make_index_with_mods(&["a"]);
        index.toggle_mod("a", true).unwrap();
        let profile = index.active_profile().unwrap();
        assert_eq!(profile.enabled_mods, vec!["a"]);
    }

    #[test]
    fn toggle_mod_not_found() {
        let mut index = LibraryIndex::default();
        let result = index.toggle_mod("nonexistent", true);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LibraryError::ModNotFound(_)));
    }

    // -----------------------------------------------------------------------
    // reorder_mods
    // -----------------------------------------------------------------------

    #[test]
    fn reorder_mods_changes_order() {
        let mut index = make_index_with_mods(&["a", "b", "c"]);
        index
            .reorder_mods(vec!["c".into(), "a".into(), "b".into()])
            .unwrap();

        let profile = index.active_profile().unwrap();
        assert_eq!(profile.mod_order, vec!["c", "a", "b"]);
    }

    #[test]
    fn reorder_mods_updates_enabled_order() {
        let mut index = make_index_with_mods(&["a", "b", "c"]);
        {
            let profile = index.active_profile_mut().unwrap();
            profile.enabled_mods = vec!["a".to_string(), "c".to_string()];
        }

        index
            .reorder_mods(vec!["c".into(), "b".into(), "a".into()])
            .unwrap();

        let profile = index.active_profile().unwrap();
        assert_eq!(profile.enabled_mods, vec!["c", "a"]);
    }

    #[test]
    fn reorder_mods_rejects_mismatched_ids() {
        let mut index = make_index_with_mods(&["a", "b"]);
        let result = index.reorder_mods(vec!["a".into(), "x".into()]);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            LibraryError::ValidationFailed(_)
        ));
    }

    #[test]
    fn reorder_mods_rejects_missing_ids() {
        let mut index = make_index_with_mods(&["a", "b", "c"]);
        let result = index.reorder_mods(vec!["a".into(), "b".into()]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // set_layer_states
    // -----------------------------------------------------------------------

    #[test]
    fn set_layer_states_success() {
        let mut index = make_index_with_mods(&["mod-1"]);
        let mut states = HashMap::new();
        states.insert("base".to_string(), true);
        states.insert("chroma".to_string(), false);

        index.set_layer_states("mod-1", states.clone()).unwrap();

        let profile = index.active_profile().unwrap();
        assert_eq!(profile.layer_states.get("mod-1").unwrap(), &states);
    }

    #[test]
    fn set_layer_states_mod_not_found() {
        let mut index = LibraryIndex::default();
        let result = index.set_layer_states("nope", HashMap::new());
        assert!(matches!(result.unwrap_err(), LibraryError::ModNotFound(_)));
    }

    // -----------------------------------------------------------------------
    // uninstall_mod
    // -----------------------------------------------------------------------

    #[test]
    fn uninstall_mod_removes_from_index() {
        let dir = storage_dir();
        let mut index = make_index_with_mods(&["mod-1", "mod-2"]);

        // Create the expected filesystem entries so uninstall doesn't fail
        let entry = index.mods.iter().find(|m| m.id == "mod-1").unwrap();
        let metadata_dir = entry.metadata_dir(&dir);
        let archive_path = entry.archive_path(&dir);
        fs::create_dir_all(metadata_dir.as_std_path()).unwrap();
        fs::create_dir_all(archive_path.parent().unwrap().as_std_path()).unwrap();
        fs::write(archive_path.as_std_path(), b"fake").unwrap();

        index.uninstall_mod(&dir, "mod-1").unwrap();
        assert_eq!(index.mods.len(), 1);
        assert_eq!(index.mods[0].id, "mod-2");
    }

    #[test]
    fn uninstall_mod_removes_from_all_profiles() {
        let dir = storage_dir();
        let mut index = make_index_with_mods(&["mod-1"]);

        // Add a second profile
        let second = Profile {
            id: "p2".to_string(),
            name: "Second".to_string(),
            slug: ProfileSlug::from("second".to_string()),
            enabled_mods: vec!["mod-1".to_string()],
            mod_order: vec!["mod-1".to_string()],
            layer_states: {
                let mut m = HashMap::new();
                m.insert("mod-1".to_string(), HashMap::new());
                m
            },
            created_at: Utc::now(),
            last_used: Utc::now(),
        };
        index.profiles.push(second);

        // Create filesystem entries
        let entry = index.mods.iter().find(|m| m.id == "mod-1").unwrap();
        let metadata_dir = entry.metadata_dir(&dir);
        let archive_path = entry.archive_path(&dir);
        fs::create_dir_all(metadata_dir.as_std_path()).unwrap();
        fs::create_dir_all(archive_path.parent().unwrap().as_std_path()).unwrap();
        fs::write(archive_path.as_std_path(), b"fake").unwrap();

        index.uninstall_mod(&dir, "mod-1").unwrap();

        for profile in &index.profiles {
            assert!(!profile.enabled_mods.contains(&"mod-1".to_string()));
            assert!(!profile.mod_order.contains(&"mod-1".to_string()));
            assert!(!profile.layer_states.contains_key("mod-1"));
        }
    }

    #[test]
    fn uninstall_mod_cleans_filesystem() {
        let dir = storage_dir();
        let mut index = make_index_with_mods(&["mod-1"]);

        let entry = index.mods.iter().find(|m| m.id == "mod-1").unwrap();
        let metadata_dir = entry.metadata_dir(&dir);
        let archive_path = entry.archive_path(&dir);
        fs::create_dir_all(metadata_dir.as_std_path()).unwrap();
        fs::create_dir_all(archive_path.parent().unwrap().as_std_path()).unwrap();
        fs::write(metadata_dir.join("mod.config.json").as_std_path(), b"{}").unwrap();
        fs::write(archive_path.as_std_path(), b"fake").unwrap();

        index.uninstall_mod(&dir, "mod-1").unwrap();
        assert!(!metadata_dir.exists());
        assert!(!archive_path.exists());
    }

    #[test]
    fn uninstall_mod_not_found() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let result = index.uninstall_mod(&dir, "nonexistent");
        assert!(matches!(result.unwrap_err(), LibraryError::ModNotFound(_)));
    }

    // -----------------------------------------------------------------------
    // Profile management
    // -----------------------------------------------------------------------

    #[test]
    fn create_profile_success() {
        let dir = storage_dir();
        let mut index = make_index_with_mods(&["mod-1", "mod-2"]);

        let profile = index.create_profile(&dir, "Ranked".to_string()).unwrap();

        assert_eq!(profile.name, "Ranked");
        assert_eq!(profile.slug.as_str(), "ranked");
        assert!(profile.enabled_mods.is_empty());
        assert_eq!(profile.mod_order, vec!["mod-1", "mod-2"]);
        assert_eq!(index.profiles.len(), 2);
    }

    #[test]
    fn create_profile_creates_directories() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let profile = index
            .create_profile(&dir, "Test Profile".to_string())
            .unwrap();

        let (overlay_dir, cache_dir) = resolve_profile_dirs(&dir, &profile.slug);
        assert!(overlay_dir.exists());
        assert!(cache_dir.exists());
    }

    #[test]
    fn create_profile_empty_name_fails() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let result = index.create_profile(&dir, "".to_string());
        assert!(matches!(
            result.unwrap_err(),
            LibraryError::ValidationFailed(_)
        ));
    }

    #[test]
    fn create_profile_whitespace_name_fails() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let result = index.create_profile(&dir, "   ".to_string());
        assert!(matches!(
            result.unwrap_err(),
            LibraryError::ValidationFailed(_)
        ));
    }

    #[test]
    fn create_profile_duplicate_name_fails() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        index.create_profile(&dir, "Ranked".to_string()).unwrap();
        let result = index.create_profile(&dir, "Ranked".to_string());
        assert!(matches!(
            result.unwrap_err(),
            LibraryError::ValidationFailed(_)
        ));
    }

    #[test]
    fn create_profile_duplicate_slug_fails() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        index
            .create_profile(&dir, "My Profile".to_string())
            .unwrap();
        // "My Profile!" would produce the same slug "my-profile"
        let result = index.create_profile(&dir, "My Profile!".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn delete_profile_success() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let profile = index.create_profile(&dir, "Ranked".to_string()).unwrap();

        index.delete_profile(&dir, &profile.id).unwrap();
        assert_eq!(index.profiles.len(), 1);
        assert_eq!(index.profiles[0].name, "Default");
    }

    #[test]
    fn delete_default_profile_fails() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let default_id = index.profiles[0].id.clone();
        let result = index.delete_profile(&dir, &default_id);
        assert!(matches!(
            result.unwrap_err(),
            LibraryError::ValidationFailed(_)
        ));
    }

    #[test]
    fn delete_active_profile_fails() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let ranked = index.create_profile(&dir, "Ranked".to_string()).unwrap();
        index.switch_profile(&ranked.id).unwrap();

        let result = index.delete_profile(&dir, &ranked.id);
        assert!(matches!(
            result.unwrap_err(),
            LibraryError::ValidationFailed(_)
        ));
    }

    #[test]
    fn delete_profile_cleans_filesystem() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let profile = index.create_profile(&dir, "Ranked".to_string()).unwrap();
        let profile_dir = dir.join("profiles").join(profile.slug.as_str());
        assert!(profile_dir.exists());

        index.delete_profile(&dir, &profile.id).unwrap();
        assert!(!profile_dir.exists());
    }

    #[test]
    fn switch_profile_success() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let ranked = index.create_profile(&dir, "Ranked".to_string()).unwrap();

        let switched = index.switch_profile(&ranked.id).unwrap();
        assert_eq!(switched.name, "Ranked");
        assert_eq!(index.active_profile_id, ranked.id);
    }

    #[test]
    fn switch_profile_updates_last_used() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let ranked = index.create_profile(&dir, "Ranked".to_string()).unwrap();
        let original_last_used = ranked.last_used;

        std::thread::sleep(std::time::Duration::from_millis(10));
        let switched = index.switch_profile(&ranked.id).unwrap();
        assert!(switched.last_used >= original_last_used);
    }

    #[test]
    fn switch_profile_not_found() {
        let mut index = LibraryIndex::default();
        let result = index.switch_profile("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn rename_profile_success() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let profile = index.create_profile(&dir, "Ranked".to_string()).unwrap();

        let renamed = index
            .rename_profile(&dir, &profile.id, "Competitive".to_string())
            .unwrap();

        assert_eq!(renamed.name, "Competitive");
        assert_eq!(renamed.slug.as_str(), "competitive");
    }

    #[test]
    fn rename_profile_moves_directory() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let profile = index.create_profile(&dir, "Ranked".to_string()).unwrap();
        let old_dir = dir.join("profiles").join("ranked");
        assert!(old_dir.exists());

        index
            .rename_profile(&dir, &profile.id, "Competitive".to_string())
            .unwrap();

        assert!(!old_dir.exists());
        assert!(dir.join("profiles").join("competitive").exists());
    }

    #[test]
    fn rename_default_profile_fails() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let default_id = index.profiles[0].id.clone();
        let result = index.rename_profile(&dir, &default_id, "Custom".to_string());
        assert!(matches!(
            result.unwrap_err(),
            LibraryError::ValidationFailed(_)
        ));
    }

    #[test]
    fn rename_profile_empty_name_fails() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        let profile = index.create_profile(&dir, "Ranked".to_string()).unwrap();
        let result = index.rename_profile(&dir, &profile.id, "".to_string());
        assert!(matches!(
            result.unwrap_err(),
            LibraryError::ValidationFailed(_)
        ));
    }

    #[test]
    fn rename_profile_duplicate_name_fails() {
        let dir = storage_dir();
        let mut index = LibraryIndex::default();
        index.create_profile(&dir, "Ranked".to_string()).unwrap();
        let second = index.create_profile(&dir, "Casual".to_string()).unwrap();
        let result = index.rename_profile(&dir, &second.id, "Ranked".to_string());
        assert!(matches!(
            result.unwrap_err(),
            LibraryError::ValidationFailed(_)
        ));
    }

    // -----------------------------------------------------------------------
    // Profile lookup helpers
    // -----------------------------------------------------------------------

    #[test]
    fn active_profile_found() {
        let index = LibraryIndex::default();
        let profile = index.active_profile().unwrap();
        assert_eq!(profile.name, "Default");
    }

    #[test]
    fn active_profile_corrupt_index() {
        let index = LibraryIndex {
            mods: Vec::new(),
            profiles: Vec::new(),
            active_profile_id: "nonexistent".to_string(),
        };
        assert!(matches!(
            index.active_profile().unwrap_err(),
            LibraryError::IndexCorrupt(_)
        ));
    }

    #[test]
    fn profile_by_id_found() {
        let index = LibraryIndex::default();
        let id = index.profiles[0].id.clone();
        let profile = index.profile_by_id(&id).unwrap();
        assert_eq!(profile.name, "Default");
    }

    #[test]
    fn profile_by_id_not_found() {
        let index = LibraryIndex::default();
        let result = index.profile_by_id("nope");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // invalidate_overlay
    // -----------------------------------------------------------------------

    #[test]
    fn invalidate_overlay_removes_file() {
        let dir = storage_dir();
        let index = LibraryIndex::default();
        let profile = index.active_profile().unwrap();
        let overlay_json = dir
            .join("profiles")
            .join(profile.slug.as_str())
            .join("overlay.json");

        fs::create_dir_all(overlay_json.parent().unwrap().as_std_path()).unwrap();
        fs::write(overlay_json.as_std_path(), b"{}").unwrap();
        assert!(overlay_json.exists());

        index.invalidate_overlay(&dir).unwrap();
        assert!(!overlay_json.exists());
    }

    #[test]
    fn invalidate_overlay_noop_when_no_file() {
        let dir = storage_dir();
        let index = LibraryIndex::default();
        // Need to create the profile dir so active_profile works
        index.invalidate_overlay(&dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // resolve_profile_dirs
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_profile_dirs_correct_paths() {
        let slug = ProfileSlug::from("ranked".to_string());
        let (overlay, cache) = resolve_profile_dirs(Utf8Path::new("/storage"), &slug);
        assert_eq!(
            overlay,
            Utf8PathBuf::from("/storage/profiles/ranked/overlay")
        );
        assert_eq!(cache, Utf8PathBuf::from("/storage/profiles/ranked/cache"));
    }

    // -----------------------------------------------------------------------
    // Serialization format
    // -----------------------------------------------------------------------

    #[test]
    fn library_index_serializes_camel_case() {
        let index = LibraryIndex::default();
        let json = serde_json::to_string(&index).unwrap();
        assert!(json.contains("activeProfileId"));
        assert!(json.contains("enabledMods"));
        assert!(json.contains("modOrder"));
        assert!(json.contains("layerStates"));
        assert!(json.contains("createdAt"));
        assert!(json.contains("lastUsed"));
    }

    #[test]
    fn mod_archive_format_serializes_lowercase() {
        let modpkg = serde_json::to_string(&ModArchiveFormat::Modpkg).unwrap();
        let fantome = serde_json::to_string(&ModArchiveFormat::Fantome).unwrap();
        assert_eq!(modpkg, "\"modpkg\"");
        assert_eq!(fantome, "\"fantome\"");
    }
}
