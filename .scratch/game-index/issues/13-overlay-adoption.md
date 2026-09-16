---
issue: 232
title: "Overlay: adopt ltk_game_index"
labels: enhancement, ltk_overlay
---

Part of #228 (design: [`docs/design/game-index.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md)
[section 10](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md#s10), [ADR-0009](https://github.com/LeagueToolkit/league-mod/blob/main/docs/adr/0009-game-index-crate.md)).
Removes `ltk_overlay::GameIndex` and moves the overlay onto the crate. Breaking change to
`ltk_overlay`'s public surface.

## Proposed surface

```rust
// removed
pub struct ltk_overlay::GameIndex;

// ltk_overlay::game
pub struct GameDir(Utf8PathBuf);
impl GameDir {
    pub fn new(path: impl Into<Utf8PathBuf>) -> Self;
    pub fn as_path(&self) -> &Utf8Path;
    pub fn data_final(&self) -> Result<Utf8PathBuf, GameDirError>;
    pub fn join(&self, rel_path: impl AsRef<Utf8Path>) -> Utf8PathBuf;
    pub fn load_or_build_index(&self, state_dir: &StateDir) -> Result<ltk_game_index::GameIndex>;
}
pub struct StateDir(Utf8PathBuf);
impl StateDir {
    pub const GAME_INDEX_CACHE: &str;   // "game_index.bin"
    pub const OVERRIDE_META_CACHE: &str;
    pub const OVERLAY_STATE: &str;
    pub fn new(path: impl Into<Utf8PathBuf>) -> Self;
    pub fn create(&self) -> Result<()>;
    pub fn game_index_cache(&self) -> Utf8PathBuf;      // game_index.bin
    pub fn override_meta_cache(&self) -> Utf8PathBuf;   // override_meta.bin
    pub fn overlay_state(&self) -> Utf8PathBuf;         // overlay.json
}
pub struct SkippedGameArchive { pub name: String, pub path: Utf8PathBuf, pub error: String }

// the overlay's rules over the index, as an extension trait
pub(crate) trait GameIndexExt {
    fn wad_rel_path(&self, id: ArchiveId) -> Utf8PathBuf;              // DATA/FINAL/<name>
    fn is_wad_rel_path(&self, id: ArchiveId, path: &Utf8Path) -> bool;
    fn holder_paths(&self, hash: WadHash) -> impl Iterator<Item = Utf8PathBuf> + '_;
    fn localized_global_wads(&self) -> Vec<(String, ArchiveId)>;
    fn subchunktoc_blocked(&self) -> HashSet<WadHash>;
    fn content_hashes(&self, hashes: &HashSet<WadHash>) -> HashMap<WadHash, ContentHash>;
    fn skipped_archives(&self) -> Vec<SkippedGameArchive>;
}
impl GameIndexExt for ltk_game_index::GameIndex {}

// ltk_overlay::OverlayBuildResult
pub skipped_archives: Vec<SkippedGameArchive>;
```

- `OverlayBuilder::new` and `analyze_single_mod` keep their path parameters and wrap them in
  `GameDir` and `StateDir`.
- `find_wad` becomes `archive_by_file_name`; `AmbiguousWad` and `WadNotFound` map from
  `ArchiveLookupError` through `From`, with `Error::ArchiveLookup` for any further variant of
  the non-exhaustive enum. `GameDirError::WadOutsideGameDir` is removed: every archive path
  comes from the index. `find_wads_with_hash(h).min()` becomes
  `row(h).map(ChunkRow::first_holder)`. `find_best_matching_wad` becomes `dominant_holder`.
- `GameDirError::MissingDataFinal` maps from `BuildError::MissingDataFinal`; any other
  `BuildError` is `Error::GameIndex`. A nonempty `skipped()` list is logged at warn and
  reported on `OverlayBuildResult::skipped_archives`, not an error.
- `OverlayState`, `OverrideMetaCache` and `ModWadReport` keep the fingerprint as `u64` on the
  wire through `Fingerprint::as_u64`. `game_index.bin` is read through `load_or_build`, and a
  file in the previous format is rebuilt.

Blocked by #230.

- [ ] `ltk_overlay` has no `game_index` module and no `GameIndex` export; `cargo build --workspace` passes
- [ ] Every existing overlay test passes with fixture archives built through `build_from_archives`
- [ ] A stale `game_index.bin` from the previous format is rebuilt without error
- [ ] A skipped archive surfaces in the build result's warnings
- [ ] Lints, format, and docs clean
