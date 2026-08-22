//! Overlay state persistence for build caching.
//!
//! After a successful overlay build, an [`OverlayState`] is serialized to
//! `overlay.json` inside the overlay directory. On the next build, the builder
//! loads this file and compares it against the current configuration:
//!
//! - **Exact match** (same version, mods, per-mod content fingerprints, game
//!   fingerprint, blocked WADs, and string-override locales): the build is
//!   skipped entirely.
//! - **Incremental** (same version and game fingerprint, but different mods or
//!   mod content): only WADs whose override fingerprints changed are rebuilt.
//! - **Full rebuild** (version or game fingerprint mismatch): the overlay is
//!   wiped and rebuilt from scratch.

use crate::error::{Error, Result};
use crate::linked_bins::LinkedBinOffender;
use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Current schema version. Bump this when the state format changes
/// incompatibly, or when build semantics change such that WADs on disk may no
/// longer match what a fresh build would produce - any state file with a
/// different version triggers a full rebuild.
const CURRENT_VERSION: u32 = 5;

/// Snapshot of the overlay build configuration, persisted as `overlay.json`.
///
/// Used to determine whether the existing overlay can be reused, incrementally
/// updated, or needs a full rebuild.
///
/// # JSON format (v5)
///
/// ```json
/// {
///   "version": 5,
///   "enabledMods": ["mod-a", "mod-b"],
///   "modFingerprints": {
///     "mod-a": 1122334455,
///     "mod-b": 5544332211
///   },
///   "gameFingerprint": 1234567890,
///   "blockedWads": ["scripts.wad.client"],
///   "wadFingerprints": {
///     "DATA/FINAL/Champions/Aatrox.wad.client": 9876543210
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    /// Schema version (current: `5`). Used for forward compatibility - if a
    /// future version changes the format, old overlays won't match.
    pub version: u32,

    /// Ordered list of enabled mod IDs at the time the overlay was built.
    /// Order matters because it determines conflict resolution.
    pub enabled_mods: Vec<String>,

    /// Per-mod content fingerprints at the time the overlay was built, from
    /// [`EnabledMod::cache_fingerprint`](crate::EnabledMod::cache_fingerprint).
    ///
    /// Mod IDs alone are not enough for the exact-match skip: mutable content
    /// sources (a workshop project directory, an archive replaced in place)
    /// keep their ID when their content changes. Comparing fingerprints in
    /// [`matches`](Self::matches) makes content edits invalidate the skip.
    ///
    /// Mods whose provider could not compute a fingerprint are absent from the
    /// map, which makes the exact-match comparison fail conservatively.
    #[serde(default)]
    pub mod_fingerprints: BTreeMap<String, u64>,

    /// xxHash3 fingerprint of the game directory's WAD files.
    /// Changes when the game is patched (file sizes/timestamps differ).
    pub game_fingerprint: u64,

    /// Sorted list of lowercased WAD filenames that were blocked from patching.
    /// Changes to this list (e.g. toggling TFT) trigger a rebuild.
    #[serde(default)]
    pub blocked_wads: Vec<String>,

    /// Sorted list of lowercased locales string overrides were applied to
    /// (empty when string overrides are disabled). Changing the target locales
    /// (e.g. switching the client language or toggling "all locales") must
    /// invalidate the exact-match skip, so it participates in [`matches`](Self::matches).
    #[serde(default)]
    pub string_override_locales: Vec<String>,

    /// Per-WAD override fingerprints from the last build.
    ///
    /// Key: relative WAD path (e.g. `"DATA/FINAL/Champions/Aatrox.wad.client"`).
    /// Value: deterministic hash of the overrides applied to that WAD.
    ///
    /// Used for incremental rebuilds - only WADs whose fingerprint changed
    /// need to be re-patched.
    #[serde(default)]
    pub wad_fingerprints: BTreeMap<String, u64>,

    /// Mods whose property-bins reference unresolved linked dependencies, as
    /// computed during the last build. Persisted so the exact-match skip path can
    /// re-surface the same advisory without recomputing.
    #[serde(default)]
    pub linked_bin_offenders: Vec<LinkedBinOffender>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            enabled_mods: Vec::new(),
            mod_fingerprints: BTreeMap::new(),
            game_fingerprint: 0,
            blocked_wads: Vec::new(),
            string_override_locales: Vec::new(),
            wad_fingerprints: BTreeMap::new(),
            linked_bin_offenders: Vec::new(),
        }
    }
}

