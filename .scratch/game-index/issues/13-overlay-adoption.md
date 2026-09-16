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

// ltk_overlay, functions over ltk_game_index::GameIndex
pub(crate) fn localized_global_wads(index: &GameIndex) -> Vec<(String, ArchiveId)>;
pub(crate) fn subchunktoc_blocked(index: &GameIndex) -> HashSet<WadHash>;
pub(crate) fn compute_content_hashes_batch(index: &GameIndex, hashes: &HashSet<WadHash>) -> HashMap<WadHash, ContentHash>;
```

- `find_wad` becomes `archive_by_file_name`; `AmbiguousWad` maps from
  `ArchiveLookupError::Ambiguous`. `find_wads_with_hash(h).min()` becomes
  `row(h).map(ChunkRow::first_holder)`. `find_best_matching_wad` becomes `dominant_holder`.
- `GameDirError::MissingDataFinal` maps from `BuildError::MissingDataFinal`. A nonempty
  `skipped()` list is reported as build warnings, not an error.
- `OverlayState` stores `Fingerprint`; `game_index.bin` is read through `load_or_build` and
  its format version bump invalidates the previous file.
- `ModWadReport.game_index_fingerprint` keeps its `u64` wire type via `Fingerprint::as_u64`.

Blocked by #230.

- [ ] `ltk_overlay` has no `game_index` module and no `GameIndex` export; `cargo build --workspace` passes
- [ ] Every existing overlay test passes with fixture archives built through `build_from_archives`
- [ ] A stale `game_index.bin` from the previous format is rebuilt without error
- [ ] A skipped archive surfaces in the build result's warnings
- [ ] Lints, format, and docs clean
