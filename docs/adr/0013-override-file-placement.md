# ADR-0013: Override file placement

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_data`, `ltk_mod_project`, `ltk_modpkg`, `ltk_fantome`, `ltk_overlay`
- **Related:** #191, `docs/design/game-data.md` [section 4](../design/game-data.md#s4),
  [section 5](../design/game-data.md#s5) and [section 6](../design/game-data.md#s6), ADR-0006

## Context and problem statement

An override file is a build resource: a `.ptch` in the layer directory that a declaration
names and the build reads. It is not game content. A manifest and its sources are the same
kind of thing and travel only as expanded declarations; an override file has to travel as
bytes, in every container, and come back to the same place on extraction.

An authored path is relative to the source file that names it, or to the layer directory in
a direct body. The same file is named from several sources with different spellings. The
declaration document is one shape across the project, modpkg, Fantome, and the overlay.

Modpkg stores a layer's loose files as chunks of the layer with no WAD, extracted to the
layer's content directory at their path. Fantome packs WAD directories only and drops loose
files; its `META/` directory holds every non-content entry, matched case-insensitively.

## Decision drivers

- One spelling of an override reference from project to build; no per-container rewriting.
- An extracted project repacks to the same package.
- The overlay reads an override file through the layer's own content provider.
- Override files stay out of game-content enumeration in every container.

## Considered options

1. **A layer-relative path as the one reference.** Modpkg stores the file as a WAD-less chunk
   of its layer at that path; Fantome stores it at `META/game_data/<layer>/<path>`.
2. **A meta chunk in every container**, `_meta_/game_data/<layer>/<path>` in modpkg beside
   the hashtables, `META/game_data/<layer>/<path>` in Fantome.
3. **A per-container reference rewritten at pack**, the archive-root path the wiki proposal
   sketches for Fantome, restored to a project path on import.

## Decision

**A loaded declaration names an override file by its layer-relative path, and every
container stores the file under that path: modpkg as a chunk of the layer with no WAD,
Fantome as `META/game_data/<layer>/<path>`.**

`docs/design/game-data.md` [section 5](../design/game-data.md#s5) states the storage rule and
[section 6](../design/game-data.md#s6) the provider method. `load_declarations` resolves an
authored path against its containing file lexically and refuses one that leaves the layer.
Modpkg extraction of a layer places the file with the layer's other chunks; Fantome import
places it under `content/<layer>/`.

## Consequences

- **Positive:** `OverridePath` is one type from manifest to build; the overlay reads it with
  one provider method per container and no path translation.
- **Positive:** modpkg needs no format change; the chunk is the shape of a loose layer
  file, and `ltk_modpkg` extracts it without a new destination.
- **Negative:** in modpkg an override file and an ordinary loose file of the layer are the
  same kind of chunk; only the declaration says which is which. A consumer enumerating
  WAD-less chunks sees both.
- **Negative:** `FantomeEntry` gains a variant, a breaking change for a consumer that matches
  it exhaustively.
- **Revisit when:** a container gains a resource table that names build inputs by role.

## Pros and cons of the options

### A layer-relative path as the one reference

- Good: no rewriting, no per-container reference shape, extraction is placement.
- Good: modpkg needs no new chunk kind.
- Bad: modpkg cannot tell an override chunk from a loose file without the declaration.

### A meta chunk in every container

- Good: the file is visibly a resource in both containers; the hashtable precedent.
- Bad: `ltk_modpkg` needs a new `ChunkDestination` and a placement rule with the layer
  encoded in the path; a second layer spelling to keep consistent with the layer table.
- Bad: a project extracted and repacked routes the file through two placements.

### A per-container reference rewritten at pack

- Good: an archive's references name what the archive holds.
- Bad: the declaration document differs by container; the overlay and the importer each
  translate; the manager cites two shapes.
