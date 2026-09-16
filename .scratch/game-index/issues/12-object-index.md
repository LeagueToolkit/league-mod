---
issue: 231
title: "Game index: object index and resolver"
labels: enhancement
---

Part of #228 (design: [`docs/design/game-index.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md)
[section 8](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md#s8) and [section 9](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md#s9)). Adds the
`ResolveWadPath` trait, the `hashtable` feature implementing it for `ltk_hashtable` resolvers,
and the `ObjectIndex` behind the `objects` feature, ported from LTK Manager's
`object_index/build.rs`.

## Proposed surface

```rust
pub trait ResolveWadPath {
    fn for_each_named(&self, hashes: &[WadHash], visit: &mut dyn FnMut(usize, &str));
}
#[cfg(feature = "hashtable")]
impl<T: ltk_hashtable::PathResolver> ResolveWadPath for T {}

pub struct Declaration { pub object: BinHash, pub class: BinHash, pub chunk: WadHash, pub archive: ArchiveId }
pub struct BuildOptions<'a> {
    pub resolver: Option<&'a dyn ResolveWadPath>,
    pub workers: Option<NonZeroUsize>,
    pub called_off: Option<&'a (dyn Fn() -> bool + Sync)>,
}
pub struct ObjectIndex { /* declarations, by_object, by_chunk, stats, skipped, fingerprint */ }
impl ObjectIndex {
    pub fn build(game: &GameIndex) -> Result<Self, ObjectBuildError>;
    pub fn build_with(game: &GameIndex, options: &BuildOptions<'_>) -> Result<Self, ObjectBuildError>;
    pub fn load_for(cache_path: &Utf8Path, game: &GameIndex) -> Result<Self, CacheError>;
    pub fn save(&self, cache_path: &Utf8Path) -> Result<(), CacheError>;
    pub fn load_or_build_with(game: &GameIndex, cache_path: &Utf8Path, options: &BuildOptions<'_>) -> Result<Self, ObjectBuildError>;
    pub fn declares(&self, object: BinHash) -> bool;
    pub fn declarations(&self, object: BinHash) -> &[Declaration];
    pub fn chunk_declarations(&self, chunk: WadHash) -> impl Iterator<Item = &Declaration>;
    pub fn objects(&self) -> impl ExactSizeIterator<Item = BinHash> + '_;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn stats(&self) -> &ObjectStats;
    pub fn skipped(&self) -> &[SkippedArchive];
    pub fn fingerprint(&self) -> Fingerprint;
}
pub fn for_each_declaration(reader: impl Read + Seek, visit: impl FnMut(BinHash, BinHash)) -> Result<(), ltk_meta::Error>;
pub struct ObjectStats { /* archives, files, sniffed, sniffed_bins, declarations, skipped_chunks, bytes, elapsed, workers */ }
pub enum ObjectBuildError { CalledOff }
```

- Chunk classification: named `.bin` is read; bare-named and unnamed are sniffed; other named
  chunks are not read; without a resolver everything is sniffed.
- `PROP` reads through `BinStream::entries`; `PTCH` through `BinOverride::from_reader`. Both
  are in published `ltk_meta` 0.8.2 with no git pin.
- Declarations store in archive id order, then named, bare, unnamed within an archive.
- The object cache carries the chunk index's fingerprint and its own format version.

Blocked by #230.

- [ ] `cargo build -p ltk_game_index --features objects,hashtable` and `--no-default-features --features objects`
- [ ] A `PROP` fixture named `x.bin` and an unnamed copy of a `PTCH` fixture both contribute declarations; a named `.dds` chunk is never read
- [ ] `declarations(object)` for an object in two chunks lists both in archive id order; `chunk_declarations` inverts it
- [ ] `build_with` and a `called_off` returning `true` is `CalledOff`
- [ ] Without a resolver, `stats().sniffed` equals the chunk count and declarations match the resolved build
- [ ] `load_for` against a chunk index with another fingerprint is `Stale`
- [ ] Serial and rayon builds produce equal declaration slices
- [ ] Lints, format, and docs clean under every feature combination
