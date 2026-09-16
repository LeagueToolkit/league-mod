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
pub struct Module { pub selector: Selector, pub origin: Origin }
#[non_exhaustive]
pub enum Selector {
    Target { target: Target, edits: Vec<Edit> },
    Entries(IndexMap<EntryName, EntryEdit>),
}
/// One batch of bindings on a chunk; fields are phases in apply order.
#[non_exhaustive] #[derive(Default)]
pub struct Edit { pub links: LinkEdit }
/// The bindings of one entry, applied in every chunk declaring it.
#[non_exhaustive] #[derive(Default)]
pub struct EntryEdit { pub links: LinkEdit }
pub struct LinkEdit { pub add: Vec<LinkPath>, pub remove: Vec<LinkPath> }
pub struct Origin { pub manifest: String, pub source: Option<String>, pub module_index: usize }
pub struct EntryName(/* path or `0x` + 8 hex digits */);
impl EntryName {
    pub fn as_str(&self) -> &str;
    pub fn object_hash(&self) -> BinHash;
}
impl TryFrom<String> for EntryName {}
impl TryFrom<&str> for EntryName {}
pub fn apply(base: &[u8], edits: &[Edit]) -> Result<ApplyResult, Error>;
```

- Serialized: `target` with `steps` (one compact body per edit), or `entries` (one compact
  body per entry); a module with both or neither keys is an error. `manifest_json` writes the
  same shape and always writes `links`.
- Authoring: an `entries` value is a mapping of entry name to a compact body; `steps` and
  `source` are not permitted inside `entries`. Sources keep their existing shape. A compact
  body and every step carry at least one binding.
- `EntryName` rejects the empty string; `0x` and 8 hex digits spell an object hash, every
  other spelling hashes as FNV-1a over the ASCII-lowercased path through `ltk_hash`.
- `ApplyDiagnostic::edit_index` replaces `step_index`; the wire key stays `step`.
- Decisions: ADR-0010 (phased edits, `links` inside an entry body), ADR-0011 (`preserve_order`).

Blocked by nothing.

- [ ] YAML, TOML, and JSON manifests with an `entries` module load into `Selector::Entries` in mapping order, through the document round trip and `manifest_json`
- [ ] A module with both `target` and `entries`, or neither, is a load error naming the module
- [ ] A `source` or `steps` key inside an entry body, and a step with no binding, are load errors
- [ ] `DeclarationDocument` round trip preserves both selector shapes; `manifest_json` reproduces the input
- [ ] `EntryName::object_hash` matches a known bin entry hash for mixed-case input and for the `0x` spelling; the empty string is rejected at construction and deserialization
- [ ] Existing `target` fixtures load unchanged
- [ ] Lints, format, and docs clean
