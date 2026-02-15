//! Overlay state persistence for build caching.
//!
//! After a successful overlay build, an [`OverlayState`] is serialized to
//! `overlay.json` inside the overlay directory. On the next build, the builder
//! loads this file and compares it against the current configuration:
//!
//! - **Exact match** (same version, mods, game fingerprint, and per-WAD
//!   fingerprints): the build is skipped entirely.
//! - **Incremental** (same version and game fingerprint, but different mods):
//!   only WADs whose override fingerprints changed are rebuilt.
//! - **Full rebuild** (version or game fingerprint mismatch): the overlay is
//!   wiped and rebuilt from scratch.

use crate::error::Result;
use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Current schema version. Bump this when the state format changes
/// incompatibly — any state file with a different version triggers a full
/// rebuild.
const CURRENT_VERSION: u32 = 3;

/// Snapshot of the overlay build configuration, persisted as `overlay.json`.
///
/// Used to determine whether the existing overlay can be reused, incrementally
/// updated, or needs a full rebuild.
///
/// # JSON format (v3)
///
/// ```json
/// {
///   "version": 3,
///   "enabledMods": ["mod-a", "mod-b"],
///   "gameFingerprint": 1234567890,
///   "wadFingerprints": {
///     "DATA/FINAL/Champions/Aatrox.wad.client": 9876543210
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    /// Schema version (current: `3`). Used for forward compatibility — if a
    /// future version changes the format, old overlays won't match.
    pub version: u32,

    /// Ordered list of enabled mod IDs at the time the overlay was built.
    /// Order matters because it determines conflict resolution.
    pub enabled_mods: Vec<String>,

    /// xxHash3 fingerprint of the game directory's WAD files.
    /// Changes when the game is patched (file sizes/timestamps differ).
    pub game_fingerprint: u64,

    /// Per-WAD override fingerprints from the last build.
    ///
    /// Key: relative WAD path (e.g. `"DATA/FINAL/Champions/Aatrox.wad.client"`).
    /// Value: deterministic hash of the overrides applied to that WAD.
    ///
    /// Used for incremental rebuilds — only WADs whose fingerprint changed
    /// need to be re-patched.
    #[serde(default)]
    pub wad_fingerprints: BTreeMap<String, u64>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            enabled_mods: Vec::new(),
            game_fingerprint: 0,
            wad_fingerprints: BTreeMap::new(),
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
    /// * `wad_fingerprints` - Per-WAD override fingerprints
    pub fn new(
        enabled_mods: Vec<String>,
        game_fingerprint: u64,
        wad_fingerprints: BTreeMap<String, u64>,
    ) -> Self {
        Self {
            version: CURRENT_VERSION,
            enabled_mods,
            game_fingerprint,
            wad_fingerprints,
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

    /// Check if this state is an exact match for the current configuration.
    ///
    /// Returns `true` if:
    /// - Version matches the current version (3)
    /// - Enabled mods list matches exactly (same IDs, same order)
    /// - Game fingerprint matches
    ///
    /// When this returns `true` and all WAD files exist on disk, the build can
    /// be skipped entirely.
    ///
    /// # Arguments
    ///
    /// * `enabled_mod_ids` - Current list of enabled mod IDs
    /// * `game_fingerprint` - Current game fingerprint
    pub fn matches(&self, enabled_mod_ids: &[String], game_fingerprint: u64) -> bool {
        self.version == CURRENT_VERSION
            && self.enabled_mods == enabled_mod_ids
            && self.game_fingerprint == game_fingerprint
    }

    /// Check if this state supports incremental rebuilding.
    ///
    /// Returns `true` if the state version and game fingerprint match the
    /// current build. Even if the enabled mods differ, an incremental build
    /// can compare per-WAD fingerprints and only rebuild what changed.
    ///
    /// Returns `false` if the state is from an older version or the game was
    /// patched, in which case a full rebuild is required.
    ///
    /// # Arguments
    ///
    /// * `game_fingerprint` - Current game fingerprint
    pub fn supports_incremental(&self, game_fingerprint: u64) -> bool {
        self.version == CURRENT_VERSION && self.game_fingerprint == game_fingerprint
    }

    /// Look up the fingerprint of a specific WAD from the previous build.
    ///
    /// # Arguments
    ///
    /// * `wad_relative_path` - Relative WAD path (e.g. `"DATA/FINAL/Champions/Aatrox.wad.client"`)
    pub fn wad_fingerprint(&self, wad_relative_path: &str) -> Option<u64> {
        self.wad_fingerprints.get(wad_relative_path).copied()
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
        assert_eq!(state.version, CURRENT_VERSION);
        assert_eq!(state.enabled_mods.len(), 0);
        assert_eq!(state.game_fingerprint, 0);
        assert!(state.wad_fingerprints.is_empty());
    }

    #[test]
    fn test_new_state() {
        let mods = vec!["mod1".to_string(), "mod2".to_string()];
        let state = OverlayState::new(mods.clone(), 0x123456, BTreeMap::new());

        assert_eq!(state.version, CURRENT_VERSION);
        assert_eq!(state.enabled_mods, mods);
        assert_eq!(state.game_fingerprint, 0x123456);
        assert!(state.wad_fingerprints.is_empty());
    }

    #[test]
    fn test_new_state_with_wad_fingerprints() {
        let mut wad_fps = BTreeMap::new();
        wad_fps.insert(
            "DATA/FINAL/Champions/Aatrox.wad.client".to_string(),
            0xDEADBEEF,
        );
        wad_fps.insert("DATA/FINAL/Maps/Map11.wad.client".to_string(), 0xCAFEBABE);

        let state = OverlayState::new(vec!["mod1".to_string()], 0x123, wad_fps);
        assert_eq!(state.wad_fingerprints.len(), 2);
        assert_eq!(
            state.wad_fingerprint("DATA/FINAL/Champions/Aatrox.wad.client"),
            Some(0xDEADBEEF)
        );
        assert_eq!(
            state.wad_fingerprint("DATA/FINAL/Maps/Map11.wad.client"),
            Some(0xCAFEBABE)
        );
        assert_eq!(state.wad_fingerprint("nonexistent"), None);
    }

    #[test]
    fn test_matches_identical() {
        let mods = vec!["mod1".to_string(), "mod2".to_string()];
        let state = OverlayState::new(mods.clone(), 0x123456, BTreeMap::new());

        assert!(state.matches(&mods, 0x123456));
    }

    #[test]
    fn test_matches_different_mods() {
        let state = OverlayState::new(vec!["mod1".to_string()], 0x123456, BTreeMap::new());
        let other_mods = vec!["mod2".to_string()];

        assert!(!state.matches(&other_mods, 0x123456));
    }

    #[test]
    fn test_matches_different_order() {
        let state = OverlayState::new(
            vec!["mod1".to_string(), "mod2".to_string()],
            0x123456,
            BTreeMap::new(),
        );
        let other_mods = vec!["mod2".to_string(), "mod1".to_string()];

        assert!(!state.matches(&other_mods, 0x123456));
    }

    #[test]
    fn test_matches_different_fingerprint() {
        let mods = vec!["mod1".to_string()];
        let state = OverlayState::new(mods.clone(), 0x123456, BTreeMap::new());

        assert!(!state.matches(&mods, 0x789ABC));
    }

    #[test]
    fn test_supports_incremental() {
        let state = OverlayState::new(vec!["mod1".to_string()], 0x123456, BTreeMap::new());

        // Same game fingerprint -> supports incremental
        assert!(state.supports_incremental(0x123456));
        // Different game fingerprint -> does not support incremental
        assert!(!state.supports_incremental(0x789ABC));
    }

    #[test]
    fn test_v2_deserialization_triggers_full_rebuild() {
        // A v2 state file (no wad_fingerprints) should still deserialize
        // but supports_incremental and matches should return false
        let v2_json = r#"{"version":2,"enabledMods":["mod1"],"gameFingerprint":1234}"#;
        let state: OverlayState = serde_json::from_str(v2_json).unwrap();

        assert_eq!(state.version, 2);
        assert!(state.wad_fingerprints.is_empty());
        assert!(!state.supports_incremental(1234));
        assert!(!state.matches(&[String::from("mod1")], 1234));
    }

    #[test]
    fn test_save_and_load() {
        let temp = NamedTempFile::new().unwrap();
        let path = Utf8Path::from_path(temp.path()).unwrap();

        let mut wad_fps = BTreeMap::new();
        wad_fps.insert("DATA/FINAL/test.wad.client".to_string(), 0xABC);

        let mods = vec!["mod1".to_string(), "mod2".to_string()];
        let state = OverlayState::new(mods.clone(), 0x123456, wad_fps);

        // Save
        state.save(path).unwrap();

        // Load
        let loaded = OverlayState::load(path).unwrap().unwrap();
        assert_eq!(loaded.version, state.version);
        assert_eq!(loaded.enabled_mods, state.enabled_mods);
        assert_eq!(loaded.game_fingerprint, state.game_fingerprint);
        assert_eq!(loaded.wad_fingerprints, state.wad_fingerprints);
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
        let state = OverlayState::new(vec!["mod1".to_string()], 0x123456, BTreeMap::new());
        let json = serde_json::to_string(&state).unwrap();

        assert!(json.contains("\"version\":3"));
        assert!(json.contains("\"enabledMods\""));
        assert!(json.contains("\"gameFingerprint\""));
        assert!(json.contains("\"wadFingerprints\""));
    }
}
