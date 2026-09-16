---
issue: 233
title: "Game data: entries selector"
labels: enhancement
---

Part of #228 (design: [`docs/design/game-data.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md)
[section 4](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md#s4) and [section 5](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md#s5)). Replaces
`Module::target` and `Module::steps` with a `Selector`, and adds `EntryName`. Breaking change
to `ltk_game_data`'s public types and the direct JSON manifest.

## Proposed surface

```rust
pub struct Module { pub selector: Selector, pub location: DeclarationLocation }
pub enum Selector {
    Target { target: Target, steps: Vec<Step> },
    Entries(IndexMap<EntryName, Vec<Step>>),
}
pub struct EntryName(String);
impl EntryName {
    pub fn as_str(&self) -> &str;
    pub fn object_hash(&self) -> BinHash;
}
impl TryFrom<String> for EntryName {}
impl TryFrom<&str> for EntryName {}
```

- Serialized: `target` with `steps`, or `entries`; a module with both or neither keys is an
  error. `manifest_json` writes the same shape.
- Authoring: an `entries` value is a mapping of entry name to compact bindings or `steps`;
  `source` is not permitted inside `entries`. Sources keep their existing shape.
- `EntryName` rejects the empty string; `object_hash` is FNV-1a over the ASCII-lowercased
  spelling through `ltk_hash`.
- `apply` is unchanged; it takes steps.

Blocked by nothing.

- [ ] YAML, TOML, and JSON manifests with an `entries` module load into `Selector::Entries` in mapping order
- [ ] A module with both `target` and `entries`, or neither, is a load error naming the module
- [ ] A `source` key inside an entry body is a load error
- [ ] `DeclarationDocument` round trip preserves both selector shapes; `manifest_json` reproduces the input
- [ ] `EntryName::object_hash` matches a known bin entry hash for mixed-case input; the empty string is rejected at construction and deserialization
- [ ] Existing `target` fixtures load unchanged
- [ ] Lints, format, and docs clean
