---
title: "Game index: write spec, ADR and implementation tickets"
labels: docs
---

Part of the game index map.
Type: task
Status: resolved
Blocked by: 03, 04, 05, 06

## Question

Write `docs/design/game-index.md` (vocabulary, crate boundary, chunk index, object index, resolver,
cache, errors, tests) with the `write-spec` skill, one ADR for adding the crate with the
`write-adr` skill (the removal of `ltk_overlay::GameIndex` is a line in it), edit
`docs/design/game-data.md` per the selector resolution decision, and slice the implementation
tickets under `.scratch/game-index/issues/` with `write-ticket`. Run `sync-issues`. The map is
done when this closes.

## Answer

Written, uncommitted, in the working tree:

- `docs/design/game-index.md`: vocabulary, crate boundary, archives, chunk index, build,
  fingerprint and cache, resolver, object index, consumers, validation, rules G1 to G14.
- `docs/adr/0009-game-index-crate.md`: one crate with an `objects` feature, over two crates and
  over growing `ltk_overlay`; the removal of `ltk_overlay::GameIndex` is a line in it.
- `docs/design/game-data.md`: sections 2, 3, 4, 5, 6 and 8 rewritten for `Selector`,
  `EntryName`, declaring chunks, the three diagnostic kinds, the lazy object index, rules D7 to D10.
- Implementation tickets `00-umbrella.md` and `10` to `16` in this directory, each with the
  surface lifted from the spec and a test-shaped checklist. Dependency order: 10, 11, then 12 and
  13 in parallel, 14 independent, 15 after 12, 13 and 14, 16 after 15.
- `sync-issues` not run. The GitHub label scheme the skill names (`crate:*`, `area:*`) does not
  exist on the repository; the tickets use the labels that do (`enhancement`, `epic`,
  `ltk_overlay`, `documentation`).
