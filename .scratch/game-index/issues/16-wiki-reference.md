---
issue: 235
title: "Wiki: declaring chunks in the game data reference"
labels: documentation
---

Part of #228. The LTK wiki page `reference/mod-packages/game-data.mdx` uses
"holder" for a chunk containing an entry. The spec reserves holder for an archive containing a
chunk (`docs/design/game-index.md` [section 2](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md#s2)).

- Reword the Holder vocabulary entry and the "Several holders" rule to declaring chunk, and
  state that every declaring chunk is edited with an informational report.
- Name the three diagnostic kinds `EntryUnresolved`, `EntryFanOut`, `IndexUnavailable` in the
  reports table and the troubleshooting rows.
- Move `entries` from the roadmap to the status caution's implemented list once the overlay
  resolution ticket lands.
- Keep the single-container exception as an unimplemented rule with a marker.

Blocked by #234.

- [ ] No occurrence of "holder" on the page refers to a chunk
- [ ] The three diagnostic kinds appear in the reports table
- [ ] `npm run build` in the wiki checkout passes
