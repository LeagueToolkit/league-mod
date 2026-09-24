# ADR-0032: Module names

- **Status:** Accepted
- **Date:** 2026-09-24
- **Crates:** `ltk_game_data`
- **Related:** ADR-0007, ADR-0010, `docs/design/game-data.md`
  [section 4](../design/game-data.md#s4) and [section 5](../design/game-data.md#s5)

## Context and problem statement

A manifest's `modules` list holds modules in execution order. A module holds one selector and
its edits, and a manifest module refuses every key that is not a selector, `source`, `edits`,
or a binding. A diagnostic and a loading error name a module by its zero-based index.

A mod that recolors a champion spans several modules: a `target` module per skin bin and an
`entries` module for the shared particles. LTK Manager lists the modules of a layer to the mod
author. An index and a target path are the only labels it can show, and a list of target paths
does not tell the author which modules belong to one change.

The game-data reference defines no module key beside the selector. A reader of a
declaration document refuses a module key it does not know.

## Decision drivers

- An author labels a group of modules in the file the author writes.
- The label survives packing, both archives, and extraction.
- Execution order and application are unchanged.
- One more public identifier follows the rules of the others (ADR-0007).

## Considered options

1. **An optional `name` on a manifest module, carried on `Module`.** A nonempty string beside
   the selector, not unique, written by every serialized form.
2. **A unique module id.** The same key, with a duplicate refused at loading.
3. **A separate name table.** A top-level manifest mapping from module index to name.
4. **YAML comments.** No key; the manager reads comments above each module.

## Decision

**A manifest module takes an optional `name`, a nonempty `ModuleName` carried on `Module` and
written by the declaration document, `manifest_json()`, and both archives. Two modules may
hold the same name.** `docs/design/game-data.md` [section 4](../design/game-data.md#s4) and
[section 5](../design/game-data.md#s5) state the rule. An empty name is `EmptyModuleName`. A
`name` in a source file or an `edits` item is an unsupported binding. The declaration version
stays 1.

## Consequences

- **Positive:** an author labels one change across several modules, and every consumer reads
  the label from the declarations it already holds.
- **Positive:** a diagnostic's `Origin` reaches the name through the module index, with no
  change to the diagnostic.
- **Negative:** `Module` gains a public field. A consumer that builds a `Module` by hand adds
  `name`, a breaking change of the crate's API.
- **Negative:** a document holding a name is refused by a reader that predates the key. An
  archive packed with names rejects the layer's declarations in an older LTK Manager.
- **Negative:** a name is not an identifier. Two modules under one name are told apart by index
  only.
- **Revisit when:** a consumer needs to address a module by name across edits of the manifest,
  or the game-data reference defines its own module key.

## Pros and cons of the options

### An optional `name` on a manifest module

- Good: the label sits beside the module it labels, in every format.
- Good: the same name on several modules groups them with no second construct.
- Bad: nothing checks that a name means one thing.

### A unique module id

- Good: a stable handle for tooling to address one module.
- Bad: an author grouping modules under one label has to invent a suffix per module, and a
  copied module fails to load until it is renamed.

### A separate name table

- Good: the module shape is unchanged.
- Bad: the label is written away from the module, and reordering modules breaks the mapping by
  index.

### YAML comments

- Good: no format change and no compatibility cost.
- Bad: TOML and JSON manifests and every archive lose the label, and extraction writes none.
