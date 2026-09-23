# ADR-0031: Game-shadowing report

- **Status:** Accepted
- **Date:** 2026-09-23
- **Crates:** `ltk_overlay`
- **Related:** ADR-0009, ADR-0029, `docs/design/game-data.md` [section 6](../design/game-data.md#s6)

## Context and problem statement

The game-data reference states that a name a mod adds under `objects` carries the
`Mods/<modid>/` prefix, and that the build checks it against the game index before packing: a
name the index holds is a packing error unless the declaration says the override is intended.
The reference names no key for that intent.

A game bin declaring an object loads it under its path hash. A second object under the same
hash in another chunk is one of two definitions, and which one the game reads depends on load
order. A name the target itself holds is `ObjectExists` at application.

Packing runs in `ltk_mod_project` and in LTK Manager's packer, with no game installation. The
overlay build holds the object index (ADR-0009) and loads it for an `entries` module or a
reference. `ltk_game_data` does not know a mod's id.

## Decision drivers

- The check runs where the game's entries are known.
- A mod that shadows an entry on purpose builds.
- No key the reference does not define.

## Considered options

1. **A build-time report.** A created name the object index declares in another chunk is
   `ObjectShadowsGame`; the object is created.
2. **A build-time skip with an opt-in.** The same check skips the creation unless the object
   body carries a new key.
3. **A packing error.** The packer reads a game index and refuses the name.

## Decision

**A created object whose name the object index declares in a chunk other than the target is
`ObjectShadowsGame`, an informational diagnostic, and the object is created.**
`docs/design/game-data.md` [section 6](../design/game-data.md#s6) states the rule. A build
with a created object loads the object index. The `Mods/<modid>/` prefix is the reference's
recommendation, and loading does not check it.

## Consequences

- **Positive:** the author sees every shadowed name on the build that produces it.
- **Positive:** no new key, and a deliberate shadow builds unchanged.
- **Negative:** a build creating an object pays the object index, a full read of the game's
  bins on a cold cache.
- **Negative:** a name that does not follow the prefix packs and builds.
- **Revisit when:** the reference defines an opt-in key, or packing gains a game installation.

## Pros and cons of the options

### A build-time report

- Good: the index is at hand; nothing is refused.
- Bad: a report is easy to miss.

### A build-time skip with an opt-in

- Good: an accidental shadow never reaches the game.
- Bad: a key the reference does not define, and a mod that built before a game patch added the
  name stops creating it.

### A packing error

- Good: the reference's wording.
- Bad: packing has no game index; the check would need an installation at pack time, and a
  game patch adding the name after packing escapes it.
