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
pub struct GameDataDiagnostic {
    pub kind: GameDataDiagnosticKind,
    pub mod_id: String,
    pub layer: String,
    pub target: Option<String>,
    /// The chunk the diagnostic is about; absent from a module-level diagnostic.
    pub chunk: Option<WadHash>,
    pub origin: Option<Origin>,
    pub edit_index: Option<usize>,
    pub message: String,
}
#[non_exhaustive]
pub enum OverlayStage { Indexing, IndexingObjects, CollectingOverrides, PatchingWad, ApplyingStringOverrides, Complete }
impl StateDir { pub const OBJECT_INDEX_CACHE: &str; pub fn object_index_cache(&self) -> Utf8PathBuf; }
impl OverlayBuilder {
    /// Polled before the chunk index loads, before overrides are collected, between the
    /// archives of an object index build, and before each WAD is patched.
    pub fn with_called_off<F: Fn() -> bool + Send + Sync + 'static>(self, called_off: F) -> Self;
}
pub enum Error { /* … */ CalledOff }
```

- The object index is loaded or built from `object_index.bin` only when an enabled layer's
  declarations contain an `entries` module, under `OverlayStage::IndexingObjects`, with
  `ltk_overlay` depending on `ltk_game_index` with the `objects` feature.
- Each entry lowers to one application per declaring chunk, in mapping order, ahead of base
  selection; the per-chunk pipeline is unchanged. A lowered application's diagnostics carry the
  entry name as `target` and the declaring chunk as `chunk`.
- Several declaring chunks: all are edited and one `EntryFanOut` diagnostic per entry names
  them. None: `EntryUnresolved`, edits skipped. Index failure: `IndexUnavailable` per
  `entries` module plus a logged warning; the build continues. An object index build the poll
  stops ends the build with `Error::CalledOff` and writes no state.
- Diagnostics serialize in camelCase; unknown kinds decode to `Unknown`; a missing `chunk`
  decodes to `None`.

Blocked by #231, #232, and #233.

- [ ] A build with no `entries` module never opens or creates `object_index.bin`
- [ ] An entry declared in two fixture bins is edited in both and reports one `EntryFanOut` naming both chunks
- [ ] An entry no bin declares reports `EntryUnresolved` and leaves every chunk unchanged
- [ ] An unreadable object cache is rebuilt; a build failure reports `IndexUnavailable` for each `entries` module and the build completes
- [ ] A poll returning `true` before the index loads, or during the object index build, ends the build with `Error::CalledOff` and writes no state
- [ ] Progress reports `IndexingObjects` exactly once per build that needs it
- [ ] Cached builds replay the new kinds; an `overlay.json` with an unknown kind decodes to `Unknown`
- [ ] Lints, format, and docs clean
