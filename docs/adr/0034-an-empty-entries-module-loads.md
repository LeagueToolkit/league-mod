# ADR-0034: An empty entries module loads

- **Status:** Accepted
- **Date:** 2026-09-25
- **Crates:** `ltk_game_data`, `ltk_overlay`
- **Related:** ADR-0032, `docs/design/game-data.md` [section 4](../design/game-data.md#s4)

## Context and problem statement

An `entries` module with no entry was `EntriesEmpty`, and the layer's declarations did not
load. A module therefore existed only from its first entry.

LTK Manager lets an author organize declarations into named modules (ADR-0032) and move entries
between them. An author makes the module first and fills it after, by moving entries into it or
by editing a game bin with the module chosen. With the refusal, the manager could not write a
module the moment it was named, and a move that took a module's last entry had to remove the
module, its name with it.

## Decision drivers

- An author names and orders modules before filling them.
- A manifest the manager writes loads at every step of that work.
- An empty module changes nothing a build applies.

## Considered options

1. **An empty `entries` mapping loads as a module that applies nothing.**
2. **Keep the refusal.** The manager holds a named module outside the manifest until an edit
   fills it.
3. **A placeholder binding.** The manager writes an entry the build skips.

## Decision

**An `entries` module may hold no entry. It loads, writes and reads back, and applies
nothing.** A `target` module keeps `EditsEmpty`, because a chunk path with no edit declares no
intent. `ErrorKind::EntriesEmpty` stays as a deprecated variant, so a consumer matching on it
still builds. An overlay build with only empty `entries` modules does not load the object index.

## Consequences

- **Positive:** a module exists in the manifest as soon as an author names it, and survives
  losing its last entry.
- **Positive:** the rule is the reading of the empty mapping, with no new key.
- **Negative:** a reader older than this change refuses a layer holding an empty module. A mod
  packed with one loads no declarations of that layer in an older LTK Manager.
- **Neutral:** `EntriesEmpty` is no longer raised.
- **Revisit when:** a `target` module needs to exist before its first edit.

## Pros and cons of the options

### An empty mapping loads

- Good: the manifest on disk is what the author sees.
- Bad: an older reader refuses it.

### Keep the refusal

- Good: no format change.
- Bad: the module lives in editor state, and is lost when nothing fills it.

### A placeholder binding

- Good: every reader loads it.
- Bad: the manifest holds a line that means nothing, and every consumer has to skip it.
