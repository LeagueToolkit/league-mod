//! Query resolution: a text, through the name sheets and the strings sheet, to declarations.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;
use std::sync::OnceLock;

use ltk_hash::WadHash;
use memchr::memmem;
use serde::{Deserialize, Serialize};

use super::order::natural_cmp;
use super::sheet::finder;
use super::{GameSearch, NameTable, Sheet, TermKind, TermKinds};
use crate::{Declaration, DeclarationId};

/// The default [`Query::limit`].
pub const DEFAULT_LIMIT: u32 = 500;

/// Postings pushed between two polls of [`Query::called_off`].
const POLL_INTERVAL: usize = 4096;

/// A text, the kinds a hit may match through, a page and a cancellation.
///
/// Built as `Query::new(text)` with fields assigned.
#[derive(Clone)]
#[non_exhaustive]
pub struct Query<'a> {
    /// The text. Trimmed and ASCII-lowercased before it is resolved.
    pub text: &'a str,
    /// The kinds a hit may match through. [`TermKinds::ALL`] by default.
    pub kinds: TermKinds,
    /// The index of the first hit of the page. Zero by default.
    pub offset: u32,
    /// Hits per page. [`DEFAULT_LIMIT`] by default.
    pub limit: u32,
    /// Polled before each sheet and every 4,096 postings. `true` stops the query.
    pub called_off: Option<&'a (dyn Fn() -> bool + Sync)>,
}

impl<'a> Query<'a> {
    /// A query for `text` over every kind, first page, default limit, never called off.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            kinds: TermKinds::ALL,
            offset: 0,
            limit: DEFAULT_LIMIT,
            called_off: None,
        }
    }
}

impl fmt::Debug for Query<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Query")
            .field("text", &self.text)
            .field("kinds", &self.kinds)
            .field("offset", &self.offset)
            .field("limit", &self.limit)
            .field("called_off", &self.called_off.is_some())
            .finish()
    }
}

/// The number of declarations matching through one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct KindCount {
    /// The kind.
    pub kind: TermKind,
    /// Declarations with at least one match of `kind`.
    pub count: u32,
}

/// One term a declaration matched through.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Match {
    /// The kind of the term.
    pub kind: TermKind,
    /// The term.
    pub term: MatchedTerm,
    /// Byte ranges of the query text in the term's text, `[start, end)`, in order. Empty for
    /// a hash query's hash lookup.
    pub ranges: Vec<(u32, u32)>,
}

/// The term of a [`Match`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum MatchedTerm {
    /// A hash term. `name` is the sheet entry as the table writes it, and `None` for a hash
    /// no sheet names.
    Hash {
        /// The hash, widened to 64 bits.
        hash: u64,
        /// The name the term's table gives the hash.
        name: Option<String>,
    },
    /// A string term, lowercased.
    String(String),
}

impl MatchedTerm {
    /// The text the match's ranges index: the name, the string, or nothing.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            MatchedTerm::Hash { name, .. } => name.as_deref(),
            MatchedTerm::String(text) => Some(text),
        }
    }
}

/// The declarations a query reaches, with the terms each one matched through.
///
/// A matched term is held once, as a reference into its sheet. Its name and ranges are read
/// when a declaration's matches are read. The per-declaration lists behind
/// [`iter`](Self::iter) and [`get`](Self::get) are built on the first call to either.
#[derive(Clone)]
pub struct Matches<'a> {
    search: &'a GameSearch,
    /// The finder of the query text, for ranges. `None` for an empty text.
    finder: Option<memmem::Finder<'static>>,
    /// Every matched term once, in `TermKind::ALL` order, then sheet order.
    terms: Vec<TermRef<'a>>,
    /// The kinds each declaration matched through, one bit per kind, by declaration id.
    kinds: Vec<u8>,
    /// One per kind, in `TermKind::ALL` order.
    counts: [KindCount; 8],
    /// Declarations with at least one match.
    len: usize,
    superseded: bool,
    lists: OnceLock<Lists>,
}

/// Where a matched term comes from.
#[derive(Debug, Clone, Copy)]
enum Source {
    /// An entry of a name sheet whose text holds the query.
    Sheet { table: NameTable, entry: u32 },
    /// A hash query's lookup.
    Lookup { table: NameTable, hash: u64 },
    /// An entry of the strings sheet whose text holds the query.
    String { entry: u32 },
}

/// One matched term and the declarations holding it.
#[derive(Debug, Clone, Copy)]
struct TermRef<'a> {
    kind: TermKind,
    source: Source,
    ids: &'a [DeclarationId],
}

