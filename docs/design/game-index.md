# Game index

## <a id="s1"></a>1. Summary

`ltk_game_index` indexes one League of Legends installation: every chunk of every archive under
`Game/DATA/FINAL`, and, behind the `objects` feature, every bin object those chunks declare. The
chunk index answers which archives hold a chunk. The object index answers which chunks declare an
object. Both are hash-keyed, cached to disk under one game fingerprint, and free of display names.
`ltk_overlay` consumes both for WAD distribution and declaration selectors. LTK Manager consumes
both and keeps its own browsing, searching, and lifecycle code on top
([ADR-0009](../adr/0009-game-index-crate.md)).

## <a id="s2"></a>2. Vocabulary

- **Installation:** A `Game` directory containing `DATA/FINAL`.
- **Archive:** One `.wad.client` file under `DATA/FINAL`. An archive has a **name**, its
  `DATA/FINAL`-relative path with forward slashes, and an absolute **path**.
- **Archive id:** The ordinal of an archive in the index's sorted archive list.
- **Chunk:** One entry of an archive's table of contents, keyed by its **chunk hash**, the XXH64
  with seed zero of the ASCII-lowercased chunk path.
- **Holder:** An archive containing a chunk. A chunk shipped in several archives has several
  holders.
- **Copy:** One holder's instance of a chunk, with that holder's table-of-contents checksum.
- **Chunk row:** What the chunk index stores for one chunk hash: the uncompressed size and every
  copy.
- **Fingerprint:** A `u64` derived from the size and modification time of every archive of an
  installation. Two installations with equal fingerprints index identically.
- **Cache:** The serialized form of an index on disk, tagged with a format version and the
  fingerprint it was built from.
- **Skipped archive:** An archive the build could not open or mount. The build records it and
  indexes the rest.
- **Object:** One bin entry, keyed by its `BinHash`, with a class hash.
- **Declaring chunk:** A bin chunk containing an object. An object present in several bin chunks
  has several declaring chunks.
- **Declaration:** One object in one declaring chunk: object hash, class hash, chunk hash, and
  the copy's archive id.
- **Resolver:** A caller-supplied source of chunk paths for chunk hashes, consulted only by the
  object build.
- **Sniffing:** Decoding the head of a chunk far enough to read its magic and decide whether it
  is a bin.

The crate does not use **file** for a chunk, **WAD** for an archive, **entry** for an object, or
**holder** for a declaring chunk.

## <a id="s3"></a>3. Crate boundary

`ltk_game_index` owns archive enumeration, the chunk table, the fingerprint, the cache format, the
object rows, and the resolver trait. It depends on `ltk_wad`, `ltk_hash`, `camino`, `serde`,
`rmp-serde`, `thiserror`, `tracing`, `walkdir`, and `xxhash-rust`. The `objects` feature adds
`ltk_meta` and `ltk_file`. The `rayon` feature, on by default, adds `rayon`.

Outside the crate:

- Directory trees, folded listings, ranked or exhaustive search, cancellation generations, and
  IPC wire types. These belong to the application.
- Display names for chunks, objects, and classes. A consumer resolves them through its own tables.
- Reading chunk bytes. A consumer mounts the archive at `Archive::path` with `ltk_wad`.
- Game-layout policy over archive names: locale archives, SubChunkTOC block lists, and content
  hashing of chunk bytes. `ltk_overlay` owns these as functions over the index.
- Where a cache file lives. The consumer passes the path.

```
ltk_game_index
|-- GameIndex            chunk table, archives, fingerprint, cache
|-- ObjectIndex          [objects] declarations, cache
|-- ResolveWadPath       resolver trait, implemented by every ltk_wad::PathResolver
|-- errors               BuildError, CacheError, ArchiveLookupError, ArchiveReadError,
                         ObjectBuildError
```

## <a id="s4"></a>4. Archives and identity

```rust
/// The ordinal of an archive in `GameIndex::archives()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ArchiveId(u32);

impl ArchiveId {
    pub fn index(self) -> usize;
}
impl fmt::Display for ArchiveId { /* `#` and the ordinal */ }

/// One `.wad.client` of the installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Archive {
    /// `DATA/FINAL`-relative, forward slashes: `Champions/Aatrox.wad.client`.
    pub name: String,
    /// Absolute path of the file.
    pub path: Utf8PathBuf,
}

