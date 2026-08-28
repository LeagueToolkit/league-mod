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
use crate::wad_builder::{SourceWadIdentity, WadTailLayout};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Current schema version. Bump this when the state format changes
/// incompatibly, or when build semantics change such that WADs on disk may no
/// longer match what a fresh build would produce - any state file with a
/// different version triggers a full rebuild.
const CURRENT_VERSION: u32 = 6;

/// What one overlay WAD on disk is, so a later build can rebuild it in place.
///
/// A record is a *hint*, never a fact. Every field is re-verified against the
/// game WAD and the overlay file before the in-place path is taken, and any
/// mismatch costs a full rebuild of that WAD rather than a wrong file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WadLayoutRecord {
    /// The game WAD this overlay was built from.
    pub source: SourceWadIdentity,

    /// Where the copied source region and the override tail sit in the file.
    pub layout: WadTailLayout,

    /// `path_hash -> content_hash` for every override currently in the tail.
    ///
    /// Comparing this against the next build's override set is what splits it
    /// into overrides whose compressed bytes can be lifted straight out of the
    /// old tail and overrides that have to be resolved and compressed again.
    pub overrides: BTreeMap<u64, u64>,
}

/// Snapshot of the overlay build configuration, persisted as `overlay.json`.
///
/// Used to determine whether the existing overlay can be reused, incrementally
/// updated, or needs a full rebuild.
///
/// # JSON format (v6)
///
/// ```json
/// {
///   "version": 6,
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
    /// Schema version (current: `6`). Used for forward compatibility - if a
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

    /// Per-WAD [layout records](WadLayoutRecord) from the last build, keyed by
    /// relative WAD path like [`wad_fingerprints`](Self::wad_fingerprints).
    ///
    /// A WAD with a record here can be rebuilt by rewriting its tail alone; one
    /// without takes the full-rebuild path.
    #[serde(default)]
    pub wad_layouts: BTreeMap<String, WadLayoutRecord>,

    /// WADs a build was part-way through rewriting in place.
    ///
    /// Written before the first byte is touched and cleared once the rewrites
    /// succeed, so a build killed mid-rewrite leaves its WADs marked and the
    /// next build rebuilds them in full. The marking is batched across the whole
    /// build rather than per WAD: over-invalidating costs a few extra full
    /// rebuilds, which is the designed fallback anyway, and keeps serialized
    /// state writes out of the parallel patch loop.
    #[serde(default)]
    pub dirty_wads: BTreeSet<String>,
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
            wad_layouts: BTreeMap::new(),
            dirty_wads: BTreeSet::new(),
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
            wad_layouts: BTreeMap::new(),
            dirty_wads: BTreeSet::new(),
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

    /// Save overlay state to a file, atomically.
    ///
    /// Creates parent directories if needed, writes a sibling `.tmp` file and
    /// renames it into place, so a crash mid-write leaves the previous state
    /// intact rather than a truncated file. Incremental rebuilds trust what
    /// this file says about WADs already on disk, so a torn one would have to
    /// be caught rather than believed.
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
        let tmp_path = Utf8PathBuf::from(format!("{path}.tmp"));
        std::fs::write(tmp_path.as_std_path(), contents)
            .map_err(|source| Error::write(&tmp_path, source))?;

        match std::fs::rename(tmp_path.as_std_path(), path.as_std_path()) {
            Ok(()) => Ok(()),
            Err(source) => {
                let _ = std::fs::remove_file(tmp_path.as_std_path());
                Err(Error::write(path, source))
            }
        }
    }

    /// Check if this state is an exact match for the current configuration.
    ///
    /// Returns `true` if:
    /// - Version matches the current version (6)
    /// - No WAD is marked dirty by an interrupted rewrite
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
            // A WAD an interrupted build was rewriting may be torn, so no state
            // carrying dirty flags can prove the overlay on disk is up to date.
            && self.dirty_wads.is_empty()
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

    /// The layout of a WAD this state can vouch for, if it can vouch for one.
    ///
    /// `None` for a WAD with no record, and for one the previous build was
    /// interrupted while rewriting - both take the full-rebuild path.
    ///
    /// # Arguments
    ///
    /// * `wad_relative_path` - Relative WAD path (e.g. `"DATA/FINAL/Champions/Aatrox.wad.client"`)
    pub fn wad_layout(&self, wad_relative_path: &str) -> Option<&WadLayoutRecord> {
        if self.dirty_wads.contains(wad_relative_path) {
            return None;
        }
        self.wad_layouts.get(wad_relative_path)
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

    /// A crash mid-save must leave the previous state readable, so the write
    /// goes through a sibling temp file that is renamed into place.
    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8Path::from_path(dir.path())
            .unwrap()
            .join("overlay.json");

        OverlayState::default().save(&path).unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(leftovers, vec!["overlay.json"], "{leftovers:?}");
        assert!(OverlayState::load(&path).unwrap().is_some());
    }

    /// Saving over an existing state file replaces it rather than failing on
    /// the rename, which Windows would do without replace-existing semantics.
    #[test]
    fn save_replaces_an_existing_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8Path::from_path(dir.path())
            .unwrap()
            .join("overlay.json");

        let first = OverlayState::new(
            vec!["mod1".to_string()],
            BTreeMap::new(),
            1,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );
        first.save(&path).unwrap();

        let second = OverlayState::new(
            vec!["mod2".to_string()],
            BTreeMap::new(),
            2,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        );
        second.save(&path).unwrap();

        let loaded = OverlayState::load(&path).unwrap().unwrap();
        assert_eq!(loaded.enabled_mods, vec!["mod2".to_string()]);
        assert_eq!(loaded.game_fingerprint, 2);
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

        assert!(json.contains("\"version\":6"));
        assert!(json.contains("\"enabledMods\""));
        assert!(json.contains("\"modFingerprints\""));
        assert!(json.contains("\"gameFingerprint\""));
        assert!(json.contains("\"blockedWads\""));
        assert!(json.contains("\"stringOverrideLocales\""));
        assert!(json.contains("\"wadFingerprints\""));
        assert!(json.contains("\"wadLayouts\""));
        assert!(json.contains("\"dirtyWads\""));
    }

    fn layout_record(overrides: &[(u64, u64)]) -> WadLayoutRecord {
        WadLayoutRecord {
            source: SourceWadIdentity {
                len: 4096,
                mtime: 1_700_000_000_000_000_000,
                toc_hash: 0x70C_0F17,
            },
            layout: WadTailLayout {
                data_region_offset: 500,
                offset_delta: 0,
                tail_offset: 4000,
                toc_capacity: 7,
            },
            overrides: overrides.iter().copied().collect(),
        }
    }

    /// A layout record survives a save/load round trip, integer map keys and
    /// all - the in-place rebuild reads every one of these fields back.
    #[test]
    fn layout_records_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8Path::from_path(dir.path())
            .unwrap()
            .join("overlay.json");

        let mut state = OverlayState::default();
        let record = layout_record(&[(0xAAAA, 0x1111), (0xBBBB, 0x2222)]);
        state.wad_layouts.insert(
            "DATA/FINAL/Champions/Ahri.wad.client".to_string(),
            record.clone(),
        );
        state.save(&path).unwrap();

        let loaded = OverlayState::load(&path).unwrap().unwrap();
        assert_eq!(
            loaded.wad_layout("DATA/FINAL/Champions/Ahri.wad.client"),
            Some(&record)
        );
    }

    /// A WAD a previous build was interrupted while rewriting must not be
    /// trusted, even though its record is still there.
    #[test]
    fn a_dirty_wad_has_no_usable_layout() {
        let mut state = OverlayState::default();
        state
            .wad_layouts
            .insert("wad".to_string(), layout_record(&[(1, 2)]));
        assert!(state.wad_layout("wad").is_some());

        state.dirty_wads.insert("wad".to_string());
        assert!(
            state.wad_layout("wad").is_none(),
            "a WAD left dirty by a killed build must fall back to a full rebuild"
        );
    }

    /// A v5 state file deserializes, but its version alone forces one clean
    /// rebuild - which is also what migrates every WAD to the tail layout.
    #[test]
    fn test_v5_state_triggers_full_rebuild() {
        let v5_json = r#"{"version":5,"enabledMods":["mod1"],"gameFingerprint":1234}"#;
        let old: OverlayState = serde_json::from_str(v5_json).unwrap();

        assert!(old.wad_layouts.is_empty());
        assert!(old.dirty_wads.is_empty());
        assert!(!old.supports_incremental(1234));
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
