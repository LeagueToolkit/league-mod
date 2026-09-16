---
issue: 228
title: "Game index: shared chunk and object index crate"
labels: enhancement, epic
---

A published `ltk_game_index` crate indexes one installation's chunks and bin objects for
`ltk_overlay`, the game data declaration engine, and LTK Manager. The overlay's private index is
replaced, and the `entries` selector of game data declarations resolves through the object index.

| Document | Where |
| --- | --- |
| Spec | [`docs/design/game-index.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md) |
| Declarations spec | [`docs/design/game-data.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md) sections 2 to 6 and 8 |
| Decision | [ADR-0009](https://github.com/LeagueToolkit/league-mod/blob/main/docs/adr/0009-game-index-crate.md) |
| Research | [`docs/research/game-data-engine-constraints.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/research/game-data-engine-constraints.md) |

## Children

- [ ] #229 Chunk index crate: archives, rows, lookups. Goes first; #230 rests on it.
- [ ] #230 Fingerprint and cache. Rests on #229.
- [ ] #231 Object index behind the `objects` feature, resolver trait, `hashtable` feature. Rests on #230.
- [ ] #232 Overlay adoption of `ltk_game_index`. Rests on #230; independent of #231.
- [ ] #233 `Selector` and `EntryName` in `ltk_game_data`. Depends on nothing here and can land in any order.
- [ ] #234 Overlay `entries` resolution, diagnostics, lazy object index. Rests on #231, #232, and #233.
- [ ] #235 Wiki reference page: declaring chunks and diagnostics. Rests on #234.
