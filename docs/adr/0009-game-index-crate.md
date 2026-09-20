# ADR-0009: Game index crate

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_index`, `ltk_overlay`, `ltk_game_data`
- **Related:** `docs/design/game-index.md` [section 3](../design/game-index.md#s3),
  `docs/design/game-data.md` [section 6](../design/game-data.md#s6)

## Context and problem statement

Three indexes of one installation exist across two repositories. `ltk_overlay::GameIndex` maps a
chunk hash to every archive holding it, for WAD distribution. LTK Manager's `game_index` folds
every archive into one directory tree for browsing and search, keeping one holder per chunk. LTK
Manager's `object_index` maps every bin object to the chunks declaring it, read with `ltk_meta`.
Each has its own archive walk, its own archive identity (game-relative path against
`DATA/FINAL`-relative name), and its own cache policy.

The `entries` selector of game data declarations resolves an object name to its declaring chunks
inside an overlay build. That lookup is the object index, and the object index is built from a
chunk table with every holder. Neither exists in this repository. LTK Manager is the main
consumer of `ltk_overlay` and needs the same two tables for its browser.

## Decision drivers

- One archive walk and one cache per installation, shared by every consumer.
- `ltk_overlay` stays free of `ltk_meta` and of a full-installation bin read unless a build needs
  one.
- Application concerns stay in the application: browsing, search, lifecycle, IPC types, display
  names.
- Both consumers move onto the same archive identity.

## Considered options

1. **One published crate `ltk_game_index`**, chunk index in the core, object index behind an
   `objects` feature, names through a resolver trait.
2. **Two crates**, `ltk_game_index` and `ltk_object_index`, the second depending on the first.
3. **Grow `ltk_overlay`**: add the object index to the overlay's existing index and have LTK
   Manager depend on `ltk_overlay` for both.

## Decision

**One published crate, `ltk_game_index`, holds the chunk index and, behind the `objects` feature,
the object index.** `game-index.md` [section 3](../design/game-index.md#s3) specifies the
boundary. `ltk_overlay::GameIndex` is removed and `ltk_overlay` depends on the crate. Name
resolution enters through a trait the crate defines. Trees, search, cancellation generations,
lifecycle slots, and wire types remain in LTK Manager.

## Consequences

- **Positive:** One build, one fingerprint, and one cache format serve the overlay, the
  declaration engine, and the manager. The manager's private hash-to-archive tables collapse into
  one.
- **Negative:** `ltk_overlay` 0.9 loses a public type, and every consumer constructing or
  reading `GameIndex` fields changes. A crate with a feature-gated half has one release cadence
  for both halves, and a fix to the object build bumps the chunk index's version. Archive ids are
  meaningless across builds, and a consumer holding one across a rebuild reads the wrong archive.
- **Revisit when:** A third index kind with its own heavy dependency wants to join, or the object
  index gains consumers that never need the chunk table.

## Pros and cons of the options

### One crate with a feature

- Good: One dependency line for the common case, one archive identity, one cache directory.
- Good: The object index reads the chunk index's private rows without a public seam between
  crates.
- Bad: Feature-gated API surface is easy to break unnoticed without a CI job per feature set.

### Two crates

- Good: Independent versions, and the object crate's `ltk_meta` dependency is invisible to
  chunk-only consumers without a feature.
- Bad: The object build needs the chunk index's rows, which forces a public iteration seam sized
  for one caller.
- Bad: Two release cycles and two changelogs for what every consumer uses as a pair.

### Grow `ltk_overlay`

- Good: No new crate and no migration of the overlay's own call sites.
- Bad: The manager's browser would depend on a WAD-patching crate for a lookup table, and
  `ltk_overlay` would carry `ltk_meta` for every consumer.
- Bad: The overlay's index keeps its path-per-holder rows and its cache size.
