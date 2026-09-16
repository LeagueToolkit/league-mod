---
issue: 240
title: "Game data: .rito override files"
labels: enhancement, On Hold
---

Part of #191 (design: [`docs/design/game-data.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md)
[section 4](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md#s4)).
The game-data reference accepts a `.rito` override file, ritobin text of type `PTCH`, and
has packing convert it to `.ptch`. `ltk_ritobin` 0.8.0 recognises the `PTCH` root kind and
lowers a document to `ltk_meta::Bin` only; it has no `BinOverride` lowering and no record
grammar.

Parked. The slice is scheduled when `ltk_ritobin` lowers a `#PTCH_text` document to
`BinOverride`. Rule D16 states the interim behaviour: a `.rito` path is an error naming the
extension.

## Proposed surface

```rust
impl TryFrom<String> for OverridePath { /* accepts `.rito` */ }

// ltk_mod_project::game_data
pub struct OverrideFile {
    pub path: OverridePath,
    pub source: Utf8PathBuf,
    /// The compiled `PTCH` bytes: the file's own for `.ptch`, converted for `.rito`.
    pub bytes: Vec<u8>,
}
```

- A `.rito` file whose type is not `PTCH` is a loading error.
- Packing stores the compiled `.ptch` under the authored path with its extension replaced;
  the packed declaration document carries the `.ptch` spelling.
- An extracted project carries the `.ptch` file; source preservation of the `.rito` text is
  outside this slice.

Blocked by #236 and #237, and by `PTCH` lowering in `ltk_ritobin`.

- [ ] A `.rito` override of type `PTCH` compiles to the same bytes `ltk_meta` writes for the equivalent `BinOverride`
- [ ] A `.rito` override of type `PROP` refuses the layer's declarations
- [ ] A modpkg round trip stores `patch.ptch` for an authored `patch.rito` and the document names `patch.ptch`
- [ ] Lints, format, and docs clean
