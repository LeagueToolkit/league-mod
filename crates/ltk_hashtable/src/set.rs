//! [`HashtableSet`]: the merged, per-category lookup.

use std::collections::HashMap;

use crate::{Category, Hashtable, HashtableEntry, Key, KeyWidth};

/// Two different canonical names sharing one key - a packing error.
///
/// A duplicate (the same canonical name twice) is not a collision; readers
/// keep its first occurrence silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collision {
    /// The category both names were merged into.
    pub category: Category,
    /// The key both names truncate to.
    pub key: Key,
    /// The name that was kept.
    pub first: String,
    /// The name that became unresolvable.
    pub second: String,
}

/// Every declared table merged into one lookup per category.
///
/// Merging is the standard's: tables in manifest order, lines in file order,
/// first occurrence of a key wins. An entry whose category or algorithm is
/// unknown is skipped for lookup (its keys cannot mean anything here) but is
/// never anyone's to drop - preserving it is the container's job.
#[derive(Debug, Clone, Default)]
pub struct HashtableSet {
    names: HashMap<Category, HashMap<Key, String>>,
    /// The key widths each category's resolvable tables declared, widest
    /// first, so a raw hash value can be truncated the ways a lookup needs.
    widths: HashMap<Category, Vec<KeyWidth>>,
    collisions: Vec<Collision>,
}

impl HashtableSet {
    /// Merge `tables` in manifest order.
    pub fn build(tables: impl IntoIterator<Item = (HashtableEntry, Hashtable)>) -> Self {
        let mut names: HashMap<Category, HashMap<Key, String>> = HashMap::new();
        let mut widths: HashMap<Category, Vec<KeyWidth>> = HashMap::new();
        let mut collisions = Vec::new();
        for (entry, table) in tables {
            let merged = names.entry(entry.category().clone()).or_default();
            let mut keyed_any = false;
            for name in table.names() {
                let Some(key) = Key::of(name, entry.algorithm(), entry.width()) else {
                    continue;
                };
                keyed_any = true;
                match merged.get(&key) {
                    None => {
                        merged.insert(key, name.to_owned());
                    }
                    Some(kept) if !kept.eq_ignore_ascii_case(name) => {
                        // The same pair recurring is one collision, not many.
                        let seen = collisions.iter().any(|collision: &Collision| {
                            collision.key == key
                                && collision.category == *entry.category()
                                && collision.second.eq_ignore_ascii_case(name)
                        });
                        if !seen {
                            collisions.push(Collision {
                                category: entry.category().clone(),
                                key,
                                first: kept.clone(),
                                second: name.to_owned(),
                            });
                        }
                    }
                    Some(_) => {}
                }
            }
            if keyed_any {
                let seen = widths.entry(entry.category().clone()).or_default();
                if !seen.contains(&entry.width()) {
                    seen.push(entry.width());
                }
            }
        }
        for seen in widths.values_mut() {
            seen.sort_unstable_by_key(|width| std::cmp::Reverse(width.bits()));
        }
        Self {
            names,
            widths,
            collisions,
        }
    }

    /// The collisions merging ran into, in merge order.
    ///
    /// A pack must fail on any; a reader resolves what it can and should warn.
    pub fn collisions(&self) -> &[Collision] {
        &self.collisions
    }

    /// The name behind `key` in `category`, if any table holds one.
    pub fn resolve(&self, category: &Category, key: Key) -> Option<&str> {
        self.names.get(category)?.get(&key).map(String::as_str)
    }

    /// The name behind a raw hash value in `category`, if any table holds one.
    ///
    /// For a caller holding a hash another crate computed - a WAD chunk's
    /// path hash, say - rather than a [`Key`]. The value is truncated to each
    /// key width the category's tables declared, widest first, and the first
    /// name found is the answer; a category whose tables all declare one
    /// width (the common case) pays for one lookup.
    pub fn resolve_value(&self, category: &Category, value: u64) -> Option<&str> {
        self.widths
            .get(category)?
            .iter()
            .find_map(|&width| self.resolve(category, Key::from_value(value, width)))
    }
}
