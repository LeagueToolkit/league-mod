use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::index::LibraryIndex;

/// Slugified profile name used as the filesystem directory name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileSlug(pub String);

impl ProfileSlug {
    pub fn from_name(name: &str) -> Option<Self> {
        let s = slug::slugify(name);
        if s.is_empty() {
            None
        } else {
            Some(Self(s))
        }
    }

    pub fn is_unique_in(&self, index: &LibraryIndex, exclude_id: Option<&str>) -> bool {
        !index
            .profiles
            .iter()
            .any(|p| p.slug == *self && exclude_id.is_none_or(|id| p.id != id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProfileSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for ProfileSlug {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// A mod profile for organizing different mod configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub slug: ProfileSlug,
    /// Mod IDs in overlay priority order (first = highest priority).
    pub enabled_mods: Vec<String>,
    /// Display order of all mods (enabled and disabled).
    pub mod_order: Vec<String>,
    /// Per-mod layer states: `mod_id → (layer_name → enabled)`.
    #[serde(default)]
    pub layer_states: HashMap<String, HashMap<String, bool>>,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
}

impl Profile {
    /// Find the correct insertion position in `enabled_mods` for a mod,
    /// preserving relative order from `mod_order`.
    pub fn insertion_position_for(&self, mod_id: &str) -> usize {
        if let Some(order_pos) = self.mod_order.iter().position(|id| id == mod_id) {
            self.enabled_mods
                .iter()
                .position(|id| {
                    self.mod_order
                        .iter()
                        .position(|oid| oid == id)
                        .is_none_or(|p| p > order_pos)
                })
                .unwrap_or(self.enabled_mods.len())
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_profile(mod_order: Vec<&str>, enabled_mods: Vec<&str>) -> Profile {
        Profile {
            id: "test-profile".to_string(),
            name: "Test".to_string(),
            slug: ProfileSlug::from("test".to_string()),
            enabled_mods: enabled_mods.into_iter().map(String::from).collect(),
            mod_order: mod_order.into_iter().map(String::from).collect(),
            layer_states: HashMap::new(),
            created_at: Utc::now(),
            last_used: Utc::now(),
        }
    }

    // -----------------------------------------------------------------------
    // ProfileSlug
    // -----------------------------------------------------------------------

    #[test]
    fn slug_from_normal_name() {
        let slug = ProfileSlug::from_name("My Profile").unwrap();
        assert_eq!(slug.as_str(), "my-profile");
    }

    #[test]
    fn slug_from_special_characters() {
        let slug = ProfileSlug::from_name("Ranked!! (2024)").unwrap();
        assert!(!slug.as_str().is_empty());
        assert!(!slug.as_str().contains('!'));
        assert!(!slug.as_str().contains('('));
    }

    #[test]
    fn slug_from_empty_name_returns_none() {
        assert!(ProfileSlug::from_name("").is_none());
    }

    #[test]
    fn slug_from_whitespace_only_returns_none() {
        assert!(ProfileSlug::from_name("   ").is_none());
    }

    #[test]
    fn slug_from_symbols_only_returns_none() {
        assert!(ProfileSlug::from_name("!!!").is_none());
    }

    #[test]
    fn slug_uniqueness_no_profiles() {
        let index = LibraryIndex {
            mods: Vec::new(),
            profiles: Vec::new(),
            active_profile_id: String::new(),
        };
        let slug = ProfileSlug::from_name("test").unwrap();
        assert!(slug.is_unique_in(&index, None));
    }

    #[test]
    fn slug_uniqueness_duplicate_detected() {
        let index = LibraryIndex {
            mods: Vec::new(),
            profiles: vec![make_profile(vec![], vec![])],
            active_profile_id: String::new(),
        };
        let slug = ProfileSlug::from("test".to_string());
        assert!(!slug.is_unique_in(&index, None));
    }

    #[test]
    fn slug_uniqueness_exclude_self() {
        let index = LibraryIndex {
            mods: Vec::new(),
            profiles: vec![make_profile(vec![], vec![])],
            active_profile_id: String::new(),
        };
        let slug = ProfileSlug::from("test".to_string());
        assert!(slug.is_unique_in(&index, Some("test-profile")));
    }

    #[test]
    fn slug_display() {
        let slug = ProfileSlug::from("my-profile".to_string());
        assert_eq!(format!("{}", slug), "my-profile");
    }

    // -----------------------------------------------------------------------
    // Profile::insertion_position_for
    // -----------------------------------------------------------------------

    #[test]
    fn insertion_position_empty_enabled_list() {
        let profile = make_profile(vec!["a", "b", "c"], vec![]);
        assert_eq!(profile.insertion_position_for("b"), 0);
    }

    #[test]
    fn insertion_position_first_in_order() {
        // mod_order: [a, b, c], enabled: [b, c]
        // inserting "a" (pos 0 in order) → should go before "b" (pos 1 in order) → position 0
        let profile = make_profile(vec!["a", "b", "c"], vec!["b", "c"]);
        assert_eq!(profile.insertion_position_for("a"), 0);
    }

    #[test]
    fn insertion_position_middle() {
        // mod_order: [a, b, c], enabled: [a, c]
        // inserting "b" (pos 1 in order) → should go before "c" (pos 2 in order) → position 1
        let profile = make_profile(vec!["a", "b", "c"], vec!["a", "c"]);
        assert_eq!(profile.insertion_position_for("b"), 1);
    }

    #[test]
    fn insertion_position_last() {
        // mod_order: [a, b, c], enabled: [a, b]
        // inserting "c" (pos 2 in order) → no enabled mod comes after → append → position 2
        let profile = make_profile(vec!["a", "b", "c"], vec!["a", "b"]);
        assert_eq!(profile.insertion_position_for("c"), 2);
    }

    #[test]
    fn insertion_position_mod_not_in_order() {
        let profile = make_profile(vec!["a", "b"], vec!["a"]);
        assert_eq!(profile.insertion_position_for("unknown"), 0);
    }

    #[test]
    fn insertion_position_preserves_order_with_gaps() {
        // mod_order: [a, b, c, d, e], enabled: [a, e]
        // inserting "c" → should go between "a" (pos 0) and "e" (pos 4) → position 1
        let profile = make_profile(vec!["a", "b", "c", "d", "e"], vec!["a", "e"]);
        assert_eq!(profile.insertion_position_for("c"), 1);
    }
}