/// The matched terms of each declaration: `terms[offsets[id]..offsets[id + 1]]`, ascending.
#[derive(Debug, Clone)]
struct Lists {
    offsets: Vec<u32>,
    terms: Vec<u32>,
}

impl<'a> Matches<'a> {
    fn empty(search: &'a GameSearch, finder: Option<memmem::Finder<'static>>) -> Self {
        Self {
            search,
            finder,
            terms: Vec::new(),
            kinds: Vec::new(),
            counts: TermKind::ALL.map(|kind| KindCount { kind, count: 0 }),
            len: 0,
            superseded: false,
            lists: OnceLock::new(),
        }
    }

    /// Declarations matching through each kind, in [`TermKind::ALL`] order.
    #[must_use]
    pub fn counts(&self) -> &[KindCount] {
        &self.counts
    }

    /// The number of declarations with at least one match of `kind`.
    #[must_use]
    pub fn count(&self, kind: TermKind) -> u32 {
        self.counts[kind.ordinal()].count
    }

    /// The number of declarations matching through any kind.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no declaration matched.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether `called_off` stopped the query. The matches are a part of the answer.
    #[must_use]
    pub fn superseded(&self) -> bool {
        self.superseded
    }

    /// Every matched declaration with the kinds it matched through, by ascending id.
    ///
    /// Reads no term. A consumer ranking by its own measure narrows here first.
    pub fn declarations(&self) -> impl Iterator<Item = (DeclarationId, TermKinds)> + '_ {
        self.kinds
            .iter()
            .enumerate()
            .filter(|&(_, &kinds)| kinds != 0)
            .map(|(id, &kinds)| (DeclarationId::new(id as u32), TermKinds::from_bits(kinds)))
    }

    /// Every matched declaration with its matches, by ascending id.
    pub fn iter(&self) -> impl Iterator<Item = (DeclarationId, DeclarationMatches<'_>)> + '_ {
        self.declarations()
            .map(|(id, kinds)| (id, self.matches_of(id, kinds)))
    }

    /// The matches of declaration `id`, when it matched.
    #[must_use]
    pub fn get(&self, id: DeclarationId) -> Option<DeclarationMatches<'_>> {
        let kinds = *self.kinds.get(id.index())?;
        (kinds != 0).then(|| self.matches_of(id, TermKinds::from_bits(kinds)))
    }

    fn matches_of(&self, id: DeclarationId, kinds: TermKinds) -> DeclarationMatches<'_> {
        let lists = self.lists();
        let start = lists.offsets[id.index()] as usize;
        let end = lists.offsets[id.index() + 1] as usize;
        DeclarationMatches {
            matches: self,
            terms: &lists.terms[start..end],
            kinds,
        }
    }

    /// The per-declaration lists, built on first use by one counting pass and one filling pass
    /// over every posting.
    fn lists(&self) -> &Lists {
        self.lists.get_or_init(|| {
            let mut offsets = vec![0u32; self.kinds.len() + 1];
            for term in &self.terms {
                for id in term.ids {
                    offsets[id.index() + 1] += 1;
                }
            }
            for at in 1..offsets.len() {
                offsets[at] += offsets[at - 1];
            }
            let mut next = offsets.clone();
            let mut terms = vec![0u32; offsets.last().copied().unwrap_or(0) as usize];
            for (at, term) in self.terms.iter().enumerate() {
                for id in term.ids {
                    let slot = &mut next[id.index()];
                    terms[*slot as usize] = at as u32;
                    *slot += 1;
                }
            }
            Lists { offsets, terms }
        })
    }

    /// The term at `index` as a [`Match`], with its name and the ranges of the query text.
    fn to_match(&self, index: u32) -> Match {
        let term = self.terms[index as usize];
        let ranges = |sheet: &Sheet, entry: u32| {
            self.finder
                .as_ref()
                .map(|finder| sheet.ranges(entry, finder))
                .unwrap_or_default()
        };
        let (matched, ranges) = match term.source {
            Source::Sheet { table, entry } => {
                let sheet = self.search.scan_sheet(table);
                let matched = MatchedTerm::Hash {
                    hash: sheet.value(entry),
                    name: Some(sheet.entry(entry).into_owned()),
                };
                (matched, ranges(sheet, entry))
            }
            Source::Lookup { table, hash } => {
                let name = self.search.name(table, hash).map(Cow::into_owned);
                (MatchedTerm::Hash { hash, name }, Vec::new())
            }
            Source::String { entry } => {
                let strings = self.search.terms().strings();
                let matched = MatchedTerm::String(strings.entry_lower(entry).to_owned());
                (matched, ranges(strings, entry))
            }
        };
        Match {
            kind: term.kind,
            term: matched,
            ranges,
        }
    }

    /// Marks the kinds of every declaration in every term's posting, and counts.
    fn finish(mut self, declarations: usize, superseded: bool) -> Self {
        self.superseded = superseded;
        if self.terms.is_empty() {
            return self;
        }
        self.kinds = vec![0u8; declarations];
        for term in &self.terms {
            let bit = TermKinds::from(term.kind).bits();
            for id in term.ids {
                self.kinds[id.index()] |= bit;
            }
        }
        let mut counts = [0u32; 8];
        for &kinds in &self.kinds {
            if kinds == 0 {
                continue;
            }
            self.len += 1;
            for (bit, count) in counts.iter_mut().enumerate() {
                *count += u32::from(kinds >> bit & 1);
            }
        }
        for (slot, count) in self.counts.iter_mut().zip(counts) {
            slot.count = count;
        }
        self
    }
}

