---
issue: 239
title: "Overlay: apply declared overrides"
labels: enhancement, ltk_overlay
---

Part of #191 (design: [`docs/design/game-data.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md)
[section 6](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md#s6)).
The overlay reads a layer's override files through its content provider and applies them at
the override phase of each batch, with the two new diagnostic kinds.

## Proposed surface

```rust
pub trait ModContentProvider: Send + Sync {
    /* … */
    /// The bytes of the layer's override file at a layer-relative path.
    fn read_game_data_resource(&mut self, layer: &str, path: &str) -> Result<Vec<u8>> {
        Err(ModContentError::GameDataResourceUnsupported.into())
    }
}

#[non_exhaustive]
pub enum ModContentError { /* … */ GameDataResourceUnsupported }

#[non_exhaustive]
pub enum GameDataDiagnosticKind {
    DeclarationsRejected,
    TargetSkipped,
    EntryUnresolved,
    EntryFanOut,
    IndexUnavailable,
    OverrideUnreadable,
    OverrideInvalid,
    OverrideRecordSkipped,
    LinkRemovalUnmatched,
    Unknown,
}
pub struct GameDataDiagnostic {
    /* … */
    /// The record of an `OverrideRecordSkipped` diagnostic.
    pub record: Option<SkippedRecord>,
}
```

- Filesystem, modpkg, and Fantome providers implement the read: `content/<layer>/<path>`
  with containment; the layer's WAD-less chunk; `META/game_data/<layer>/<path>`.
- Each override file is read once per build and the bytes shared across applications.
- `OverrideUnreadable` and `OverrideInvalid` carry the edit index; `OverrideRecordSkipped`
  carries the `SkippedRecord` in `record`. All are reported per application and persist
  through cached builds; `record` decodes absent as `None`.
- The target's final bytes replace the chunk in every holding WAD.

Blocked by #236. The archive cases rest on #237.

- [ ] A directory mod whose manifest names a `PTCH` setting one property of a game bin produces an overlay chunk with that property set and no diagnostic
- [ ] A `PTCH` naming a missing object reports one `OverrideRecordSkipped` with `chunk`, `origin`, `step`, and `record` set, and the other records apply
- [ ] A `PROP` file named as an override reports `OverrideInvalid`, a missing file reports `OverrideUnreadable`, and the batch's links apply
- [ ] A modpkg and a Fantome archive each apply a packed override file
- [ ] A cached build replays the new kinds; an unknown kind decodes to `Unknown`
- [ ] Lints, format, and docs clean
