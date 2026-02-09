//! Overlay state persistence for build caching.
//!
//! After a successful overlay build, an [`OverlayState`] is serialized to
//! `overlay.json` inside the overlay directory. On the next build, the builder
//! loads this file and compares it against the current configuration. If the
//! enabled mod list and game fingerprint match (and the overlay WAD files on
//! disk are still valid), the entire build is skipped.
//!
//! The state is deliberately simple — it tracks *what* was built, not *how*.
//! Any mismatch triggers a full rebuild. This avoids complex incremental
//! diffing logic while still providing a significant speedup for the common
//! case of "nothing changed since last build".

use crate::error::Result;
use camino::Utf8Path;
use serde::{Deserialize, Serialize};

/// Snapshot of the overlay build configuration, persisted as `overlay.json`.
///
/// Used to determine whether the existing overlay can be reused or needs rebuilding.
/// The comparison is strict: any change to the mod list (including reordering) or
/// game directory invalidates the state.
///
/// # JSON format
///
/// ```json
/// {
///   "version": 2,
///   "enabledMods": ["mod-a", "mod-b"],
///   "gameFingerprint": 1234567890
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    /// Schema version (current: `2`). Used for forward compatibility — if a
    /// future version changes the format, old overlays won't match.
    pub version: u32,

    /// Ordered list of enabled mod IDs at the time the overlay was built.
    /// Order matters because it determines conflict resolution.
    pub enabled_mods: Vec<String>,

    /// xxHash3 fingerprint of the game directory's WAD files.
    /// Changes when the game is patched (file sizes/timestamps differ).
    pub game_fingerprint: u64,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            version: 2,
            enabled_mods: Vec::new(),
            game_fingerprint: 0,
        }
    }
}

impl OverlayState {
    /// Create a new overlay state.
    ///
    /// # Arguments
    ///
    /// * `enabled_mods` - List of enabled mod IDs in order
    /// * `game_fingerprint` - Fingerprint of the game directory
    pub fn new(enabled_mods: Vec<String>, game_fingerprint: u64) -> Self {
        Self {
            version: 2,
            enabled_mods,
            game_fingerprint,
        }
    }

    /// Load overlay state from a file.
    ///
    /// Returns `Ok(None)` if the file doesn't exist.
    /// Returns `Ok(Some(state))` if the file exists and is valid.
    /// Returns `Err` if the file exists but cannot be parsed.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the overlay.json state file
    pub fn load(path: &Utf8Path) -> Result<Option<Self>> {
        if !path.as_std_path().exists() {
            return Ok(None);
        }

        let contents = std::fs::read_to_string(path.as_std_path())?;
        let state: Self = serde_json::from_str(&contents)?;
        Ok(Some(state))
    }

    /// Save overlay state to a file.
    ///
    /// Creates parent directories if needed.
    ///
    /// # Arguments
    ///
    /// * `path` - Path where the overlay.json state file should be written
    pub fn save(&self, path: &Utf8Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent.as_std_path())?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path.as_std_path(), contents)?;
        Ok(())
    }

    /// Check if this state matches the current configuration.
    ///
    /// Returns `true` if:
    /// - Version matches (currently 2)
    /// - Enabled mods list matches exactly (same IDs, same order)
    /// - Game fingerprint matches
    ///
    /// # Arguments
    ///
    /// * `enabled_mod_ids` - Current list of enabled mod IDs
    /// * `game_fingerprint` - Current game fingerprint
    pub fn matches(&self, enabled_mod_ids: &[String], game_fingerprint: u64) -> bool {
        self.version == 2
            && self.enabled_mods == enabled_mod_ids
            && self.game_fingerprint == game_fingerprint
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8Path;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_state() {
        let state = OverlayState::default();
        assert_eq!(state.version, 2);
        assert_eq!(state.enabled_mods.len(), 0);
        assert_eq!(state.game_fingerprint, 0);
    }

    #[test]
    fn test_new_state() {
        let mods = vec!["mod1".to_string(), "mod2".to_string()];
        let state = OverlayState::new(mods.clone(), 0x123456);

        assert_eq!(state.version, 2);
        assert_eq!(state.enabled_mods, mods);
        assert_eq!(state.game_fingerprint, 0x123456);
    }

    #[test]
    fn test_matches_identical() {
        let mods = vec!["mod1".to_string(), "mod2".to_string()];
        let state = OverlayState::new(mods.clone(), 0x123456);

        assert!(state.matches(&mods, 0x123456));
    }

    #[test]
    fn test_matches_different_mods() {
        let state = OverlayState::new(vec!["mod1".to_string()], 0x123456);
        let other_mods = vec!["mod2".to_string()];

        assert!(!state.matches(&other_mods, 0x123456));
    }

    #[test]
    fn test_matches_different_order() {
        let state = OverlayState::new(vec!["mod1".to_string(), "mod2".to_string()], 0x123456);
        let other_mods = vec!["mod2".to_string(), "mod1".to_string()];

        assert!(!state.matches(&other_mods, 0x123456));
    }

    #[test]
    fn test_matches_different_fingerprint() {
        let mods = vec!["mod1".to_string()];
        let state = OverlayState::new(mods.clone(), 0x123456);

        assert!(!state.matches(&mods, 0x789ABC));
    }

    #[test]
    fn test_save_and_load() {
        let temp = NamedTempFile::new().unwrap();
        let path = Utf8Path::from_path(temp.path()).unwrap();

        let mods = vec!["mod1".to_string(), "mod2".to_string()];
        let state = OverlayState::new(mods.clone(), 0x123456);

        // Save
        state.save(path).unwrap();

        // Load
        let loaded = OverlayState::load(path).unwrap().unwrap();
        assert_eq!(loaded.version, state.version);
        assert_eq!(loaded.enabled_mods, state.enabled_mods);
        assert_eq!(loaded.game_fingerprint, state.game_fingerprint);
    }

    #[test]
    fn test_load_nonexistent() {
        let temp = NamedTempFile::new().unwrap();
        let std_path = temp.path().with_extension("nonexistent");
        let path = Utf8Path::from_path(&std_path).unwrap();

        let loaded = OverlayState::load(path).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_load_invalid_json() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"{ invalid json }").unwrap();
        temp.flush().unwrap();

        let path = Utf8Path::from_path(temp.path()).unwrap();
        let result = OverlayState::load(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_serialization_format() {
        let state = OverlayState::new(vec!["mod1".to_string()], 0x123456);
        let json = serde_json::to_string(&state).unwrap();

        assert!(json.contains("\"version\":2"));
        assert!(json.contains("\"enabledMods\""));
        assert!(json.contains("\"gameFingerprint\""));
    }
}
