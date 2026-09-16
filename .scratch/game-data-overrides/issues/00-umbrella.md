---
issue: 191
title: "`ltk_mod_project`: Declarative PTCH targeting"
labels: enhancement, epic
---

A layer declares `overrides`: `.ptch` files in the layer whose `PTCH` records the overlay
build applies over a declared target, record by record, with the client's matching rules.
The binding is the other half of roadmap item 1 of the
[game-data reference](https://wiki.leaguetoolkit.dev/reference/mod-packages/game-data/),
beside `links` (#190). An override file travels with the layer through both archives and
comes back on extraction.

| Document | Where |
| --- | --- |
| Spec | [`docs/design/game-data.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md) sections 3 to 6 and 8 |
| Decisions | [ADR-0012](https://github.com/LeagueToolkit/league-mod/blob/main/docs/adr/0012-eager-tree-override-application.md), [ADR-0013](https://github.com/LeagueToolkit/league-mod/blob/main/docs/adr/0013-override-file-placement.md) |
| Record language | [`ptch-property-patches.md`](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md) in `league-toolkit` |

## Children

- [ ] #236 `ltk_game_data`: `OverridePath`, `Edit::overrides`, the override phase of `apply`. Goes first; #237 and #239 rest on it.
- [ ] #237 Project and archives: override files as build resources, packed into modpkg and Fantome, restored on import. Rests on #236.
- [ ] #239 Overlay: provider read of override files and the override diagnostic kinds. Rests on #236; independent of #237 for a directory mod, and its archive cases rest on #237.
- [ ] #240 `.rito` override files. Parked: `ltk_ritobin` lowers `#PROP_text` only.
- [ ] #238 Wiki reference page: status of `overrides`. Rests on #239.
