# ADR-0035: Patch targets as records

- **Status:** Accepted
- **Date:** 2026-09-29
- **Crates:** `ltk_game_data`, `ltk_overlay`
- **Related:** ADR-0012, ADR-0017, `docs/design/game-data.md` [section 6](../design/game-data.md#s6)

## Context and problem statement

A controller of the game's UI names variants: `PTCH` chunks such as `uirtl` and
`uiflippedminimap`, each a set of records over a base scene `PROP`. The client lays a
switched-on variant over its base on load, and its parser accepts a variant only as `PTCH`
version 1. A shipped variant patches `Position.UIRect`, `Scene`, `Position.Anchors`, `Layer`
and `TextureData` of objects the base declares, and some variants add objects.

LTK Manager's Atlas editor edits a UI view with a variant drawn over its base. An edit made
there belongs to the variant: the same field of the base is the value every other variant
starts from, and the variant's own record for the field wins over the base in game.

`apply` mounts its target as a `PROP` and refuses anything else as `UnsupportedBase`. A
declaration over a variant chunk is therefore refused in the editor and `TargetSkipped` in the
overlay. The one route a layer has is to ship its own copy of the whole variant, which replaces
every record the game ships in it and takes none of the game's later changes.

## Decision drivers

- A variant edit is stored as a declaration, as every other game data edit is.
- The overlay writes a chunk the client loads: a variant stays a `PTCH`.
- A mod names only the keys it edits, over whatever the game's variant holds at build time.
- The `PROP` pipeline's typing, signs, coercion and diagnostics carry over unchanged.

## Considered options

1. **Records over the variant.** A `PTCH` target lowers each settled key to one record, and
   object edits to the patch's deletions and objects.
2. **Merge and diff.** Lay the variant over the base scene, apply the edits, and diff the result
   against the base scene into a new `PTCH`.
3. **Declarations on the base only.** A variant stays read-only, and an edit made over a variant
   is declared on the base scene.

## Decision

**A `PTCH` target takes every edit as the patch's own deletions, objects and records.**

The edits run over a view: the patch's objects, and `read_entry`'s copy of each other object an
edit names with the patch's records laid over it. A settled key of an object the patch holds
edits the object. A settled key of any other object is a record `{object, path, value}`, which
replaces every held record of the object at that path or under it. The output is a `PTCH` with
no dependencies. A link edit is `LinkUnsupported`, a hash-form path on a recorded object is
`HashFormPath`, and an entry `read_entry` fails on is `EntryUnreadable`. The overlay loads the
object index at the first patch target of a build.

## Consequences

- **Positive:** a variant edit composes with other mods and with the game's later changes to the
  variant, record by record.
- **Positive:** the `PROP` pipeline runs unchanged over the view, and one key is one record, as
  one key is one patch in a `PROP` (ADR-0017).
- **Negative:** a `+` or `-` on a list or map settles the whole container as the view held it,
  and that record fixes the container over later changes to the base.
- **Negative:** a record path is text the client hashes, so a key spelled by field hash does
  not lower to a record.
- **Negative:** the view reads base objects from the game before any mod content applies. A
  container edit does not see another mod's declarations on the base scene.
- **Revisit when:** a consumer needs a record inside a map entry, which a record path cannot
  insert.

## Pros and cons of the options

### Records over the variant

- Good: the output names only what the author edited.
- Good: typing and signs read the value the variant shows, which is what the author saw.
- Bad: the target needs the game's copy of each base object an edit names.

### Merge and diff

- Good: no view logic in the apply phases.
- Bad: needs the base scene chunk and field names to spell record paths.
- Bad: rewrites the game's records, and falls back to whole objects where a diff cannot lift a
  change into a record.

### Declarations on the base only

- Good: no change to `apply`.
- Bad: a field the variant patches keeps the variant's value in game, so the edit has no effect
  in that variant.
