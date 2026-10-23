# Game search

## <a id="s1"></a>1. Summary

The `search` feature of `ltk_game_index` answers a substring query over every bin object of an
installation: the objects that mention the query as their own path, their class, a class
embedded in them, a field name, a string value, a hash value, an object link or a chunk link.
The same feature lists the installation's objects as one tree of paths. Both read a **term
index**, every declaration with every term it holds, and a set of **name sheets** cut from a
consumer's hash tables. The term index stores hashes only. A query resolves to hashes through
the name sheets and to declarations through the term index
([ADR-0038](../adr/0038-search-in-the-index-crate.md)). A hash table update recuts the name
sheets and leaves the term index in place.

```text
query "aatrox/skins/skin0"   -> entries sheet: Characters/Aatrox/Skins/Skin0  (0x1a2b3c4d)
                             -> declarations whose Object or Link term is 0x1a2b3c4d
query "assets/characters/aatrox/skins/base/aatrox.skn"
                             -> strings sheet: term #88213
                             -> declarations holding that string value
query "4f2e7d11"             -> every hash kind at 0x4f2e7d11, and the text scan as well
```

The feature answers in two layers. The lower layer, `matches`, is the set of declarations a
query reaches with the terms each one matched through. The upper layer, `search`, orders that
set and pages it. A consumer with its own ranking reads the lower layer. `ritobin-lsp` and
LTK Manager consume the feature. Each keeps its own hash tables, its own lifecycle around a
build, its own ranking and grammar, its own chunk rendering, and its own wire types
([section 10](#s10)).

## <a id="s2"></a>2. Vocabulary

The vocabulary of `game-index.md` [section 2](game-index.md#s2) applies. In addition:

- **Term:** One thing a declaration mentions. A **hash term** is a `(TermKind, u64)` pair. A
  **string term** is an ASCII-lowercased string value.
- **Term kind:** What a hash term is. `Object` is the declaration's own path hash. `Class` is
  the class the declaration is declared as. `Embedded` is the class of a struct or embedded
  value inside the declaration. `Field` is a property name. `Hash` is a `hash` value. `Link`
  is an object link value. `Wad` is a chunk link value. A string term has the kind `String`.
  Type `TermKind`.
- **Declaration id:** The ordinal of a declaration in the object index's storage order. Type
  `DeclarationId`.
- **Posting:** The ascending declaration ids holding one term.
- **Term index:** An object index with hash postings, string postings and the strings sheet.
  Type `TermIndex`.
- **Sheet:** Entries in one newline-separated text buffer, ASCII-lowercased for scanning and
  carrying a **case mask** that restores each entry as written, with one `u64` value per
  entry. Type `Sheet`.
- **Strings sheet:** The sheet of every string term. Its values are string term ids.
- **Name table:** One of the five hash tables search reads: `Entries`, `Types`, `Fields`,
  `Hashes`, `Chunks`. Type `NameTable`. A consumer supplies the tables through `NameSource`.
- **Name sheet:** The sheet cut from one name table. Its values are hashes.
- **Name sheets:** One name sheet per table, each with the version it was cut under. Type
  `NameSheets`.
- **Query:** A text, a set of kinds, a page and a cancellation. Type `Query`.
- **Hash query:** A query text of 8 or 16 hexadecimal digits, with or without a `0x` prefix.
- **Matches:** The declarations a query reaches, each with every term it matched through, and
  the count per kind. Type `Matches`.
- **Hit:** One declaration of a page, with its matches. Type `Hit`.
- **Object tree:** The named objects of the installation as paths split on `/`. Type
  `GameSearch` lists one directory of it. A **node** is one object in the tree. Type
  `ObjectNode`.
- **Unnamed directory:** The root pseudo-directory holding every object the `Entries` table
  does not name. Type `Dir::Unnamed`.

The feature does not say **search index** for any one structure. The term index and the name
sheets have different lifetimes. It does not say **file** for a declaration's path, and it does
not say **row** for a hit.

## <a id="s3"></a>3. Evidence

One installation, measured 2026-10-23 ([appendix A](#appendix-a)): 394 archives, 921,530
chunks, 50,550 bin chunks, 466,863 objects, 2.4 GB of decoded bin. Rendered as ritobin text
with no names resolved it is 8.3 GB and 186 million lines. A term index over the same
installation holds 447,791 declarations, 1.46 million distinct hash terms and 1.57 million
distinct string terms, in 200 MB of postings and a 100 MB strings sheet.

The `Entries` table holds 444,169 names, `Hashes` 418,634, `Fields` 10,198, `Types` 3,871. The
`Chunks` table holds 2.3 million paths against 0.9 million chunks in the installation.

A declarations-only object build over a comparable installation runs in 445 ms on eight
threads with a warm file cache; naming its 350,008 objects takes 399 ms (LTK Manager,
2026-09-05). A full decode of every bin chunk, which the term walk needs, is tens of seconds
on sixteen threads. A substring scan over a 100 MB sheet answers in tens of milliseconds.

## <a id="s4"></a>4. Feature and dependencies

```toml
[features]
search = ["objects", "dep:memchr"]
```

The `search` feature implies `objects`. It adds `memchr`. The `rayon` feature applies to the
term build as it applies to the object build. Every item of this specification is behind
`search`.

```
ltk_game_index [search]
|-- TermKind, TermKinds      the kinds of a term; a set of kinds
|-- DeclarationId            ordinal of a declaration in the object index
|-- TermIndex, TermStats     object index plus postings plus strings sheet; cache
|-- Sheet, Occurrence        lowercased entries with a case mask, substring scan
|-- NameTable, NameSource    the five tables; the consumer's supply of them
|-- NameSheets               one sheet per table; cache
|-- GameSearch               term index plus name sheets; matches, search, listing
|-- Query, Matches, DeclarationMatches, SearchResult, Hit, Match, MatchedTerm, KindCount
|-- Dir, ListOptions, Listing, TreeEntry, ObjectNode
|-- natural_cmp, path_cmp    digit runs by value; slash below every byte
|-- errors                   ObjectBuildError, CacheError
```

## <a id="s5"></a>5. Terms

```rust
/// What a hash term is, and the one kind a string term has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum TermKind {
    Object,
    Class,
    Embedded,
    Field,
    Hash,
    Link,
    Wad,
    String,
}

impl TermKind {
    /// Every kind, in declaration order.
    pub const ALL: [TermKind; 8];
    /// Every kind but `String`.
    pub const HASH_KINDS: [TermKind; 7];
    /// The name table a hash kind resolves through. `None` for `String`.
    pub const fn table(self) -> Option<NameTable>;
    /// The lowercase name: `object`, `class`, `embedded`, ...
    pub const fn name(self) -> &'static str;
}

/// A set of term kinds. `Debug` lists the kinds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TermKinds(u8);

impl TermKinds {
    pub const ALL: TermKinds;
    pub const NONE: TermKinds;
    pub fn of(kinds: impl IntoIterator<Item = TermKind>) -> Self;
    pub fn contains(self, kind: TermKind) -> bool;
    pub fn with(self, kind: TermKind) -> Self;
    pub fn without(self, kind: TermKind) -> Self;
    pub fn iter(self) -> impl Iterator<Item = TermKind>;
    pub fn is_empty(self) -> bool;
}
impl FromIterator<TermKind> for TermKinds {}
impl From<TermKind> for TermKinds {}
impl fmt::Display for TermKind { /* the lowercase name */ }
```

`Object` and `Link` resolve through `Entries`. `Class` and `Embedded` resolve through `Types`.
`Field` resolves through `Fields`. `Hash` resolves through `Hashes`. `Wad` resolves through
`Chunks`.

The terms of one object:

- Its path hash is an `Object` term. Its class hash is a `Class` term.
- Every struct or embedded value with a non-zero class hash contributes an `Embedded` term.
- Every property name is a `Field` term, at any depth.
- Every `hash` value is a `Hash` term, every object link a `Link` term, every chunk link a
  `Wad` term, every string a `String` term.
- Containers, unordered containers, optionals and maps are descended. A map's keys and values
  both count.
- Numbers, booleans, bit-booleans, vectors, matrices and colours contribute nothing.
- A `None`-typed value contributes nothing.

A string term is the value ASCII-lowercased. An empty string and a string holding a newline are
not terms. A declaration holds a term once however many times the term appears in it.

A reference query of LTK Manager's References document maps onto the kinds: every object
declared as a class is the `Class` posting of the class hash; every object holding a struct or
embedded value of a class is the `Embedded` posting; every object linking to an object is the
union of the `Link` and `Hash` postings of the object hash; every object naming a chunk is the
union of the `Wad` posting of the chunk hash, the `Hash` posting of the path's bin hash, and
the `String` posting of the path.

## <a id="s6"></a>6. Term index

### <a id="s6.1"></a>6.1 Rows

A declaration id, `DeclarationId`, is an index into the object index's storage order
(`game-index.md` [section 9.1](game-index.md#s9.1)). Ids are dense from zero. An id is stable for one term
index and its cache, and is not stable across builds. A term build and an object build over
the same chunk index with the same resolver produce the same storage order; an id of one
names the same declaration in the other.

Hash postings are one ascending array of `u64` hashes, sorted by kind and then hash, with one
start per kind into it and an offset per hash into one array of declaration ids; one more
offset than hashes holds the end. Lookup is a binary search over the run of one kind. String
postings are one offset per string term id into one array of declaration ids, with one more
offset than terms. Each posting is ascending and holds a declaration once. Every string term
has a non-empty posting.

The strings sheet holds every distinct string term once, in first-seen order over the
declarations in storage order. The value of an entry is its own index. A string term id is that
index. The build interns each job's strings in one buffer keyed by the XXH3 hash of each
string, and the merge interns the jobs' strings into the buffer that becomes the strings sheet.
Two strings sharing a hash are told apart by their text.

### <a id="s6.2"></a>6.2 Surface

```rust
/// Every declaration of an installation with the terms each one holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermIndex { /* objects, hash postings, string postings, strings sheet, stats */ }

/// Counts of one term build, beyond the object build's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TermStats {
    /// Distinct `(kind, hash)` keys.
    pub hash_terms: u32,
    /// Distinct string terms.
    pub string_terms: u32,
    /// Bytes of the strings sheet.
    pub string_bytes: u64,
    /// Bin chunks whose object tables read and whose values did not decode.
    pub failed_chunks: u32,
}

impl TermIndex {
    pub fn build(game: &GameIndex) -> Result<Self, ObjectBuildError>;
    pub fn build_with(game: &GameIndex, options: &BuildOptions<'_>) -> Result<Self, ObjectBuildError>;

    pub fn load_for(cache_path: &Utf8Path, game: &GameIndex) -> Result<Self, CacheError>;
    pub fn save(&self, cache_path: &Utf8Path) -> Result<(), CacheError>;
    pub fn load_or_build_with(
        game: &GameIndex,
        cache_path: &Utf8Path,
        options: &BuildOptions<'_>,
    ) -> Result<Self, ObjectBuildError>;

    /// The object index the build produced.
    pub fn objects(&self) -> &ObjectIndex;
    /// The declarations holding hash term `(kind, hash)`, ascending. Empty for `String`.
    pub fn hash_postings(&self, kind: TermKind, hash: u64) -> &[DeclarationId];
    /// The union of the postings of `hash` under every kind in `kinds`, ascending.
    pub fn postings_in(&self, kinds: TermKinds, hash: u64) -> Vec<DeclarationId>;
    /// The declarations holding string term `term`, ascending.
    pub fn string_postings(&self, term: u32) -> &[DeclarationId];
    /// Every string term, lowercased.
    pub fn strings(&self) -> &Sheet;

    pub fn stats(&self) -> &TermStats;
    pub fn fingerprint(&self) -> Fingerprint;
}

/// The cache format version of the term index.
pub const TERM_CACHE_FORMAT_VERSION: u32;
```

`objects()` is a complete object index: `rows`, `declarations`, `chunk_declarations`, `stats`,
`skipped` and `fingerprint` answer as they do for an object index built on its own. A consumer
with a term index has no need of a second object build. A consumer with an object build of
its own keeps it; the ids agree ([section 6.1](#s6.1)).

`hash_postings(Class, class).len()` is the number of declarations of a class.

### <a id="s6.3"></a>6.3 Building

The term build is the object build (`game-index.md` [section 9.3](game-index.md#s9.3)) with
a different chunk read. The partition into one job per archive, the read-sniff-skip policy over
a resolver, sniffing, `workers`, `called_off`, `on_archive` and skipped archives are the object
build's. A `PROP` is walked object by object over its streamed bytes, in object table order:
the object cursor of `ltk_meta::stream::BinStream` and `ltk_meta::walk` over each buffered
object, with no object materialized. A `PTCH` is read through
`ltk_meta::BinOverride::from_reader` and its objects walked over the owned tree. One visitor
serves both. It takes `Object` and `Class` at the object, `Embedded` at every nested node and
`Field` at every property; it opens a container, optional or map only when its item or key
kind is a string, hash, chunk link or object link, and decodes no other leaf. Every object is a
declaration in the order the object build reads it, and contributes its terms
([section 5](#s5)).

A chunk whose object table reads and whose walk fails on a header or a term leaf holds the
declarations the object build reads from it and contributes no term. It counts in `TermStats::failed_chunks`.
A chunk that does not read as far as its object table counts in `ObjectStats::skipped_chunks` and
produces no declaration, as in the object build. The storage order of a term build is the
storage order of an object build over the same chunk index with the same resolver.

Per-archive results merge in archive id order. Declaration ids are assigned in that order.
String terms are interned into the strings sheet in that order.

### <a id="s6.4"></a>6.4 Cache

The term cache is a MessagePack document in the shape of `game-index.md`
[section 7](game-index.md#s7): `TERM_CACHE_FORMAT_VERSION`, the fingerprint, the body. The body
holds the object index body, both postings and the strings sheet. Every array of ids, keys,
offsets and values, and every sheet buffer, serializes as one MessagePack byte string of
little-endian elements. A load of such a field is one copy. `load_for` reports
`CacheError::Stale` when the cached fingerprint differs from `game.fingerprint()`,
`CacheError::Version` on any other format version, and `CacheError::Inconsistent` when the
postings disagree with the declarations or the strings sheet. `load_or_build_with` follows the chunk
index's load-or-build rule.

## <a id="s7"></a>7. Names

### <a id="s7.1"></a>7.1 Sheet

```rust
/// Entries in one text buffer, lowercased for scanning, each with a `u64` value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sheet { /* text, cases, starts, values */ }

/// One occurrence of a needle in a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Occurrence {
    pub entry: u32,
    /// Byte offsets into the entry, `[start, end)`.
    pub start: u32,
    pub end: u32,
}

impl Sheet {
    /// A repeated entry, compared lowercased, keeps its first value. An empty entry and an
    /// entry holding a newline are dropped.
    pub fn build<S: AsRef<str>>(entries: impl IntoIterator<Item = (S, u64)>) -> Self;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    /// The entry at `index` as it was given, without its newline.
    pub fn entry(&self, index: u32) -> Cow<'_, str>;
    /// The entry at `index` lowercased.
    pub fn entry_lower(&self, index: u32) -> &str;
    pub fn value(&self, index: u32) -> u64;
    /// Every value, in entry order.
    pub fn values(&self) -> &[u64];
    /// The lowercased buffer: every entry followed by `\n`.
    pub fn text(&self) -> &str;
    /// The entry holding byte `offset` of `text()`.
    pub fn entry_at(&self, offset: usize) -> u32;
    /// The byte range of entry `index` in `text()`, without its newline.
    pub fn span(&self, index: u32) -> Range<usize>;
    /// Every entry whose text contains `needle`, ascending, each once. `needle` is lowercased
    /// first. An empty needle matches nothing.
    pub fn scan(&self, needle: &str) -> Vec<u32>;
    /// Every occurrence of `needle`, in buffer order. Occurrences in one entry do not overlap.
    pub fn occurrences(&self, needle: &str) -> impl Iterator<Item = Occurrence> + '_;
}
```

The text buffer holds every entry ASCII-lowercased and newline-terminated. The case mask is
one bit per byte of the buffer, set where the byte was an ASCII uppercase letter. `entry`
restores those bytes to uppercase and borrows the buffer when no bit is set in its span. A
non-ASCII byte is stored as given and matched as given. An ASCII case change keeps a byte's
length; an offset into the lowercased entry is an offset into the entry as written.

A sheet loads only when its starts, values, case mask and text agree: every entry ends in a
newline and the next entry starts after it. A scan is a `memchr::memmem` search over the buffer
with one find per entry holding the needle: a find maps to its entry by a forward search over
entry starts, and the search resumes at the next entry. With the `rayon` feature a sheet of more
than 1 MiB scans in segments of whole entries in parallel. A needle holding a newline matches
nothing. A consumer with a
pattern the scan cannot express, a regular expression, runs it over `text()` and maps each
match through `entry_at`.

### <a id="s7.2"></a>7.2 Name source

```rust
/// The hash tables search reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum NameTable {
    /// Object paths, FNV-1a 32.
    Entries,
    /// Class names, FNV-1a 32.
    Types,
    /// Property names, FNV-1a 32.
    Fields,
    /// `hash` values, FNV-1a 32.
    Hashes,
    /// Chunk paths, XXH64.
    Chunks,
}

impl NameTable {
    pub const ALL: [NameTable; 5];
    /// The width of the table's hashes in bits: 32, or 64 for `Chunks`.
    pub const fn bits(self) -> u8;
    /// The lowercase name: `entries`, `types`, `fields`, `hashes`, `chunks`.
    pub const fn name(self) -> &'static str;
}
impl fmt::Display for NameTable { /* the lowercase name */ }

/// A consumer's hash tables.
pub trait NameSource {
    /// Identifies the contents of `table`. `None` cuts the sheet on every build and caches
    /// nothing for it.
    ///
    /// Two sources with equal versions for a table name every hash the same. A checksum of
    /// the table file is a version.
    fn version(&self, table: NameTable) -> Option<String>;
    /// Visits every `(hash, name)` of `table`, in any order. A table the source does not
    /// have visits nothing.
    fn for_each_name(&self, table: NameTable, visit: &mut dyn FnMut(u64, &str));
}
```

`version` keys the cache of a name sheet. `for_each_name` cuts a sheet. The sheets hold every
name as the table writes it; nothing is looked up at query time. `ritobin-lsp` implements the
trait over its Mimir hash store with the manifest checksum as the version. LTK Manager
implements it over its layered hash database. `ltk_hashtable::HashtableSet` carries no checksum
and implements the trait with a caller-supplied version, or `None`.

A `NameSource` is not a `ResolveWadPath`. The term build takes a resolver for its policy and a
source for its sheets; a consumer passes the same tables to both through two implementations.

### <a id="s7.3"></a>7.3 Name sheets

```rust
/// One sheet per name table, each with the version it was cut under.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameSheets { /* per table: version, sheet */ }

/// The cache format version of the name sheets.
pub const NAME_CACHE_FORMAT_VERSION: u32;

impl NameSheets {
    /// Cuts every table's sheet from `source`.
    pub fn build(source: &dyn NameSource, game: &GameIndex) -> Self;
    /// The cached sheets, with every table whose cached version differs from the source's,
    /// or whose version is `None`, recut.
    pub fn load_or_build(cache_path: &Utf8Path, source: &dyn NameSource, game: &GameIndex) -> Self;
    pub fn load(cache_path: &Utf8Path) -> Result<Self, CacheError>;
    pub fn save(&self, cache_path: &Utf8Path) -> Result<(), CacheError>;

    /// The sheet of `table`. Empty for a table the source does not have.
    pub fn sheet(&self, table: NameTable) -> &Sheet;
    pub fn version(&self, table: NameTable) -> Option<&str>;
    /// Whether every sheet is empty.
    pub fn is_empty(&self) -> bool;
}
```

The `Chunks` sheet keeps only the hashes `game.contains`. Its stored version is the source's
version joined to the game fingerprint. Every other sheet keeps every entry of its table. A
sheet's value is the hash.

The name cache is a MessagePack document of `NAME_CACHE_FORMAT_VERSION`, a zero fingerprint, and
the body, encoded as the term cache is ([section 6.4](#s6.4)). `load_or_build` loads, treats
any error as an empty set with a debug log for a missing file and a warn otherwise, recuts
every table whose cached version differs from `source.version(table)` or whose version is
`None`, saves best-effort with a warn on failure when a recut table has a version or had one,
and returns the set. A table whose version is `None` is saved as empty, and its recut alone
writes nothing. A name sheet is never stale by fingerprint; the `Chunks` version
carries the fingerprint.

## <a id="s8"></a>8. Search

### <a id="s8.1"></a>8.1 Surface

```rust
/// A term index and the name sheets that resolve its hashes.
#[derive(Debug)]
pub struct GameSearch { /* terms, names, lookups, scan tables, standing order, sorted paths */ }

impl GameSearch {
    pub fn new(terms: TermIndex, names: NameSheets) -> Self;
    /// Cuts every structure a query, a page or a listing reads on first use.
    pub fn prepare(&self);
    pub fn terms(&self) -> &TermIndex;
    pub fn names(&self) -> &NameSheets;
    /// Replaces the name sheets. The term index and its ids are unchanged.
    pub fn set_names(&mut self, names: NameSheets);
    pub fn into_parts(self) -> (TermIndex, NameSheets);
    /// The name `table` gives `hash`, as the table writes it.
    pub fn name(&self, table: NameTable, hash: u64) -> Option<Cow<'_, str>>;

    /// The declarations `query` reaches ([section 8.2](#s8.2)).
    pub fn matches(&self, query: &Query<'_>) -> Matches<'_>;
    /// One page of `matches` through `query.kinds`, in the standing order
    /// ([section 8.3](#s8.3)).
    pub fn page(&self, matches: &Matches<'_>, query: &Query<'_>) -> SearchResult;
    /// `page(&matches(query), query)`.
    pub fn search(&self, query: &Query<'_>) -> SearchResult;
}

/// Built as `Query::new(text)` with fields assigned.
#[derive(Clone)]
#[non_exhaustive]
pub struct Query<'a> {
    pub text: &'a str,
    /// The kinds a hit may match through. `TermKinds::ALL` by default.
    pub kinds: TermKinds,
    /// The index of the first hit of the page. Zero by default.
    pub offset: u32,
    /// Hits per page. `DEFAULT_LIMIT` by default.
    pub limit: u32,
    /// Polled between sheets and every few thousand postings. `true` stops the query.
    pub called_off: Option<&'a (dyn Fn() -> bool + Sync)>,
}

impl<'a> Query<'a> {
    pub fn new(text: &'a str) -> Self;
}
impl fmt::Debug for Query<'_> { /* text, kinds, offset, limit, whether called_off is set */ }

/// The default `Query::limit`.
pub const DEFAULT_LIMIT: u32 = 500;

/// The declarations a query reaches, with the terms each one matched through.
#[derive(Clone)]
pub struct Matches<'a> { /* the search, term references, kinds per declaration, counts */ }

impl Matches<'_> {
    /// Declarations matching through each kind, in `TermKind::ALL` order.
    pub fn counts(&self) -> &[KindCount];
    /// Declarations with at least one match of `kind`.
    pub fn count(&self, kind: TermKind) -> u32;
    /// Declarations matching through any kind.
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    /// Whether `called_off` stopped the query. The matches are a part of the answer.
    pub fn superseded(&self) -> bool;
    /// Every matched declaration with the kinds it matched through, by ascending id. Reads no
    /// term.
    pub fn declarations(&self) -> impl Iterator<Item = (DeclarationId, TermKinds)> + '_;
    /// Every matched declaration with its matches, by ascending id.
    pub fn iter(&self) -> impl Iterator<Item = (DeclarationId, DeclarationMatches<'_>)>;
    pub fn get(&self, id: DeclarationId) -> Option<DeclarationMatches<'_>>;
}

impl fmt::Debug for Matches<'_> { /* term and declaration counts, counts, superseded */ }

/// The matches of one declaration, in `TermKind::ALL` order, then sheet order. Each match is
/// read from its sheet as the iterator reaches it.
#[derive(Debug, Clone)]
pub struct DeclarationMatches<'a> { /* borrowed from `Matches` */ }

impl Iterator for DeclarationMatches<'_> { type Item = Match; }
impl ExactSizeIterator for DeclarationMatches<'_> {}
impl DeclarationMatches<'_> {
    /// The kinds the declaration matched through.
    pub fn kinds(&self) -> TermKinds;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchResult {
    /// `Matches::counts`.
    pub counts: Vec<KindCount>,
    /// Matching declarations through the requested kinds, before paging.
    pub total: u32,
    pub offset: u32,
    /// Whether hits exist past this page.
    pub truncated: bool,
    /// `Matches::superseded`.
    pub superseded: bool,
    pub hits: Vec<Hit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct KindCount {
    pub kind: TermKind,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Hit {
    pub id: DeclarationId,
    pub declaration: Declaration,
    /// Every term the declaration matched through, in `TermKind::ALL` order, then sheet order.
    pub matches: Vec<Match>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Match {
    pub kind: TermKind,
    pub term: MatchedTerm,
    /// Byte ranges of the query text in the term's text, `[start, end)`, in order. Empty
    /// for a hash query's hash lookup.
    pub ranges: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum MatchedTerm {
    /// A hash term. `name` is the sheet entry as the table writes it, and `None` for a hash
    /// no sheet names.
    Hash { hash: u64, name: Option<String> },
    /// A string term, lowercased.
    String(String),
}

impl MatchedTerm {
    /// The text the ranges index: the name, the string, or `None` for an unnamed hash.
    pub fn text(&self) -> Option<&str>;
}

/// Orders two names with digit runs compared by value: `Skin2` before `Skin10`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering;
/// Orders two paths by bytes with `/` below every other byte.
pub fn path_cmp(a: &str, b: &str) -> Ordering;
```

A hit carries hashes. The consumer names the object, class and chunk of a hit through `name` or
its own tables, and the archive through `GameIndex::archive`. `name` reads a per-table lookup
from hash to sheet entry, cut from the sheet on first use and kept until `set_names`.

A query scans one **scan table** per name table, cut on first use and kept until `set_names`:
a sheet and, per entry, the position of the entry's hash among the hash postings under each of
the table's kinds. A matching entry reaches its postings without a search. An entry whose hash
no declaration holds under any of the table's kinds never matches; when such entries are more
than a quarter of a sheet's bytes, the scan table holds a copy of the sheet without them, and
otherwise it scans the table's own sheet. On the measured installation the `Types`, `Fields`
and `Chunks` scan tables hold copies, the `Chunks` copy 150,460 of 823,233 entries, and
`Entries` and `Hashes` scan their own sheets. `prepare` cuts
the lookups, the scan tables, the standing order and the sorted path list, in parallel with the
`rayon` feature. A consumer that builds its search in the background calls it there.

`Matches` borrows its `GameSearch`. It holds each matched term once, as its kind, its sheet
entry or looked-up hash, and its posting, and holds one byte of kinds per declaration. A term's
name and ranges are read from its sheet when a `Match` is made of it: by `page` for the hits
of the page, and by `DeclarationMatches` as it iterates. `iter` and `get` read a list of terms
per declaration, built on the first call to either by one counting pass and one filling pass
over the postings. `page` reads the terms of its hits by one pass over the postings unless the
lists are built. The cost of `matches` is a scan of each sheet, one find per matching entry,
and one pass over the postings of the matched terms.

### <a id="s8.2"></a>8.2 Matches

1. The text is trimmed and ASCII-lowercased. An empty text is an empty `Matches` with every
   count zero.
2. A hash query looks the hash up in `hash_postings` for every kind of `HASH_KINDS`. A match
   of the lookup carries the hash, the sheet's name for it or `None`, and no ranges.
3. Every text, a hash query included, scans the sheet of every kind's table and the strings
   sheet. Every kind is scanned, whatever `query.kinds` holds. Each table's sheet is scanned
   once for the kinds that share it. Each sheet entry whose postings are non-empty is a match
   for every declaration in the postings, with the entry's name and the non-overlapping
   occurrences of the text in it as ranges.
4. `called_off` is polled before each sheet and every 4,096 postings. A query that observes
   `true` stops, marks `superseded`, and answers with what it has.
5. `counts` holds, per kind, the number of declarations with at least one match of that kind.
   A declaration matching through two kinds counts in both. A declaration matching a hash
   query by lookup and by text once lists both matches.

A `Field` term posts to most of the corpus. The query `mname` matches 25,376 declarations through
`Fields` on the measured installation. `kinds`, `limit`, `counts` and `called_off` are how a
consumer keeps that useful.

### <a id="s8.3"></a>8.3 Page and order

`page` keeps the declarations with at least one match of a kind in `query.kinds`, orders them,
and holds `limit` of them from `offset` as hits. `total` is their number before paging. An
`offset` past `total` yields no hits and `truncated` false. A hit's `matches` holds every match
of the declaration, the kinds outside `query.kinds` included.

The standing order is archive id, then the chunk's path from the `Chunks` sheet under
`natural_cmp`, then declaration id. A chunk the sheet does not name sorts after every named
chunk of the archive, by hash. Declaration ids ascend within one chunk, which is the chunk's own
object order. The standing order of every declaration, and the position of each declaration
in it, are cut on first use and kept until `set_names`. A page of fewer than a sixteenth of the
declarations sorts its declarations by position. A larger page walks the order and stops at
the end of the page.

`natural_cmp` splits each name into digit runs and non-digit runs. A digit run compares by its
value, with equal values ordered by length. A non-digit run compares by ASCII-lowercased bytes.
A run of more than nineteen digits compares as `u64::MAX`.

A consumer with a ranking of its own reads `matches`, scores the declarations it keeps, and
builds its own page. `page` is one order among the possible ones.

## <a id="s9"></a>9. Object tree

```rust
/// What `list` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir<'a> {
    Root,
    /// A path, compared without regard to ASCII case and leading or trailing slashes. An
    /// empty path is the root.
    Path(&'a str),
    /// Every object the `Entries` table does not name.
    Unnamed,
}

/// Built as `ListOptions::default()` with fields assigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ListOptions {
    /// Entries a listing holds at most. `DEFAULT_LISTING_LIMIT` by default.
    pub limit: usize,
    /// Whether a run of directories each holding one directory and no object folds into
    /// one entry. `false` by default.
    pub fold_single: bool,
}

/// The `limit` of `ListOptions::default()`.
pub const DEFAULT_LISTING_LIMIT: usize = 2000;

/// One object of the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ObjectNode {
    /// The object's path as the table writes it, or its hash as `0x` and eight lowercase
    /// hex digits for an unnamed object.
    pub path: String,
    /// The last segment of `path`, or the hash text.
    pub name: String,
    pub object: BinHash,
    /// Every declaration of the object, in storage order.
    pub declarations: Vec<DeclarationId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum TreeEntry {
    /// A path with objects beneath it.
    Dir {
        /// What `Dir::Path` takes to list it.
        path: String,
        /// The last segment, or the folded segments joined by `/`.
        name: String,
        /// Named objects anywhere beneath.
        objects: u32,
        /// The object at this path itself, when there is one.
        own: Option<ObjectNode>,
    },
    Object(ObjectNode),
    /// The unnamed directory, which `Dir::Unnamed` lists. Root listings hold it.
    Unnamed {
        /// Objects the `Entries` table does not name.
        objects: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Listing {
    pub entries: Vec<TreeEntry>,
    /// Entries before the limit.
    pub total: u32,
    pub truncated: bool,
}

impl GameSearch {
    /// One directory of the object tree. `None` for a `Dir::Path` no object path runs
    /// through.
    pub fn list(&self, dir: Dir<'_>, options: &ListOptions) -> Option<Listing>;
    /// Every named object at or below `prefix`, as its lowercased path and hash, in
    /// `path_cmp` order of the lowercased path.
    pub fn under(&self, prefix: &str) -> impl Iterator<Item = (&str, BinHash)> + '_;
    /// The node at `path`, when an object has that path.
    pub fn node(&self, path: &str) -> Option<ObjectNode>;
}
```

The tree is every object of the term index named through the `Entries` sheet, split on `/`.
One object is one node; its declarations hang on the node. A listing holds directories first,
in `natural_cmp` order of their names, then objects in the same order. A directory's `objects`
counts every named object beneath it at any depth. A path that is itself an object and the
parent of others is one `Dir` entry with `own` set, and no `Object` entry. The root holds
`TreeEntry::Unnamed` last among directories when any object is unnamed. `Dir::Unnamed` lists
one `Object` per unnamed object hash, ascending.

With `fold_single`, a directory holding exactly one directory and no object folds into that
directory, repeatedly. The folded entry's `path` is the innermost path and its `name` is the
folded segments joined by `/`. A directory with an `own` object never folds.

`entries` holds at most `limit` entries. `total` counts every entry before the limit. A
listing sorts its entries by their lowercased names and builds the nodes of the entries within
the limit alone. A directory that is cut off is reached through search.

The sorted path list is every named object's lowercased path with its hash, ordered by
`path_cmp`. Under `path_cmp` a node sorts before everything beneath it, and everything beneath
it before any sibling; one directory's objects are one run. The list is cut from the `Entries`
sheet on the first `list`, `under` or `node` and kept for the life of the `GameSearch`.
`set_names` drops it. A sheet holding no NUL byte orders under `path_cmp` as its bytes order
with each `/` read as NUL; such a sheet sorts by plain byte comparison over a copy with `/`
replaced, in parallel with the `rayon` feature.

## <a id="s10"></a>10. Consumers

### <a id="s10.1"></a>10.1 `ritobin-lsp`

The server holds a `GameSearch` in a swappable slot. Its build is one background task: the
chunk index through `load_or_build`, the name sheets through `NameSheets::load_or_build` over
its Mimir store, the term index through `TermIndex::load_or_build_with` with the `Chunks` table
as resolver and a `called_off` bound to a build generation. A hash table update recuts the
sheets through `set_names`. Its `gameSearch` request is `search` with the query's kinds, page
and the request's cancellation; its `gameBrowse` request is `list` without folding; its
`workspace/symbol` request is `search` over `Object` and `Class`. The server maps `Hit` and
`TreeEntry` to its wire types with names from `names()`, marks the query in a term from
`ranges`, and renders a declaring chunk through `ltk_ritobin`.

### <a id="s10.2"></a>10.2 LTK Manager

The manager's object index is a declarations-only build of its own over its own chunk index.
The crate's `ObjectIndex` is the same design. The manager adopts `GameIndex` and `ObjectIndex`
as [ADR-0009](../adr/0009-game-index-crate.md) states, and warms a `TermIndex` behind its
Objects switch, cached in its state directory.

What stays the manager's, over the crate's layers:

- The palette's ranked search: band, score, bounded heap, `class:` grammar and completions.
  It reads `matches` over `Object`, narrows by `Declaration::class`, ranks with its own matcher
  over the `Entries` sheet's names, and marks runs from its own matcher or from `ranges`.
- The full-results find: a literal or regular-expression pattern over object paths. It runs
  its `regex` over `names().sheet(Entries).text()` and maps matches through `entry_at`.
- The References document: `Class`, `Embedded`, `Linked` and `File` queries resolve at the
  object level from postings ([section 5](#s5)); the walk that finds the property steps inside
  an object reads the declaring chunks of those postings alone.
- The object browser: `list` with `fold_single`, its `?` group for `TreeEntry::Unnamed`, and its
  own `ObjectNodeEntry`
  with declarations ordered as a link resolves them.
- Declarations of an object ordered for link resolution, this file first, then a dependency,
  then the rest: an order over `ObjectIndex::declarations(object)`.
- `character_spells`: `under("Characters/<name>/Spells")`.
- Lifecycle slots, build tickets, generation lines, IPC types and `specta` bindings.

### <a id="s10.3"></a>10.3 `ltk_overlay`

`ltk_overlay` uses the `objects` feature alone.

## <a id="s11"></a>11. Validation

- **Terms.** A `Bin` built with `ltk_meta` builders holding every leaf kind inside a struct, an
  embedded, a container, an unordered container, an optional and a map yields exactly the
  expected term set, each term once; the object's own class is `Class` and the nested classes
  are `Embedded`; a numeric property contributes only its `Field` term.
- **Sheet.** A sheet round-trips through its cache; `entry` restores mixed case and borrows a
  lowercase entry; `scan` returns every entry containing the needle and no other; a needle
  spanning an entry boundary matches nothing; `occurrences` reports every run with offsets
  into the entry; `entry_at` and `span` agree; a repeated entry keeps its first value.
- **Term index.** Over a temporary installation written with `ltk_wad`'s builder, as in the
  object index tests: a `PROP` and a `PTCH` fixture produce the expected postings per kind;
  `objects()` equals an `ObjectIndex` built on its own over the same installation, ids
  included; `postings_in` unions; the cache round-trips, rejects a foreign version and a stale
  fingerprint, and its id arrays are byte strings; `called_off` after the first archive returns
  `CalledOff`.
- **Name sheets.** A hand-written `NameSource`: a sheet is cut per table with a version; a
  changed version recuts that table alone; a `None` version recuts on every load and is saved
  empty; the `Chunks` sheet holds only held chunks and its version carries the fingerprint.
- **Matches and search.** Over the same installation and source: a hash query with and
  without `0x` hits by lookup in every hash kind and by text as well; a name query resolves
  through each sheet with ranges; a string query resolves through the strings sheet; `counts`
  covers every kind while `page` keeps the requested kinds; `limit` truncates and `total` is
  unaffected; an `offset` past the end is empty; hits are in archive, natural chunk path, id
  order; a `called_off` that answers `true` on the second poll marks `superseded` and keeps the
  first sheet's matches.
- **Object tree.** The root lists directories first in natural order with counts, a directory
  that is also an object carries `own`, the unnamed directory sits last among directories and
  lists by hash, `fold_single` folds a single-child run and stops at an object, a listing over
  the limit reports `total` and `truncated`, `under` yields a prefix's objects in `path_cmp`
  order, and `node` finds a path.
- **Order.** `root`, `Skin1`, `skin2`, `skin10`, `skin100` sort in that order under
  `natural_cmp`; `a`, `a/b`, `a-b` sort in that order under `path_cmp`.

Tests sit under `tests/search.rs` with the fixture helpers of `tests/common`, and inline as
`#[cfg(test)] mod tests` for `Sheet`, `natural_cmp` and `path_cmp`.

## <a id="s12"></a>12. Rules

| ID | Rule | Instead of | Why | Spec |
| --- | --- | --- | --- | --- |
| S1 | Search is a feature of `ltk_game_index` | A sibling crate or a copy per application | Two applications want one index | [ADR-0038](../adr/0038-search-in-the-index-crate.md) |
| S2 | The term index stores hashes; names resolve through sheets at query time | Resolved names in the index | A table update recuts 100 MB of sheets, not a 30 s walk | [section 1](#s1), [ADR-0038](../adr/0038-search-in-the-index-crate.md) |
| S3 | Substring over a sheet | A tokenized or fst index | The query shape is "find in files"; substring is exact by construction | [section 7.1](#s7.1), [ADR-0038](../adr/0038-search-in-the-index-crate.md) |
| S4 | Numbers, vectors and booleans are not terms | Every value a term | 186 million lines of numbers nobody searches by | [section 5](#s5) |
| S5 | A term index owns a complete object index with the same ids | A second object build | One walk serves both; a consumer's own object build agrees | [section 6.1](#s6.1), [section 6.2](#s6.2) |
| S6 | Postings key on the `(kind, hash)` pair | A hash folded into one `u64` | A 64-bit chunk hash keeps every bit | [section 6.1](#s6.1) |
| S7 | Names enter through `NameSource`, five tables by name, two methods | A hash table dependency; a per-hash lookup | Each consumer keeps its own tables; a sheet holds the name as written | [section 7.2](#s7.2) |
| S8 | A name sheet is keyed by the consumer's version string; `None` is never cached | The game fingerprint | A table changes without the installation changing; a source without a checksum is a source | [section 7.3](#s7.3) |
| S9 | The `Chunks` sheet keeps only held chunks | The whole table | The table is 2.3 million paths against 0.9 million chunks; a link to an absent chunk resolves nothing | [section 7.3](#s7.3) |
| S10 | Every kind is counted on every query | Counting the requested kinds | A consumer's kind filter is drawn from the counts | [section 8.2](#s8.2) |
| S11 | A page is `offset` and `limit`, default 500 | Unbounded results | A `Field` term posts to most of the corpus | [section 8.3](#s8.3) |
| S12 | A hit carries hashes and a `Declaration`, not names | Resolved names in a hit | The consumer's tables are its own; the wire type is the application's | [section 8.1](#s8.1) |
| S13 | Hits order by archive, natural chunk path, id | Hash order | A reader scans by archive and chunk; `Skin2` precedes `Skin10` | [section 8.3](#s8.3) |
| S14 | The tree is the object namespace, one node per object | A tree of chunk paths; a row per declaration | A reader edits objects; a path is one thing; its declarations hang on it | [section 9](#s9) |
| S15 | The sorted path list is cut once per `GameSearch` | Sorting per listing | 384 thousand paths sort in a tenth of a second; a directory lists in microseconds | [section 9](#s9) |
| S16 | A listing holds at most `limit` entries | Unbounded listings | The unnamed directory holds 48 thousand objects | [section 9](#s9) |
| S17 | Chunk reading and rendering stay in the application | A render in the crate | Rendering needs `ltk_ritobin`; the crate names nothing | `game-index.md` [section 3](game-index.md#s3) |
| S18 | Result types derive `Serialize` and `Deserialize` with field names | Positional, as the cache rows | A consumer forwards a result over IPC or maps it; both read field names | [section 8.1](#s8.1) |
| S19 | Cache arrays and buffers are MessagePack byte strings | Integer arrays | Fifty million ids load in one copy | [section 6.4](#s6.4) |
| S20 | A sheet carries a case mask | A lowercased sheet and a lookup per hit; two buffers | Names read as written at an eighth of a second buffer | [section 7.1](#s7.1) |
| S21 | `matches` is public beneath `search` | One ranked entry point | The manager ranks with band and score; the LSP pages | [section 8.1](#s8.1) |
| S22 | `Class` is the declared class and `Embedded` the nested ones | One `Class` kind | The References document asks the two questions apart | [section 5](#s5) |
| S23 | A hash query also scans the text | Lookup alone | Eight hex digits are a text too; the union needs no rule for which | [section 8.2](#s8.2) |
| S24 | A query carries `called_off` | Unbounded scans | A one-letter query reaches every string term | [section 8.2](#s8.2) |
| S25 | `Dir` is an enum and the unnamed directory is `TreeEntry::Unnamed`; the crate reserves no path token | `<unnamed>` or `?` as a path | A consumer's token is its own | [section 9](#s9) |
| S26 | The path list orders by `path_cmp` | Byte order | A node, its descendants and its siblings are one run | [section 9](#s9) |
| S27 | Folding is a listing option | Always or never folded | The manager folds; the LSP does not | [section 9](#s9) |
| S28 | A sheet exposes its buffer | A regex dependency | A consumer's regular expression runs over the buffer | [section 7.1](#s7.1) |

## <a id="appendix-a"></a>Appendix A. Measurements

Taken 2026-10-23 by the `ritobin-lsp` implementation this feature is specified from, on one
installation at `C:\Riot Games\League of Legends\Game` read over a WSL mount, 16 threads,
`ltk_wad` 0.5.10, `ltk_meta` 0.9.1, Mimir manifest of 2026-10-19. The LTK Manager figures are
from `docs/research/bin-object-index.md` in its repository, 2026-09-05, 392 archives, 8 threads,
warm file cache.

| Metric | Value |
| --- | --- |
| archives | 394 |
| chunks | 921,530 |
| bin chunks (`PROP` + `PTCH`) | 50,550 (235 `PTCH`) |
| objects | 466,863 |
| decoded bin bytes | 2.4 GB |
| rendered ritobin text, hashes unresolved | 8.3 GB, 186 M lines |
| `Chunks` table entries | 2,324,172 |
| `Entries` / `Hashes` / `Fields` / `Types` entries | 444,169 / 418,634 / 10,198 / 3,871 |
| archives walked (first holders) / chunks after dedup by hash | 384 / 824,375 |
| bins / declarations | 44,521 / 447,791 |
| distinct hash terms / string terms | 1,459,617 / 1,566,481 |
| chunks failing to decode | 3 |
| walk, 16 threads, full decode | 25.8 s |
| chunk index, name sheets and walk, cold cache | 43.5 s |
| chunk index, name sheets and term index, warm cache, integer-array encoding | 5.5 s to 7.8 s |
| postings / strings sheet / `Chunks` sheet before S9 | 202 MB / 100 MB / 211 MB |
| `aatrox`, every kind, 6,438 declarations | 45 ms |
| `aatrox`, `Object` only, 2,062 declarations | 4 ms |
| `mname`, `Field` only, 25,376 declarations | 6 ms |
| root listing of the object tree, first call | 200 ms |
| LTK Manager declarations-only build, 8 threads, streaming | 445 ms |
| LTK Manager naming 350,008 objects | 399 ms |

The `measure_a_real_installation` test of `tests/search.rs`, 2026-10-23, the same installation
and tables, release build, 16 threads, warm file cache, `ltk_meta` 0.9.0:

| Metric | Value |
| --- | --- |
| chunk index build | 2.0 s, 824,375 chunks |
| name sheets cut, five tables | 1.3 s |
| term walk, `ltk_meta::walk` over the streamed bytes | 6.3 s to 6.4 s, 44,524 bins, 447,791 declarations, 423,558 objects |
| term walk, every object decoded to a `BinObject` | 7.5 s to 11.0 s, the same terms |
| hash terms / string terms / strings sheet | 1,459,635 / 1,566,481 / 88 MB |
| chunks whose walk fails / chunks that do not read | 0 / 3 |
| declarations-only object build | 3.7 s, storage order equal to the term walk's |
| term cache file / save / load | 288 MB / 2.1 s / 0.4 s |
| term merge: intern, sort, postings | 0.4 s, 0.2 s, 0.15 s |
| `prepare` | 224 ms |
| `aatrox`, every kind, after `prepare` | 11 ms, 6,438 declarations |
| `aatrox`, `Object` only | 10 ms, 2,062 declarations |
| `mname`, `Field` only | 9 ms, 25,376 declarations |
| `a`, every kind | 115 ms, 446,506 declarations, 2.7 million matched terms |
| root listing, after `prepare` | 0.1 ms |
| `Characters/Aatrox` listing | 11 us |