impl fmt::Debug for Matches<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Matches")
            .field("terms", &self.terms.len())
            .field("len", &self.len)
            .field("counts", &self.counts)
            .field("superseded", &self.superseded)
            .finish_non_exhaustive()
    }
}

/// The matches of one declaration, in [`TermKind::ALL`] order, then sheet order.
///
/// Each match is read from its sheet as the iterator reaches it.
#[derive(Debug, Clone)]
pub struct DeclarationMatches<'a> {
    matches: &'a Matches<'a>,
    terms: &'a [u32],
    kinds: TermKinds,
}

impl DeclarationMatches<'_> {
    /// The kinds the declaration matched through.
    #[must_use]
    pub fn kinds(&self) -> TermKinds {
        self.kinds
    }
}

impl Iterator for DeclarationMatches<'_> {
    type Item = Match;

    fn next(&mut self) -> Option<Self::Item> {
        let (&term, rest) = self.terms.split_first()?;
        self.terms = rest;
        Some(self.matches.to_match(term))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.terms.len(), Some(self.terms.len()))
    }
}

impl ExactSizeIterator for DeclarationMatches<'_> {}

/// One page of a query's matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchResult {
    /// [`Matches::counts`].
    pub counts: Vec<KindCount>,
    /// Matching declarations through the requested kinds, before paging.
    pub total: u32,
    /// The index of the first hit in the standing order.
    pub offset: u32,
    /// Whether hits exist past this page.
    pub truncated: bool,
    /// [`Matches::superseded`].
    pub superseded: bool,
    /// The page.
    pub hits: Vec<Hit>,
}

/// One declaration of a page, with its matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Hit {
    /// The declaration's id.
    pub id: DeclarationId,
    /// The declaration.
    pub declaration: Declaration,
    /// Every term the declaration matched through, in [`TermKind::ALL`] order, then sheet
    /// order. Kinds outside the query's kinds included.
    pub matches: Vec<Match>,
}

/// The hash a hash query names: 8 or 16 hexadecimal digits, with or without `0x`.
///
/// `text` is the trimmed, lowercased query text.
pub(super) fn parse_hash(text: &str) -> Option<u64> {
    let digits = text.strip_prefix("0x").unwrap_or(text);
    if !matches!(digits.len(), 8 | 16) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

/// Polls `called_off` once per sheet and once per [`POLL_INTERVAL`] postings.
struct Poll<'a> {
    called_off: Option<&'a (dyn Fn() -> bool + Sync)>,
    postings: usize,
    stopped: bool,
}

impl Poll<'_> {
    /// Polls `called_off`. Whether the query is called off.
    fn check(&mut self) -> bool {
        self.stopped = self.stopped || self.called_off.is_some_and(|called_off| called_off());
        self.stopped
    }

    /// Counts `postings` and polls when an interval has passed.
    fn after(&mut self, postings: usize) -> bool {
        let before = self.postings / POLL_INTERVAL;
        self.postings += postings;
        match self.postings / POLL_INTERVAL != before {
            true => self.check(),
            false => self.stopped,
        }
    }
}