impl OverlayState {
    /// Create a new overlay state.
    ///
    /// # Arguments
    ///
    /// * `enabled_mods` - List of enabled mod IDs in order
    /// * `mod_fingerprints` - Per-mod content fingerprints (mods without one are absent)
    /// * `game_fingerprint` - Fingerprint of the game directory
    /// * `blocked_wads` - Sorted list of lowercased blocked WAD filenames
    /// * `string_override_locales` - Sorted list of lowercased string-override target locales
    /// * `wad_fingerprints` - Per-WAD override fingerprints
    pub fn new(
        enabled_mods: Vec<String>,
        mod_fingerprints: BTreeMap<String, u64>,
        game_fingerprint: u64,
        blocked_wads: Vec<String>,
        string_override_locales: Vec<String>,
        wad_fingerprints: BTreeMap<String, u64>,
    ) -> Self {
        Self {
            version: CURRENT_VERSION,
            enabled_mods,
            mod_fingerprints,
            game_fingerprint,
            blocked_wads,
            string_override_locales,
            wad_fingerprints,
            linked_bin_offenders: Vec::new(),
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

        let contents = std::fs::read_to_string(path.as_std_path())
            .map_err(|source| Error::read(path, source))?;
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
            std::fs::create_dir_all(parent.as_std_path())
                .map_err(|source| Error::write(parent, source))?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path.as_std_path(), contents)
            .map_err(|source| Error::write(path, source))?;
        Ok(())
    }

