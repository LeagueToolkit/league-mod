# ADR-0022: Load-time override compilation

- **Status:** Accepted
- **Date:** 2026-09-19
- **Crates:** `ltk_game_data`, `ltk_mod_project`, `ltk_overlay`
- **Related:** #191, #240, [league-toolkit#238](https://github.com/LeagueToolkit/league-toolkit/issues/238),
  `docs/design/game-data.md` [section 4](../design/game-data.md#s4),
  [section 5](../design/game-data.md#s5) and [section 6](../design/game-data.md#s6), ADR-0013

## Context and problem statement

The [game-data reference](https://wiki.leaguetoolkit.dev/reference/mod-packages/game-data/)
accepts a `.rito` override file: ritobin text of type `PTCH`, the text form moonshadow's
ritobin and LtMAO write. The client reads a binary `PTCH` only. `ltk_ritobin` parses `PTCH` text
to an `ltk_meta::BinOverride`, with a diagnostic at a byte span for every malformed record.

An override file travels under its layer-relative path in every container (ADR-0013). The
overlay reads it through the layer's content provider, and `apply` reads the bytes as a binary
`PTCH`. `apply` reports a file it cannot read as `OverrideUnreadable` and one it cannot parse as
`OverrideInvalid`, each with no place inside the file. A layer's declarations load in the project crate, which reads every override file and
refuses the layer for a file that is not a `PTCH`.

LTK Manager is the one packer and overlay builder. It builds from archives and from a project
directory read in place.

## Decision drivers

- One override encoding past the project boundary: archives, `apply` and every archive consumer
  read a binary `PTCH`.
- An authoring error in the text surfaces before a build, at a span in the text.
- No text parser on the application path.
- An extracted project repacks to the same package.

## Considered options

1. **Compile at load, pack the binary.** The project crate compiles a `.rito` file when it loads
   the layer; packing stores the compiled bytes under the path with its extension replaced by
   `.ptch`, and the packed document names that path.
2. **Compile at apply, pack the text.** Archives carry the `.rito` file as authored, and `apply`
   parses a `.rito` path through `ltk_ritobin`.
3. **Pack both.** Archives carry the text beside the compiled bytes, and the document names both.

## Decision

**A `.rito` override file compiles to `PTCH` bytes when `ltk_mod_project` loads the layer.
Packing stores the bytes under the authored path with its extension replaced by `.ptch`, and the
packed declaration document names that path. A directory mod's provider compiles the file on
read.**

`docs/design/game-data.md` [section 4](../design/game-data.md#s4) states the loading rule and the
errors, [section 5](../design/game-data.md#s5) the packed spelling, and
[section 6](../design/game-data.md#s6) the provider. `ltk_mod_project` depends on `ltk_ritobin`.

## Consequences

- **Positive:** an archive the project crate packs holds no text, and `apply` parses none;
  `ltk_game_data` gains no dependency.
- **Positive:** a malformed `.rito` refuses the layer at load with a `Syntax` code and the span of
  the first ritobin error, where the manager's editor can point at it.
- **Negative:** `ltk_mod_project` depends on `ltk_ritobin` and its parser dependencies, for every
  consumer of the project crate.
- **Negative:** the text does not survive packing. An extracted project carries `patch.ptch` for an
  authored `patch.rito`, without the text's comments and the names it spells for hashes;
  repacking it yields the same package, not the same project.
- **Negative:** the packed document spells a path the author did not write, and two files whose
  paths differ only in `.rito` and `.ptch` collide.
- **Negative:** a directory mod compiles a `.rito` file on every load of its layer and again at
  read. The filesystem provider loads a layer's declarations several times per build.
- **Revisit when:** an extracted project has to restore the authored `.rito` text.

## Pros and cons of the options

### Compile at load, pack the binary

- Good: one encoding in every archive; the overlay's archive providers and `apply` need no change.
- Good: loading reads and validates every override file, and compiling is one step of that read.
- Bad: the authored text is lost to extraction.

### Compile at apply, pack the text

- Good: the archive carries what the author wrote, and extraction restores it.
- Bad: `ltk_game_data` and every build depend on `ltk_ritobin` and parse text at build time.
- Bad: `apply` reports an unparsable file as `OverrideInvalid`, without a place in the text; a
  report at a span needs a new diagnostic shape in `ltk_game_data` and in the overlay's persisted
  state.
- Bad: every archive consumer that reads an override file handles two encodings.

### Pack both

- Good: extraction restores the text, and builds read the binary.
- Bad: two files per override in every container, and a document shape naming both; ADR-0013's one
  reference per file becomes two.
- Bad: nothing keeps the two copies in agreement in an archive edited by hand.
