//! [`GameResolver`]: the merged `game` category as an `ltk_wad` resolver.
//!
//! Only compiled with the `wad` cargo feature, so callers that never touch a
//! WAD pull no WAD machinery in.

use ltk_wad::{PathResolver, WadHash};

use crate::{Category, HashtableSet};

/// Resolves WAD chunk hashes through a set's merged `game` category.
///
/// The bridge from embedded tables to [`ltk_wad`]: an extraction that should
/// name chunks from a mod's own tables wraps the set here and passes the
/// result wherever a [`PathResolver`] is asked for. `game` names are paths
/// relative to the WAD root in the xxh64 hash space, which is exactly what a
/// [`WadHash`] keys, so the categories line up by construction.
///
/// # Example
///
/// ```
/// use ltk_hashtable::{
///     Algorithm, Category, GameResolver, Hashtable, HashtableEntry, HashtableSet, KeyWidth,
/// };
/// use ltk_wad::PathResolver;
///
/// let entry = HashtableEntry::new(
///     "hashes/game.hashes.txt",
///     Category::Game,
///     Algorithm::Xxh64,
///     KeyWidth::new(64).unwrap(),
/// );
/// let table = Hashtable::from_names(["assets/custom/trail.tex"]).unwrap();
/// let set = HashtableSet::build([(entry, table)]);
///
/// let resolver = GameResolver::new(&set);
/// let hash = ltk_wad::WadHash::from("assets/custom/trail.tex");
/// assert_eq!(resolver.resolve(hash).as_deref(), Some("assets/custom/trail.tex"));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct GameResolver<'a> {
    set: &'a HashtableSet,
}

impl<'a> GameResolver<'a> {
    /// Resolve through the `game` category of `set`.
    ///
    /// The set's other categories are ignored: nothing in a WAD is keyed the
    /// way `binentries` or `binhashes` names are.
    pub fn new(set: &'a HashtableSet) -> Self {
        Self { set }
    }
}

impl PathResolver for GameResolver<'_> {
    fn resolve(&self, path_hash: WadHash) -> Option<String> {
        self.set
            .resolve_value(&Category::Game, path_hash.0)
            .map(str::to_owned)
    }

    fn is_known(&self, path_hash: WadHash) -> bool {
        self.set
            .resolve_value(&Category::Game, path_hash.0)
            .is_some()
    }
}
