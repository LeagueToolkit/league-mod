---
issue: 230
title: "Game index: fingerprint and cache"
labels: enhancement
---

Part of #228 (design: [`docs/design/game-index.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md)
[section 7](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md#s7)). Adds the installation fingerprint and the MessagePack cache
with versioned, fingerprint-checked load and atomic save.

## Proposed surface

```rust
pub struct Fingerprint(u64);
impl Fingerprint { pub fn as_u64(self) -> u64; }
impl fmt::Display for Fingerprint {}   // 16 lowercase hex digits
impl fmt::LowerHex for Fingerprint {}
impl GameIndex {
    pub fn fingerprint(&self) -> Fingerprint;
    pub fn fingerprint_of(game_dir: &Utf8Path) -> Result<Fingerprint, BuildError>;
    pub fn load(cache_path: &Utf8Path) -> Result<Self, CacheError>;
    pub fn load_for(cache_path: &Utf8Path, game_dir: &Utf8Path) -> Result<Self, CacheError>;
    pub fn save(&self, cache_path: &Utf8Path) -> Result<(), CacheError>;
    pub fn load_or_build(game_dir: &Utf8Path, cache_path: &Utf8Path) -> Result<Self, BuildError>;
}
pub enum CacheError { Read, Write, Decode, Encode, Version, Stale, Build }
```

- The fingerprint is XXH3-64 over sorted archive names with each file's size and modification
  time; skipped archives contribute their metadata.
- The cache is MessagePack: format version, fingerprint, body. Ids and rows serialize as integers.
- `save` writes to a sibling temporary file and renames over the target.
- `load_or_build` logs a missing cache at debug and any other cache error at warn, builds, saves
  best-effort, and returns the built index.

Blocked by #229.

- [ ] A rebuild of an untouched fixture tree yields the same fingerprint; touching one archive's modification time changes it
- [ ] Round trip: `save` then `load` yields an index equal to the original
- [ ] A cache with a foreign format version loads as `CacheError::Version`
- [ ] `load_for` against a changed tree is `CacheError::Stale` carrying both fingerprints
- [ ] A failed `save` leaves no partial file at `cache_path`
- [ ] `load_or_build` with an unreadable cache path returns a built index
- [ ] Lints, format, and docs clean
