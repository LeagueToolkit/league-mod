---
title: "Game index: resolving declaration selectors"
labels: area:api
---

Part of the game index map.
Type: grilling
Status: resolved
Blocked by: 03, 05

## Question

How does the game data engine consume the index? Decide where a `target` (path or 16-hex hash)
becomes its holder chunks and where an `entries` name becomes its declaring chunks: inside
`ltk_game_data`, inside the `ltk_overlay` build, or in a resolution step the crate itself offers.
Decide the diagnostics for a target with no holder, an entry no chunk declares, and an entry
declared in several chunks (fan-out to every holder per the wiki reference). Decide which sections
of `docs/design/game-data.md` change and what the wiki page `reference/mod-packages/game-data.mdx`
must say about index resolution.

## Answer

- Vocabulary: a **holder** is an archive containing a chunk (`GameIndex::holders`). A chunk
  containing an entry is a **declaring chunk** (`ObjectIndex::declarations`). The wiki page's
  Holder entry and its "Several holders" rule are reworded to declaring chunks.
- Resolution lives in `ltk_overlay`. An `entries` module lowers to one application per declaring
  chunk before base selection; the per-hash application pipeline is unchanged. `ltk_game_data`
  never depends on `ltk_game_index`.
- `ltk_game_data::Module` becomes `{ selector: Selector, location }` with
  `enum Selector { Target { target: Target, steps: Vec<Step> }, Entries(IndexMap<EntryName, Vec<Step>>) }`,
  serde-flattened on the `target` plus `steps` and `entries` keys. Steps live inside the selector
  so an `entries` module carries no empty top-level `steps`. Spec sections 2 to 6 and 8 of
  `docs/design/game-data.md` change.
- Several declaring chunks: every one is edited and an informational diagnostic names them. The
  single-container exception stays an unimplemented wiki rule pending a class allowlist.
- New `GameDataDiagnosticKind` variants: `EntryUnresolved`, `EntryFanOut`, `IndexUnavailable`.
- The overlay builds or loads the object index lazily, only when an enabled layer declares an
  `entries` module, through `load_or_build` beside `game_index.bin`. Progress goes through the
  overlay's build-stage reporting with an index stage. An index failure yields
  `IndexUnavailable` for every `entries` module and a build warning, never a failed build.
