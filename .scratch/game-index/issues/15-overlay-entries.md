---
issue: 234
title: "Overlay: resolve entries through the object index"
labels: enhancement, ltk_overlay
---

Part of #228 (design: [`docs/design/game-data.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md)
[section 6](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md#s6)). Lowers `Selector::Entries` to per-chunk applications through
`ObjectIndex`, adds the three diagnostic kinds, and builds the object index lazily.

## Proposed surface

```rust
pub enum GameDataDiagnosticKind {
    DeclarationsRejected,
    TargetSkipped,
    EntryUnresolved,
    EntryFanOut,
    IndexUnavailable,
    LinkRemovalUnmatched,
    Unknown,
}
pub enum OverlayStage { Indexing, IndexingObjects, CollectingOverrides, PatchingWad, ApplyingStringOverrides, Complete }
```

- The object index is loaded or built from `object_index.bin` only when an enabled layer's
  declarations contain an `entries` module, under `OverlayStage::IndexingObjects`, with
  `ltk_overlay` depending on `ltk_game_index` with the `objects` feature.
- Each entry lowers to one application per declaring chunk, in mapping order, ahead of base
  selection; the per-chunk pipeline is unchanged.
- Several declaring chunks: all are edited and one `EntryFanOut` diagnostic per entry names
  them. None: `EntryUnresolved`, steps skipped. Index failure: `IndexUnavailable` per
  `entries` module plus a build warning; the build continues.
- Diagnostics serialize in camelCase; unknown kinds decode to `Unknown`.

Blocked by #231, #232, and #233.

- [ ] A build with no `entries` module never opens or creates `object_index.bin`
- [ ] An entry declared in two fixture bins is edited in both and reports one `EntryFanOut` naming both chunks
- [ ] An entry no bin declares reports `EntryUnresolved` and leaves every chunk unchanged
- [ ] An unreadable object cache plus a build failure reports `IndexUnavailable` for each `entries` module and the build completes
- [ ] Progress reports `IndexingObjects` exactly once per build that needs it
- [ ] Cached builds replay the new kinds; an `overlay.json` with an unknown kind decodes to `Unknown`
- [ ] Lints, format, and docs clean