impl Archive {
    /// The last segment of `name`: `Aatrox.wad.client`.
    pub fn file_name(&self) -> &str;
}
```

The archive list is sorted by `name` with byte order. Archive ids are dense from zero in that
order. An archive's id is stable for one index and its caches, and is not stable across builds
against a changed installation.

The build enumerates every file under `DATA/FINAL` whose name ends in `.wad.client`,
case-insensitively. Files with any other suffix, including `.wad.SubChunkTOC`, are not archives.

## <a id="s5"></a>5. Chunk index

### <a id="s5.1"></a>5.1 Rows

```rust
/// One copy of a chunk in one holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChunkCopy {
    pub archive: ArchiveId,
    /// The holder's table-of-contents checksum for this chunk.
    pub checksum: u64,
}

/// What the index stores for one chunk hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRow { /* size, copies */ }

impl ChunkRow {
    /// Uncompressed size in bytes, as the first holder's table of contents states it.
    pub fn size(&self) -> u64;
    /// Every copy, in archive id order.
    pub fn copies(&self) -> &[ChunkCopy];
    /// Every holder, in archive id order.
    pub fn holders(&self) -> impl Iterator<Item = ArchiveId> + '_;
    /// The first holder in archive id order.
    pub fn first_holder(&self) -> ArchiveId;
    /// Whether every copy carries the same checksum.
    pub fn is_consistent(&self) -> bool;
}
```

A row has at least one copy. Copies are deduplicated per archive: a table of contents listing one
hash twice contributes one copy.

### <a id="s5.2"></a>5.2 Surface

```rust
/// Every chunk of an installation, keyed by chunk hash, with every holder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameIndex { /* archives, rows, filename index, fingerprint, skipped */ }

impl GameIndex {
    // Building (section 6)
    pub fn build(game_dir: &Utf8Path) -> Result<Self, BuildError>;
    pub fn build_from_archives(root: &Utf8Path, archives: &[Utf8PathBuf]) -> Result<Self, BuildError>;

    // Caching (section 7)
    pub fn load(cache_path: &Utf8Path) -> Result<Self, CacheError>;
    pub fn load_for(cache_path: &Utf8Path, game_dir: &Utf8Path) -> Result<Self, CacheError>;
    pub fn save(&self, cache_path: &Utf8Path) -> Result<(), CacheError>;
    pub fn load_or_build(game_dir: &Utf8Path, cache_path: &Utf8Path) -> Result<Self, BuildError>;
    pub fn fingerprint_of(game_dir: &Utf8Path) -> Result<Fingerprint, BuildError>;

    // Archives
    pub fn archives(&self) -> &[Archive];
    pub fn archive(&self, id: ArchiveId) -> &Archive;
    pub fn archive_by_file_name(&self, file_name: &str) -> Result<ArchiveId, ArchiveLookupError>;
    pub fn skipped(&self) -> &[SkippedArchive];

    // Chunks
    pub fn row(&self, hash: WadHash) -> Option<&ChunkRow>;
    pub fn row_by_path(&self, path: &str) -> Option<&ChunkRow>;
    pub fn holders(&self, hash: WadHash) -> impl Iterator<Item = ArchiveId> + '_;
    pub fn contains(&self, hash: WadHash) -> bool;
    pub fn chunks(&self) -> impl ExactSizeIterator<Item = (WadHash, &ChunkRow)>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn dominant_holder(&self, hashes: &[WadHash]) -> Option<ArchiveId>;

    pub fn fingerprint(&self) -> Fingerprint;
}

