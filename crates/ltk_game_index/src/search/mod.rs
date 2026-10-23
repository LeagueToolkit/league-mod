//! Search over every bin object of an installation, and the object tree.
//!
//! A [`TermIndex`] holds every declaration with every term it holds, keyed by hash. A set of
//! [`NameSheets`] holds a consumer's hash tables as substring-searchable sheets. A
//! [`GameSearch`] pairs the two: a query resolves to hashes through the sheets and to
//! declarations through the term index.

mod bytes;
mod intern;
mod names;
mod order;
mod query;
mod scan;
mod sheet;
mod term_index;
mod terms;
mod tree;

use std::borrow::Cow;
use std::fmt;
use std::sync::OnceLock;

pub use names::{NAME_CACHE_FORMAT_VERSION, NameSheets, NameSource, NameTable};
pub use order::{natural_cmp, path_cmp};
pub use query::{
    DEFAULT_LIMIT, DeclarationMatches, Hit, KindCount, Match, MatchedTerm, Matches, Query,
    SearchResult,
};
pub use sheet::{Occurrence, Sheet};
pub use term_index::{TERM_CACHE_FORMAT_VERSION, TermIndex, TermStats};
pub use terms::{TermKind, TermKinds};
pub use tree::{DEFAULT_LISTING_LIMIT, Dir, ListOptions, Listing, ObjectNode, TreeEntry};

/// A term index and the name sheets that resolve its hashes.
///
/// The hash lookups, the scan tables, the standing order of hits and the object tree's sorted
/// paths are cut from the sheets on first use, or by [`prepare`](Self::prepare), and kept
/// until [`set_names`](Self::set_names).
pub struct GameSearch {
    terms: TermIndex,
    names: NameSheets,
    /// Per table, `(hash, entry)` in ascending hash order.
    lookups: [OnceLock<Vec<(u64, u32)>>; 5],
    /// Per table, the sheet a query scans and the posting keys of its entries.
    scans: [OnceLock<scan::ScanTable>; 5],
    /// The standing order of hits.
    order: OnceLock<query::Standing>,
    paths: OnceLock<tree::Paths>,
}

impl GameSearch {
    /// A search over `terms` with names from `names`.
    #[must_use]
    pub fn new(terms: TermIndex, names: NameSheets) -> Self {
        Self {
            terms,
            names,
            lookups: Default::default(),
            scans: Default::default(),
            order: OnceLock::new(),
            paths: OnceLock::new(),
        }
    }

    /// Cuts every structure a query, a page or a listing reads on first use.
    ///
    /// A consumer that builds the search in the background calls it there. The first query
    /// then pays for no cut.
    pub fn prepare(&self) {
        #[cfg(feature = "rayon")]
        {
            rayon::scope(|scope| {
                for table in NameTable::ALL {
                    scope.spawn(move |_| {
                        self.scan(table);
                        self.lookup(table, 0);
                    });
                }
            });
        }
        #[cfg(not(feature = "rayon"))]
        for table in NameTable::ALL {
            self.scan(table);
            self.lookup(table, 0);
        }
        self.order();
        self.paths();
    }

    /// The term index.
    #[must_use]
    pub fn terms(&self) -> &TermIndex {
        &self.terms
    }

    /// The name sheets.
    #[must_use]
    pub fn names(&self) -> &NameSheets {
        &self.names
    }

    /// Replaces the name sheets. The term index and its ids are unchanged.
    pub fn set_names(&mut self, names: NameSheets) {
        self.names = names;
        self.lookups = Default::default();
        self.scans = Default::default();
        self.order = OnceLock::new();
        self.paths = OnceLock::new();
    }

    /// The term index and the name sheets.
    #[must_use]
    pub fn into_parts(self) -> (TermIndex, NameSheets) {
        (self.terms, self.names)
    }

    /// The name `table` gives `hash`, as the table writes it.
    #[must_use]
    pub fn name(&self, table: NameTable, hash: u64) -> Option<Cow<'_, str>> {
        let entry = self.lookup(table, hash)?;
        Some(self.names.sheet(table).entry(entry))
    }

    /// The entry of `table`'s sheet naming `hash`.
    fn lookup(&self, table: NameTable, hash: u64) -> Option<u32> {
        let lookup = self.lookups[table as usize].get_or_init(|| {
            let sheet = self.names.sheet(table);
            let mut lookup: Vec<(u64, u32)> = sheet
                .values()
                .iter()
                .enumerate()
                .map(|(entry, &hash)| (hash, entry as u32))
                .collect();
            lookup.sort_unstable();
            lookup.dedup_by_key(|&mut (hash, _)| hash);
            lookup
        });
        let at = lookup.binary_search_by_key(&hash, |&(hash, _)| hash).ok()?;
        Some(lookup[at].1)
    }

    /// The scan table of `table`.
    fn scan(&self, table: NameTable) -> &scan::ScanTable {
        self.scans[table.ordinal()].get_or_init(|| scan::ScanTable::cut(self, table))
    }

    /// The sheet a query scans for `table`.
    fn scan_sheet(&self, table: NameTable) -> &Sheet {
        self.scan(table).sheet(self.names.sheet(table))
    }

    /// The standing order of hits.
    fn order(&self) -> &query::Standing {
        self.order.get_or_init(|| query::Standing::cut(self))
    }

    fn paths(&self) -> &tree::Paths {
        self.paths.get_or_init(|| tree::Paths::cut(self))
    }
}

impl fmt::Debug for GameSearch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GameSearch")
            .field("declarations", &self.terms.objects().rows().len())
            .field("stats", self.terms.stats())
            .field("fingerprint", &self.terms.fingerprint())
            .field(
                "names",
                &NameTable::ALL.map(|table| self.names.sheet(table).len()),
            )
            .finish_non_exhaustive()
    }
}