    /// Check if this state is an exact match for the current configuration.
    ///
    /// Returns `true` if:
    /// - Version matches the current version (5)
    /// - Enabled mods list matches exactly (same IDs, same order)
    /// - Per-mod content fingerprints match exactly
    /// - Game fingerprint matches
    /// - Blocked WADs list matches
    /// - String-override target locales match
    ///
    /// When this returns `true` and all WAD files exist on disk, the build can
    /// be skipped entirely.
    ///
    /// # Arguments
    ///
    /// * `enabled_mod_ids` - Current list of enabled mod IDs
    /// * `mod_fingerprints` - Current per-mod content fingerprints, or `None`
    ///   when any enabled mod's provider could not compute one. `None` never
    ///   matches - without a complete fingerprint set there is no way to prove
    ///   the mod content is unchanged, so the skip must not be taken.
    /// * `game_fingerprint` - Current game fingerprint
    /// * `blocked_wads` - Current sorted list of blocked WAD filenames
    /// * `string_override_locales` - Current sorted list of string-override target locales
    pub fn matches(
        &self,
        enabled_mod_ids: &[String],
        mod_fingerprints: Option<&BTreeMap<String, u64>>,
        game_fingerprint: u64,
        blocked_wads: &[String],
        string_override_locales: &[String],
    ) -> bool {
        self.version == CURRENT_VERSION
            && self.enabled_mods == enabled_mod_ids
            && mod_fingerprints.is_some_and(|fps| &self.mod_fingerprints == fps)
            && self.game_fingerprint == game_fingerprint
            && self.blocked_wads == blocked_wads
            && self.string_override_locales == string_override_locales
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

    fn fps(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

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
        let fingerprints = fps(&[("mod1", 0xA1), ("mod2", 0xB2)]);
        let state = OverlayState::new(
            mods.clone(),
            fingerprints.clone(),
            0x123456,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );

        assert_eq!(state.version, CURRENT_VERSION);
        assert_eq!(state.enabled_mods, mods);
        assert_eq!(state.mod_fingerprints, fingerprints);
        assert_eq!(state.game_fingerprint, 0x123456);
        assert!(state.blocked_wads.is_empty());
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

        let state = OverlayState::new(
            vec!["mod1".to_string()],
            BTreeMap::new(),
            0x123,
            Vec::new(),
            Vec::new(),
            wad_fps,
        );
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
        let fingerprints = fps(&[("mod1", 0xA1), ("mod2", 0xB2)]);
        let state = OverlayState::new(
            mods.clone(),
            fingerprints.clone(),
            0x123456,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );

        assert!(state.matches(&mods, Some(&fingerprints), 0x123456, &[], &[]));
    }

    #[test]
    fn test_matches_different_mods() {
        let state = OverlayState::new(
            vec!["mod1".to_string()],
            BTreeMap::new(),
            0x123456,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );
        let other_mods = vec!["mod2".to_string()];

        assert!(!state.matches(&other_mods, Some(&BTreeMap::new()), 0x123456, &[], &[]));
    }

    #[test]
    fn test_matches_different_order() {
        let state = OverlayState::new(
            vec!["mod1".to_string(), "mod2".to_string()],
            BTreeMap::new(),
            0x123456,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );
        let other_mods = vec!["mod2".to_string(), "mod1".to_string()];

        assert!(!state.matches(&other_mods, Some(&BTreeMap::new()), 0x123456, &[], &[]));
    }

    #[test]
    fn test_matches_different_fingerprint() {
        let mods = vec!["mod1".to_string()];
        let state = OverlayState::new(
            mods.clone(),
            BTreeMap::new(),
            0x123456,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );

        assert!(!state.matches(&mods, Some(&BTreeMap::new()), 0x789ABC, &[], &[]));
    }

    #[test]
    fn test_matches_different_blocked_wads() {
        let mods = vec!["mod1".to_string()];
        let blocked = vec!["map22.wad.client".to_string()];
        let state = OverlayState::new(
            mods.clone(),
            BTreeMap::new(),
            0x123456,
            blocked,
            Vec::new(),
            BTreeMap::new(),
        );

        // Different blocked_wads should not match
        assert!(!state.matches(&mods, Some(&BTreeMap::new()), 0x123456, &[], &[]));
        // Same blocked_wads should match
        assert!(state.matches(
            &mods,
            Some(&BTreeMap::new()),
            0x123456,
            &["map22.wad.client".to_string()],
            &[]
        ));
    }

    #[test]
    fn test_matches_mod_fingerprints() {
        let mods = vec!["workshop:proj".to_string()];
        let fingerprints = fps(&[("workshop:proj", 0xAAAA)]);
        let state = OverlayState::new(
            mods.clone(),
            fingerprints.clone(),
            0x123456,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );

        assert!(state.matches(&mods, Some(&fingerprints), 0x123456, &[], &[]));

        // Same mod ID with changed content must invalidate the exact-match skip.
        let changed = fps(&[("workshop:proj", 0xBBBB)]);
        assert!(!state.matches(&mods, Some(&changed), 0x123456, &[], &[]));

        // Unknown current fingerprints can never prove content is unchanged.
        assert!(!state.matches(&mods, None, 0x123456, &[], &[]));
    }

    #[test]
    fn test_matches_incomplete_stored_fingerprints() {
        // State built when a provider couldn't fingerprint: the mod is absent
        // from the stored map, so even a complete current set must not match.
        let mods = vec!["mod1".to_string()];
        let state = OverlayState::new(
            mods.clone(),
            BTreeMap::new(),
            0x123456,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );

        let current = fps(&[("mod1", 0xAAAA)]);
        assert!(!state.matches(&mods, Some(&current), 0x123456, &[], &[]));
    }

    #[test]
    fn test_supports_incremental() {
        let state = OverlayState::new(
            vec!["mod1".to_string()],
            BTreeMap::new(),
            0x123456,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );

        // Same game fingerprint -> supports incremental
        assert!(state.supports_incremental(0x123456));
        // Different game fingerprint -> does not support incremental
        assert!(!state.supports_incremental(0x789ABC));
    }

    #[test]
    fn test_old_version_deserialization_triggers_full_rebuild() {
        // A v3 state file (no blocked_wads) should still deserialize
        // but supports_incremental and matches should return false
        let v3_json = r#"{"version":3,"enabledMods":["mod1"],"gameFingerprint":1234}"#;
        let state: OverlayState = serde_json::from_str(v3_json).unwrap();

        assert_eq!(state.version, 3);
        assert!(state.blocked_wads.is_empty());
        assert!(state.wad_fingerprints.is_empty());
        assert!(!state.supports_incremental(1234));
        assert!(!state.matches(
            &[String::from("mod1")],
            Some(&BTreeMap::new()),
            1234,
            &[],
            &[]
        ));
    }

    #[test]
    fn test_save_and_load() {
        let temp = NamedTempFile::new().unwrap();
        let path = Utf8Path::from_path(temp.path()).unwrap();

        let mut wad_fps = BTreeMap::new();
        wad_fps.insert("DATA/FINAL/test.wad.client".to_string(), 0xABC);

        let mods = vec!["mod1".to_string(), "mod2".to_string()];
        let fingerprints = fps(&[("mod1", 0xA1), ("mod2", 0xB2)]);
        let state = OverlayState::new(
            mods.clone(),
            fingerprints,
            0x123456,
            Vec::new(),
            Vec::new(),
            wad_fps,
        );

        // Save
        state.save(path).unwrap();

        // Load
        let loaded = OverlayState::load(path).unwrap().unwrap();
        assert_eq!(loaded.version, state.version);
        assert_eq!(loaded.enabled_mods, state.enabled_mods);
        assert_eq!(loaded.mod_fingerprints, state.mod_fingerprints);
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
        let state = OverlayState::new(
            vec!["mod1".to_string()],
            BTreeMap::new(),
            0x123456,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );
        let json = serde_json::to_string(&state).unwrap();

        assert!(json.contains("\"version\":5"));
        assert!(json.contains("\"enabledMods\""));
        assert!(json.contains("\"modFingerprints\""));
        assert!(json.contains("\"gameFingerprint\""));
        assert!(json.contains("\"blockedWads\""));
        assert!(json.contains("\"stringOverrideLocales\""));
        assert!(json.contains("\"wadFingerprints\""));
    }

    #[test]
    fn test_matches_different_string_override_locales() {
        let mods = vec!["mod1".to_string()];
        let locales = vec!["en_us".to_string()];
        let state = OverlayState::new(
            mods.clone(),
            BTreeMap::new(),
            0x123456,
            Vec::new(),
            locales.clone(),
            BTreeMap::new(),
        );

        let no_fps = BTreeMap::new();
        assert!(state.matches(&mods, Some(&no_fps), 0x123456, &[], &locales));
        // Toggling the target locales must invalidate the exact-match skip.
        assert!(!state.matches(&mods, Some(&no_fps), 0x123456, &[], &[]));
        assert!(!state.matches(&mods, Some(&no_fps), 0x123456, &[], &["ko_kr".to_string()]));
    }

    #[test]
    fn test_v4_state_triggers_full_rebuild() {
        // A v4 state file (no modFingerprints) deserializes with an empty map,
        // and the version bump makes both the exact-match skip and the
        // incremental path reject it - one clean rebuild on upgrade.
        let mods = vec!["mod1".to_string()];
        let v4_json = r#"{"version":4,"enabledMods":["mod1"],"gameFingerprint":1234}"#;
        let old: OverlayState = serde_json::from_str(v4_json).unwrap();

        assert!(old.mod_fingerprints.is_empty());
        assert!(!old.matches(&mods, Some(&BTreeMap::new()), 1234, &[], &[]));
        assert!(!old.supports_incremental(1234));
    }
}