/// The chunk hash of a path: XXH64, seed zero, over the ASCII-lowercased path.
pub fn chunk_hash(path: &str) -> WadHash;
```

- `archive(id)` panics on an id from another index. The id type is only obtainable from this
  index or its cache.
- `archive_by_file_name` compares ASCII case-insensitively against `Archive::file_name`. One match
  is `Ok`. No match is `ArchiveLookupError::Absent`. Several matches are
  `ArchiveLookupError::Ambiguous` carrying every candidate in id order.
- `holders(hash)` is empty for an absent chunk. `row_by_path` hashes with `chunk_hash`.
- `chunks()` iterates in ascending chunk hash order.
- `dominant_holder` returns the archive with the most hits over `hashes`, ties broken by the
  lower id, and `None` when no hash is present.
- `chunk_hash` is the only hashing rule the crate applies. `Target::chunk_hash()` in
  `ltk_game_data` computes the same value.

## <a id="s6"></a>6. Building the chunk index

`build(game_dir)` requires `game_dir/DATA/FINAL` to exist and enumerates its archives
([section 4](#s4)). `build_from_archives(root, archives)` takes explicit files; each name is the
path relative to `root` with forward slashes. Both sort the archives by name and index them in
that order.

For each archive the build opens the file, mounts the table of contents with `ltk_wad`, and adds
one copy per chunk. An archive that cannot be opened or mounted is a skipped archive: the build
records `SkippedArchive { archive, error }` and continues. The archive keeps its id and its
place in `archives()`. A build with every archive skipped is an empty index with a full skipped
list, and is `Ok`.

With the `rayon` feature, archives mount in parallel and rows merge in archive id order. Without
it, archives mount serially. The result is identical.

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    #[error("{path} is not a League installation: it has no DATA/FINAL directory")]
    MissingDataFinal { path: Utf8PathBuf },
    #[error("cannot enumerate archives under {path}")]
    Enumerate { path: Utf8PathBuf, #[source] source: std::io::Error },
    #[error("archive {archive} is not under {root}")]
    ArchiveOutsideRoot { root: Utf8PathBuf, archive: Utf8PathBuf },
    #[error("cannot read metadata of {path}")]
    Metadata { path: Utf8PathBuf, #[source] source: std::io::Error },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ArchiveReadError {
    /// The message of the `std::io::Error`.
    #[error("cannot open the archive: {0}")]
    Open(String),
    /// The message of the `ltk_wad::WadError`.
    #[error("cannot mount the archive: {0}")]
    Mount(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SkippedArchive {
    pub archive: ArchiveId,
    pub error: ArchiveReadError,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArchiveLookupError {
    #[error("no archive is named {file_name}")]
    Absent { file_name: String },
    #[error("{file_name} names {} archives", candidates.len())]
    Ambiguous { file_name: String, candidates: Vec<ArchiveId> },
}
```

`BuildError` covers enumeration and metadata. Per-archive read failures never fail a build.
A skipped archive is part of the index: it survives the cache and takes part in index equality.
`ArchiveReadError` carries the message of the failure, not the source error.

## <a id="s7"></a>7. Fingerprint and cache

```rust
/// Identity of an installation's archive set: sizes and modification times.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint(u64);

impl Fingerprint {
    pub fn as_u64(self) -> u64;
}
impl fmt::Display for Fingerprint { /* 16 lowercase hex digits */ }
impl fmt::LowerHex for Fingerprint {}
```

The fingerprint is XXH3-64 over the sorted archive names, each followed by its file size and
modification time in nanoseconds since the Unix epoch. Skipped archives contribute their metadata.
`fingerprint_of(game_dir)` computes it without mounting an archive.

A cache is a MessagePack document of three consecutive values: the format version, the
fingerprint, and the index body. The chunk index format version is the public constant
`CACHE_FORMAT_VERSION`, bumped on any change to `Archive`, `ChunkRow`, `SkippedArchive`, or the
container layout. Ids and rows serialize positionally, as integers. The version is read first. A
foreign version is reported without decoding the body.

- `load(path)` returns the cached index regardless of fingerprint. `CacheError::Version` reports a
  format version other than the crate's.
- `load_for(path, game_dir)` computes `fingerprint_of(game_dir)` and returns `CacheError::Stale`
  when it differs from the cached one.
- `save(path)` creates the parent directory and writes atomically: to a sibling temporary
  file, then renamed over `path`.