impl GameSearch {
    /// The declarations `query` reaches.
    ///
    /// A hash query looks its hash up under every hash kind. Every query scans the sheet of
    /// every hash kind's table and the strings sheet, whatever `query.kinds` holds. Each
    /// matching entry with a non-empty posting is a match of every declaration in it.
    #[must_use]
    pub fn matches(&self, query: &Query<'_>) -> Matches<'_> {
        let text = query.text.trim().to_ascii_lowercase();
        let declarations = self.terms().objects().rows().len();
        let mut found = Matches::empty(self, finder(&text));
        if found.finder.is_none() {
            return found.finish(declarations, false);
        }
        let mut poll = Poll {
            called_off: query.called_off,
            postings: 0,
            stopped: false,
        };
        let hash = parse_hash(&text);
        let mut scans: [Option<Vec<u32>>; 5] = Default::default();

        'kinds: for kind in TermKind::ALL {
            let Some(table) = kind.table() else {
                if poll.check() {
                    break;
                }
                for entry in self.terms().strings().scan(&text) {
                    let ids = self.terms().string_postings(entry);
                    let source = Source::String { entry };
                    found.terms.push(TermRef { kind, source, ids });
                    if poll.after(ids.len()) {
                        break 'kinds;
                    }
                }
                continue;
            };
            if let Some(hash) = hash {
                let ids = self.terms().hash_postings(kind, hash);
                if !ids.is_empty() {
                    let source = Source::Lookup { table, hash };
                    found.terms.push(TermRef { kind, source, ids });
                    if poll.after(ids.len()) {
                        break;
                    }
                }
            }
            let scan = self.scan(table);
            let slot = table.kinds().iter().position(|&of| of == kind).unwrap_or(0);
            let scanned = match &mut scans[table.ordinal()] {
                Some(scanned) => scanned,
                pending => {
                    if poll.check() {
                        break;
                    }
                    pending.insert(self.scan_sheet(table).scan(&text))
                }
            };
            for &entry in scanned.iter() {
                let ids = scan.posting(self, entry, slot);
                if ids.is_empty() {
                    continue;
                }
                let source = Source::Sheet { table, entry };
                found.terms.push(TermRef { kind, source, ids });
                if poll.after(ids.len()) {
                    break 'kinds;
                }
            }
        }
        found.finish(declarations, poll.stopped)
    }

    /// One page of `matches` through `query.kinds`, in the standing order.
    ///
    /// The standing order is archive id, then the chunk's path from the
    /// [`NameTable::Chunks`] sheet under [`natural_cmp`], then declaration id. A chunk the
    /// sheet does not name sorts after every named chunk of its archive, by hash.
    #[must_use]
    pub fn page(&self, matches: &Matches<'_>, query: &Query<'_>) -> SearchResult {
        let rows = self.terms().objects().rows();
        let wanted = query.kinds.bits();
        let kinds = &matches.kinds;
        let kept = |id: DeclarationId| kinds.get(id.index()).is_some_and(|k| k & wanted != 0);
        let total = kinds.iter().filter(|&&k| k & wanted != 0).count() as u32;
        let offset = query.offset.min(total);
        let page = self.order().page(&kept, total, offset, query.limit);
        let terms = page_terms(matches, &page);
        let hits = page
            .iter()
            .zip(terms)
            .map(|(&id, terms)| Hit {
                id,
                declaration: rows[id.index()],
                matches: terms
                    .into_iter()
                    .map(|term| matches.to_match(term))
                    .collect(),
            })
            .collect();
        SearchResult {
            counts: matches.counts().to_vec(),
            total,
            offset: query.offset,
            truncated: total > offset.saturating_add(query.limit),
            superseded: matches.superseded(),
            hits,
        }
    }

    /// `page(&matches(query), query)`.
    #[must_use]
    pub fn search(&self, query: &Query<'_>) -> SearchResult {
        self.page(&self.matches(query), query)
    }
}

/// Every declaration in the standing order: archive id, then the chunk's path from the
/// `Chunks` sheet under [`natural_cmp`], unnamed chunks after named ones by hash, then id.
#[derive(Debug)]
pub(super) struct Standing {
    /// Every declaration, in the standing order.
    order: Vec<DeclarationId>,
    /// The position of each declaration in `order`, by id.
    position: Vec<u32>,
}

