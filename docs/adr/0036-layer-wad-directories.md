# ADR-0036: Layer WAD directories

- **Status:** Proposed
- **Date:** 2026-10-01
- **Crates:** `ltk_fantome`, `ltk_mod_project`, `ltk_overlay`
- **Related:** ADR-0013, `crates/ltk_fantome/DESIGN.md` "Layer WAD directories"

## Context and problem statement

A mod project has layers, and a `.modpkg` stores every layer's content. A `.fantome` stores the
layer table in `META/info.json`, with each layer's name, priority, string overrides and game data,
and its game-data override files under `META/game_data/<layer>/`. Its WAD content has one place,
`WAD/`, and that place is the base layer's. A pack to Fantome drops the WADs of every other layer.

The official Fantome specification defines `META/`, `WAD/` and `RAW/` and says nothing about
layers. Two consumers read archives that predate any layer layout:

- cslol-manager unzips an archive, mounts every child of `WAD/` as a WAD, packs `RAW/` into one
  WAD, and copies `META/` into the installed mod whole (`wad::Index::from_mod_folder` and
  `mod_copy` in `cslol-tools`). It ignores every other top-level directory.
- `ltk_fantome` releases before this one place an entry by the prefix `WAD/` and ignore every
  other top-level directory.

A layer's WADs need a place both of these skip, so an archive with layers installs in them as its
base layer and nothing else.

## Decision drivers

- A reader that predates layers loads the base layer unchanged and never misreads a layer's WAD
  as base content.
- A reader that predates layers copies no layer content into an install.
- One entry name per (layer, WAD), derivable from the two names alone.
- A layer name in an entry name never escapes an extraction directory.

## Considered options

1. **A top-level `WAD_<layer>/` directory per layer other than the base.**
2. **A nested `LAYERS/<layer>/WAD/` directory per layer other than the base.**
3. **A layer subdirectory of `WAD/`**, `WAD/<layer>/<wad>`.
4. **A layer directory under `META/`**, `META/layers/<layer>/<wad>`.

## Decision

**A layer other than the base stores its WADs under the top-level directory `WAD_<layer>/`, in
the same two shapes `WAD/` takes. `<layer>` is one or more ASCII letters, digits, `-` or `_`, and
is not `base` in any casing.**

`crates/ltk_fantome/DESIGN.md` "Layer WAD directories" states the rule, the matching of a
directory to a declared layer, and the priority-0 layer an undeclared directory loads as. Every
API that names a packed WAD names its layer beside it, and `classify_entry` reports the layer of
every WAD entry.

## Consequences

- **Positive:** cslol-manager and earlier `ltk_fantome` releases install the base layer of a
  layered archive exactly as before. The archive installs and works in both, minus its other
  layers.
- **Positive:** a layer WAD is a packed WAD to normalization, entry replacement and delta repair.
  Each orders it last and stores it, with no rule of its own.
- **Negative:** a consumer on an earlier `ltk_overlay` lists the layers `META/info.json` declares
  and offers toggles for them, and those layers carry no content there.
- **Negative:** `FantomeEntry`, the reader and writer WAD methods, `WadProgress`, `ArchiveDelta`
  and `FantomeDeltaError::WadNotPacked` change shape. A consumer that matches or calls them
  changes with them.
- **Negative:** a hand-made archive with a `WAD_<layer>/` directory the metadata does not declare
  gains a layer at priority 0. It ties the base layer, and the two apply in name order.
- **Revisit when:** the official Fantome specification or cslol-manager defines a layer layout of
  its own.

## Pros and cons of the options

### A top-level `WAD_<layer>/` directory

- Good: every reader that predates layers skips it, and the base layer stays where they look.
- Good: the name reads as a sibling of `WAD/`, one directory deep like `WAD/`.
- Bad: the layer name is part of a directory name, so its grammar is narrower than free text.

### A nested `LAYERS/<layer>/WAD/` directory

- Good: every reader that predates layers skips it, and the layer name is a whole path component.
- Bad: two levels deeper than `WAD/` for the same content, against the Windows path length a
  consumer preflights.
- Bad: a second top-level name and a nested `WAD/` that reads like the base layer's directory.

### A layer subdirectory of `WAD/`

- Good: every WAD stays under one prefix.
- Bad: cslol-manager mounts each child of `WAD/` as a WAD and logs a failure for a layer
  directory, and an earlier `ltk_fantome` places its files as base-layer WAD files.

### A layer directory under `META/`

- Good: every reader that predates layers leaves it out of the build.
- Bad: cslol-manager copies `META/` whole, so every install carries every layer's WADs unused.
- Bad: `META/` holds metadata and build resources. Game content there breaks that boundary.
