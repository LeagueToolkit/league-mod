# ADR-0004: Game data engine

- **Status:** Accepted
- **Date:** 2026-09-15
- **Crates:** `ltk_game_data`, `ltk_mod_project`, `ltk_modpkg`, `ltk_fantome`, `ltk_overlay`
- **Related:** #190, #191; `game-data.md` [section 3](../design/game-data.md#s3)

## Context and problem statement

Project directories and archives carry the same declarations. Consumers need declaration
validation without an overlay builder. The game-data standard includes schema-dependent edits;
`ltk_meta` owns binary formats and has no game schema dependency.

## Decision drivers

- One declaration model across containers.
- Consumer access independent of overlay construction.
- Byte-preserving writes for untouched objects.

## Considered options

1. **Dedicated game-data crate** - shared declaration types, loading, and materialisation.
2. **Project-owned engine** - declaration loading and materialisation in `ltk_mod_project`.
3. **Overlay-owned engine** - executable declarations and materialisation in `ltk_overlay`.

## Decision

**A dedicated `ltk_game_data` crate owns declarations and materialisation.**
The surface is specified in `game-data.md` [section 3](../design/game-data.md#s3).
The crate uses `serde-saphyr` for YAML 1.2, `toml` 1.1 for TOML 1.1,
`serde_json` for JSON, and `ltk_meta` 0.8.2 for binary validation.
Materialisation is specified in `game-data.md` [section 6](../design/game-data.md#s6).

## Consequences

- **Positive:** Archives and consumers share validation and executable types.
- **Negative:** One additional published crate and YAML parser increase dependency maintenance.
- **Revisit when:** An upstream game-data engine provides the same container-neutral contract.

## Pros and cons of the options

### Dedicated game-data crate

- Good: Consumers use declaration operations without filesystem or overlay dependencies.
- Bad: Public types cross several crate release boundaries.

### Project-owned engine

- Good: The project loader already owns authoring paths and ignore rules.
- Bad: Archive crates require a dependency on their own project conversion layer.

### Overlay-owned engine

- Good: Materialisation has direct access to game chunks.
- Bad: Editors and archive readers acquire overlay dependencies for declaration validation.

### Parser alternatives

- `serde-saphyr` supports YAML 1.2 core scalars and duplicate-key rejection. Its dependency
  graph is larger than a JSON-only loader.
- JSON-only declarations require fewer dependencies and exclude the standard's preferred
  authoring format.
- TOML 0.8 is shared with project configuration and excludes TOML 1.1 inline-table syntax.
- Published `ltk_meta` 0.8.2 provides bin validation and has no delta writer. A header
  splice preserves object bytes and carries a small amount of dependency-header encoding.
- A git dependency provides the upstream delta writer and prevents crates.io publication
  of this dependency graph. Eager bin reserialization uses published APIs and changes
  untouched object bytes.
