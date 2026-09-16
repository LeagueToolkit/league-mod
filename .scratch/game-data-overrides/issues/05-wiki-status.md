---
issue: 238
title: "Wiki: overrides status on the game-data reference"
labels: documentation
---

Part of #191. The [game-data reference](https://wiki.leaguetoolkit.dev/reference/mod-packages/game-data/)
carries a status caution naming what is implemented. `overrides` with `.ptch` files joins the
implemented list, `.rito` stays unimplemented (#240), and the build-semantics report table
gains the three override kinds.

- The status caution names the overlay's `overrides` support and the `.ptch`-only limit.
- The `overrides` binding section states that a `.rito` file is refused with an error naming
  the extension.
- The report kinds table lists `OverrideUnreadable`, `OverrideInvalid`, and
  `OverrideRecordSkipped` with their meanings from `docs/design/game-data.md` section 6.

Blocked by #239.

- [ ] The status caution lists `overrides` as implemented for `.ptch` files
- [ ] The report table carries the three override kinds
- [ ] The page builds without warnings