- `load_or_build(game_dir, cache_path)` returns `load_for` on success. On any `CacheError` it logs
  at debug for a missing file and at warn otherwise, builds, saves best-effort with a warn on
  failure, and returns the built index.

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    #[error("cannot read cache {path}")]
    Read { path: Utf8PathBuf, #[source] source: std::io::Error },
    #[error("cannot write cache {path}")]
    Write { path: Utf8PathBuf, #[source] source: std::io::Error },
    #[error("cache {path} is not a valid index")]
    Decode { path: Utf8PathBuf, #[source] source: rmp_serde::decode::Error },
    #[error("cache {path} cannot be encoded")]
    Encode { path: Utf8PathBuf, #[source] source: rmp_serde::encode::Error },
    #[error("cache {path} has format version {found}, expected {expected}")]
    Version { path: Utf8PathBuf, found: u32, expected: u32 },
    #[error("cache {path} was built for fingerprint {cached}, installation is {current}")]
    Stale { path: Utf8PathBuf, cached: Fingerprint, current: Fingerprint },
    #[error(transparent)]
    Build(#[from] BuildError),
}

impl CacheError {
    /// Whether the error is a `Read` of a file that does not exist.
    pub fn is_missing_file(&self) -> bool;
}
```

## <a id="s8"></a>8. Resolver

```rust
/// A source of chunk paths for chunk hashes.
pub trait ResolveWadPath {
    /// Visits `(index, path)` for every hash in `hashes` the source names.
    fn for_each_named(&self, hashes: &[WadHash], visit: &mut dyn FnMut(usize, &str));
}
```

The batch shape lets a disk-backed table answer one query per slice. The chunk index never takes a
resolver. The object build takes an optional one ([section 9.2](#s9.2)).

Every `T: ltk_wad::PathResolver` implements `ResolveWadPath` through `resolve_all`, one answer
per hash. `ltk_hashtable::GameResolver` is the implementation this workspace uses. LTK Manager
implements the trait over its layered hash database.

## <a id="s9"></a>9. Object index

Available with the `objects` feature.

### <a id="s9.1"></a>9.1 Rows

```rust
/// One object in one declaring chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Declaration {
    pub object: BinHash,
    pub class: BinHash,
    pub chunk: WadHash,
    /// The copy of `chunk` the build read.
    pub archive: ArchiveId,
}
```

Declarations are stored in archive id order, and within one archive in the order the build read
its bin chunks: named `.bin` chunks in ascending chunk hash order, then bare-named chunks, then
unnamed chunks. One object declared twice in one chunk contributes two declarations.

### <a id="s9.2"></a>9.2 Surface

```rust
/// Every bin object of an installation, keyed by object hash, with every declaring chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectIndex { /* declarations, by_object, by_chunk, stats, skipped, fingerprint */ }

/// Built as `BuildOptions::default()` with fields assigned.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct BuildOptions<'a> {
    /// Names chunks. `None` sniffs every chunk.
    pub resolver: Option<&'a dyn ResolveWadPath>,
    /// Archives read at once. `None` is the available parallelism.
    pub workers: Option<NonZeroUsize>,
    /// Polled before each archive. `true` stops the build.
    pub called_off: Option<&'a (dyn Fn() -> bool + Sync)>,
}

impl ObjectIndex {
    pub fn build(game: &GameIndex) -> Result<Self, ObjectBuildError>;
    pub fn build_with(game: &GameIndex, options: &BuildOptions<'_>) -> Result<Self, ObjectBuildError>;

    pub fn load_for(cache_path: &Utf8Path, game: &GameIndex) -> Result<Self, CacheError>;
    pub fn save(&self, cache_path: &Utf8Path) -> Result<(), CacheError>;
    pub fn load_or_build_with(
        game: &GameIndex,
        cache_path: &Utf8Path,
        options: &BuildOptions<'_>,
    ) -> Result<Self, ObjectBuildError>;

    pub fn declares(&self, object: BinHash) -> bool;
    /// Every declaration of `object`, in storage order.
    pub fn declarations(&self, object: BinHash) -> &[Declaration];
    /// Every declaration in `chunk`, in storage order.
    pub fn chunk_declarations(&self, chunk: WadHash) -> impl Iterator<Item = &Declaration>;
    pub fn objects(&self) -> impl ExactSizeIterator<Item = BinHash> + '_;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;

    pub fn stats(&self) -> &ObjectStats;
    pub fn skipped(&self) -> &[SkippedArchive];
    pub fn fingerprint(&self) -> Fingerprint;
}

/// Visits `(object, class)` for every object one bin declares.
pub fn for_each_declaration(
    reader: impl Read + Seek,
    visit: impl FnMut(BinHash, BinHash),
) -> Result<(), ltk_meta::Error>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ObjectStats {
    /// Archives with at least one chunk read or sniffed.
    pub archives: u32,
    /// Bin chunks read, named or sniffed, readable or not.
    pub bins: u32,
    /// Chunks decoded to their magic.
    pub sniffed: u32,
    /// Sniffed chunks whose magic is a bin's.
    pub sniffed_bins: u32,
    pub declarations: u32,
    /// Chunks that did not read.
    pub skipped_chunks: u32,
    /// Decompressed bytes read.
    pub bytes: u64,
    pub elapsed: Duration,
    pub workers: u32,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ObjectBuildError {
    #[error("the object index build was called off")]
    CalledOff,
}
```

### <a id="s9.3"></a>9.3 Building

The build partitions the chunk index's rows by first holder, one job per archive. With a resolver,
`for_each_named` runs once over every chunk hash. A named chunk whose path ends in `.bin` is a bin.
A named chunk whose last segment has no extension is bare-named and is sniffed. An unnamed chunk is
sniffed. Any other named chunk is not read. Without a resolver every chunk is sniffed.

Sniffing decodes the chunk up to `ltk_file::MAX_MAGIC_SIZE` bytes from a raw prefix of 16 KiB,
retried at 256 KiB when the first block is cut short, and reads it whole when
`LeagueFileKind::identify_from_bytes` reports `PropertyBin` or `PropertyBinOverride`. A `PROP`
is read through `ltk_meta::stream::BinStream::entries`, which yields object and class hashes
without decoding values. A `PTCH` is read through `ltk_meta::BinOverride::from_reader`. A chunk
that fails to decompress or parse counts in `skipped_chunks` and produces no declarations.

Jobs run on `workers` threads with the `rayon` feature and serially without it. `called_off` is
polled before each job. A build that observes `true` returns `ObjectBuildError::CalledOff`. An
archive that cannot be opened or mounted is a skipped archive, as in the chunk build.

The object index's fingerprint is its chunk index's. `load_for(path, game)` returns
`CacheError::Stale` when the cached fingerprint differs from `game.fingerprint()`. The object
cache has its own format version, the public constant `OBJECT_CACHE_FORMAT_VERSION`.
`load_or_build_with` follows the chunk index's load-or-build rule ([section 7](#s7)).

An archive skipped by the object build counts its chunks in `skipped_chunks`. `sniffed` counts
every chunk decoded to its magic, bare-named and unnamed alike.

## <a id="s10"></a>10. Consumers

`ltk_overlay` holds a `GameIndex` per build through `load_or_build`, and an `ObjectIndex` only
for a build in which an enabled layer declares an `entries` module
(`game-data.md` [section 6](game-data.md#s6)). Its content hashing, locale archive lookup,
SubChunkTOC block list, and base selection are functions over `GameIndex`. The overlay's cache
files are `game_index.bin` and `object_index.bin` in its state directory.

LTK Manager builds its folded directory tree from `chunks()` and `archives()`, with each row's
first holder as the archive a file reads from, and builds its object rows and browsing on
`ObjectIndex`. Its display names come from its own tables. Its lifecycle slots wrap the two
indexes without the crate's knowledge.

## <a id="s11"></a>11. Validation

Fixture archives are written with `ltk_wad`'s builder into a temporary `DATA/FINAL` tree, so
every test runs `build` or `build_from_archives` on the real path. Cases cover: archive sorting and
ids; a chunk in several holders with equal and unequal checksums; case-insensitive file-name
lookup with absent and ambiguous names; a skipped archive keeping its id; `dominant_holder` ties;
`chunk_hash` against `ltk_wad::WadHash::hash_str`; fingerprint stability across a rebuild and
change on a touched archive; cache round trip, version rejection, and stale rejection; atomic
save; with `objects`, a `PROP` and a `PTCH` fixture with named, bare-named, and unnamed chunks,
declarations order, reverse lookup, `called_off`, the sniff-everything path without a resolver,
and cache staleness following the chunk index.

## <a id="s12"></a>12. Rules

| ID | Rule | Instead of | Why | Spec |
| --- | --- | --- | --- | --- |
| G1 | One published crate holds the chunk index and the object index | An index per consumer | Two consumers share one build and one cache | [ADR-0009](../adr/0009-game-index-crate.md) |
| G2 | The object index is a feature | A second crate | `ltk_meta` and a full bin read stay optional | [ADR-0009](../adr/0009-game-index-crate.md) |
| G3 | An archive is an ordinal id | A path per holder | Rows are `Copy`-sized and the cache is small | [section 4](#s4) |
| G4 | A row keeps every holder | First holder only | Cross-archive distribution needs every copy | [section 5.1](#s5.1) |
| G5 | A row carries per-copy checksums | Size only | Copy consistency is answerable without mounting | [section 5.1](#s5.1) |
| G6 | An unreadable archive is skipped and recorded | A failed build | One corrupt file does not blank an install | [section 6](#s6) |
| G7 | The fingerprint covers skipped archives | Readable archives only | A repaired archive invalidates the cache | [section 7](#s7) |
| G8 | Caches are MessagePack with a version and a fingerprint | A new binary format | Both consumers already read it | [section 7](#s7) |
| G9 | Names enter through a batch resolver trait | A hashtable dependency | Each consumer keeps its own tables | [section 8](#s8) |
| G10 | Rows carry no display names | Resolved paths in rows | A hashtable update never invalidates a cache | [section 5.1](#s5.1), [section 9.1](#s9.1) |
| G11 | The resolver is optional | A required resolver | A build without tables is correct, only slower | [section 9.3](#s9.3) |
| G12 | Trees, search, and lifecycle stay in the application | An index with a browser | Two applications want two browsers | [section 3](#s3) |
| G13 | Game-layout name policy stays in the overlay | Locale and SubChunkTOC rules in the index | The rules belong to WAD distribution | [section 3](#s3) |
| G14 | The object fingerprint is the chunk index's | A second fingerprint | One staleness rule for both caches | [section 9.3](#s9.3) |