impl Standing {
    pub(super) fn cut(search: &GameSearch) -> Self {
        let rows = search.terms().objects().rows();
        let mut chunks: Vec<WadHash> = rows.iter().map(|row| row.chunk).collect();
        chunks.sort_unstable();
        chunks.dedup();
        let sheet = search.names().sheet(NameTable::Chunks);
        let paths: Vec<Option<&str>> = chunks
            .iter()
            .map(|chunk| Some(sheet.entry_lower(search.lookup(NameTable::Chunks, chunk.0)?)))
            .collect();
        let mut by_path: Vec<usize> = (0..chunks.len()).collect();
        by_path.sort_by(|&a, &b| match (paths[a], paths[b]) {
            (Some(x), Some(y)) => natural_cmp(x, y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => chunks[a].cmp(&chunks[b]),
        });
        let mut rank = vec![0u32; chunks.len()];
        for (at, &index) in by_path.iter().enumerate() {
            rank[index] = at as u32;
        }
        let mut keyed: Vec<(crate::ArchiveId, u32, DeclarationId)> = rows
            .iter()
            .enumerate()
            .map(|(id, row)| {
                let at = chunks.binary_search(&row.chunk).unwrap_or(0);
                (row.archive, rank[at], DeclarationId::new(id as u32))
            })
            .collect();
        keyed.sort_unstable();
        let order: Vec<DeclarationId> = keyed.into_iter().map(|(_, _, id)| id).collect();
        let mut position = vec![0u32; order.len()];
        for (at, id) in order.iter().enumerate() {
            position[id.index()] = at as u32;
        }
        Self { order, position }
    }

    /// The `limit` declarations from `offset` of the `total` that `kept` holds, in the
    /// standing order.
    ///
    /// A result of less than a sixteenth of the declarations sorts its ids by position. A
    /// larger one walks the order and stops at the end of the page.
    fn page(
        &self,
        kept: &impl Fn(DeclarationId) -> bool,
        total: u32,
        offset: u32,
        limit: u32,
    ) -> Vec<DeclarationId> {
        if total == 0 || offset >= total {
            return Vec::new();
        }
        match (total as usize) * 16 < self.order.len() {
            true => self.page_by_position(kept, offset, limit),
            false => self.page_by_walk(kept, offset, limit),
        }
    }

    fn page_by_position(
        &self,
        kept: &impl Fn(DeclarationId) -> bool,
        offset: u32,
        limit: u32,
    ) -> Vec<DeclarationId> {
        let mut ids: Vec<DeclarationId> = (0..self.position.len() as u32)
            .map(DeclarationId::new)
            .filter(|&id| kept(id))
            .collect();
        ids.sort_unstable_by_key(|id| self.position[id.index()]);
        ids.into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect()
    }

    fn page_by_walk(
        &self,
        kept: &impl Fn(DeclarationId) -> bool,
        offset: u32,
        limit: u32,
    ) -> Vec<DeclarationId> {
        self.order
            .iter()
            .copied()
            .filter(|&id| kept(id))
            .skip(offset as usize)
            .take(limit as usize)
            .collect()
    }
}

/// The matched terms of each declaration of `page`, in page order.
///
/// With the per-declaration lists built, a lookup each. Otherwise one pass over every posting
/// against a slot per declaration.
fn page_terms(matches: &Matches<'_>, page: &[DeclarationId]) -> Vec<Vec<u32>> {
    if let Some(lists) = matches.lists.get() {
        return page
            .iter()
            .map(|id| {
                let start = lists.offsets[id.index()] as usize;
                let end = lists.offsets[id.index() + 1] as usize;
                lists.terms[start..end].to_vec()
            })
            .collect();
    }
    let mut out = vec![Vec::new(); page.len()];
    if page.is_empty() {
        return out;
    }
    let mut slots = vec![u32::MAX; matches.kinds.len()];
    for (slot, id) in page.iter().enumerate() {
        slots[id.index()] = slot as u32;
    }
    for (at, term) in matches.terms.iter().enumerate() {
        for id in term.ids {
            if let Some(terms) = out.get_mut(slots[id.index()] as usize) {
                terms.push(at as u32);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_page_paths_follow_the_standing_order() {
        let order: Vec<DeclarationId> = (0..1000u32)
            .map(|at| DeclarationId::new(at.wrapping_mul(7919) % 1000))
            .collect();
        let mut position = vec![0u32; order.len()];
        for (at, id) in order.iter().enumerate() {
            position[id.index()] = at as u32;
        }
        let standing = Standing { order, position };
        for modulus in [3u32, 50, 997] {
            let kept = |id: DeclarationId| u32::from(id) % modulus == 1;
            for (offset, limit) in [(0, 10), (5, 3), (0, 2000), (40, 1)] {
                assert_eq!(
                    standing.page_by_position(&kept, offset, limit),
                    standing.page_by_walk(&kept, offset, limit),
                    "{modulus} {offset} {limit}"
                );
            }
        }
    }

    #[test]
    fn a_hash_query_is_eight_or_sixteen_hex_digits_with_or_without_the_prefix() {
        assert_eq!(parse_hash("0xdeadbeef"), Some(0xdead_beef));
        assert_eq!(parse_hash("deadbeef"), Some(0xdead_beef));
        assert_eq!(parse_hash("0123456789abcdef"), Some(0x0123_4567_89ab_cdef));
        assert_eq!(parse_hash("0xdead"), None);
        assert_eq!(parse_hash("0xzzzzzzzz"), None);
        assert_eq!(parse_hash("+eadbeef"), None);
        assert_eq!(parse_hash("aatrox"), None);
    }
}
