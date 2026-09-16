---
issue: 237
title: "Project: override files through modpkg and Fantome"
labels: enhancement
---

Part of #191 (design: [`docs/design/game-data.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md)
[section 4](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md#s4)
and [section 5](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md#s5)).
Override files are build resources of a layer: classified out of content, read and checked
at load, packed under their layer-relative path in both archives, and placed back on import
(ADR-0013).

## Proposed surface

```rust
// ltk_mod_project::game_data
pub struct LayerDeclarations { pub declarations: Result<Option<Declarations>, Error>, /* … */ }
impl LayerDeclarations {
    pub fn is_declaration_input(&self, path: &Utf8Path) -> bool;
    /// The override files of accepted declarations, one per distinct path in
    /// first-reference order.
    pub fn override_files(&self) -> &[OverrideFile];
}
pub struct OverrideFile { pub path: OverridePath, pub source: Utf8PathBuf }

// ltk_mod_project::pack
impl PlannedLayer {
    pub fn game_data(&self) -> Option<&DeclarationDocument>;
    /// The override files the layer's declarations name.
    pub fn override_files(&self) -> &[OverrideFile];
}

// ltk_fantome
pub enum FantomeEntry<'a> {
    /* … */
    /// A file under `META/game_data/`, at `<layer>/<path>`.
    GameData(&'a str),
}
impl<W: Write + Seek> FantomeWriter<W> {
    /// Write one override file as `META/game_data/{layer}/{path}`.
    pub fn write_game_data_resource(&mut self, layer: &str, path: &str, content: &mut impl Read) -> Result<(), FantomeWriteError>;
}
impl<R: Read + Seek> FantomeReader<R> {
    /// Read `META/game_data/{layer}/{path}`, matched case-insensitively.
    pub fn read_game_data_resource(&mut self, layer: &str, path: &str) -> Result<Option<Vec<u8>>, FantomeExtractError>;
}
```

- `load_layer` reads every override file of accepted declarations: a missing, ignored, or
  escaping file, or one that does not read as a `PTCH`, refuses the layer's declarations.
  Override files are declaration inputs, including those discovered in rejected documents
  through `ReferencedInputs::discover`, resolved against the file naming them.
- Modpkg stores an override file as a chunk of its layer with no WAD at its layer-relative
  path; `extract_layer` places it under the layer's content directory unchanged.
- Fantome stores it as `META/game_data/<layer>/<path>`; import places it at
  `content/<layer>/<path>`. `classify_entry` and the project layout map the new entry.

Breaking: `FantomeEntry` gains a variant.

Blocked by #236.

- [ ] A layer whose manifest names `patch.ptch` classifies the file as an input; a WAD-directory override file is excluded from WAD chunks
- [ ] A missing override file, an ignored one, and a `PROP` file named as an override each refuse the layer's declarations with a message naming the path
- [ ] A rejected manifest naming an override classifies the file as an input
- [ ] A modpkg round trip stores the file as a WAD-less chunk of its layer, and the imported project loads identical declarations and identical override bytes
- [ ] A Fantome round trip stores `META/game_data/base/patch.ptch`, and the imported project loads identical declarations and identical override bytes
- [ ] `classify_entry("META/game_data/base/a.ptch")` is `GameData("base/a.ptch")`; a directory entry is `None`
- [ ] Lints, format, and docs clean
