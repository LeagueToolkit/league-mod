# ADR-0010: Phased declaration edits

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_data`, `ltk_overlay`
- **Related:** #233, #234, `docs/design/game-data.md` [section 4](../design/game-data.md#s4)
  and [section 6](../design/game-data.md#s6)

## Context and problem statement

The game-data standard defines a module body as an ordered list of batches. One batch holds
bindings of several kinds, applied in fixed phases: override files, object creation, entry
edits, object removal, dependency-list edits. Each batch reads the result of the preceding one.
The build plan retains module and batch boundaries and never flattens repeated writes.

The link engine implements one binding kind. Its Rust unit has to be the shape the full
standard needs, so that every later binding kind lands without a second model. The standard
gives an `entries` item a body of property edits and reserves `links` for a `target` item; a
link-only engine that honours that has nothing to do with an entry name.

## Decision drivers

- The batch is the unit the standard reasons about: order, phases, precedence, diagnostics.
- Every later binding kind is an addition, never a redesign.
- A mod author can add a link to a chunk known only by an entry it declares.
- Serialized shapes stay the wiki's: `steps`, compact binding bodies.

## Considered options

1. **A phased struct per batch**: `Edit` with one field per phase in apply order, `Vec<Edit>`
   per target, `EntryEdit` with the bindings an entry body takes.
2. **A flat operation list**: `enum Edit { AddLink, RemoveLink, .. }`, batches flattened at
   load time, the engine a fold over operations.
3. **A group struct named for the manifest key**: `Step { add_links, remove_links }`, one new
   optional field and one new engine phase rule per binding kind.

## Decision

**A batch is an `Edit` whose fields are its phases; the engine applies the fields in
declaration order. An entry body is an `EntryEdit`, and it takes `links` as an extension of the
standard.**

`Selector::Target { target, edits: Vec<Edit> }` and `Selector::Entries(IndexMap<EntryName,
EntryEdit>)`. `Edit` and `EntryEdit` are `#[non_exhaustive]` with `Default`; a binding kind is a
new field. `apply(base, &[Edit])` runs each edit's phases in field order. An `entries` item is
one batch: no `steps` inside an entry body. The wire keys are the manifest's: `steps` for the
edit list, the compact binding body for one edit or one entry.

## Consequences

- **Positive:** phase order is the struct's field order, checked by no engine rule. A binding
  kind lands as one field, one wire key, and one phase in `apply`. An entry name reaches a chunk
  the author cannot spell.
- **Negative:** the Rust name `Edit` and the manifest key `steps` differ, one more row in the
  name mapping. `links` inside an entry body is not in the wiki standard until the wiki carries
  it. A consumer constructs an `Edit` through `Default` and field assignment.
- **Revisit when:** a binding kind needs an order the phase list cannot express.

## Pros and cons of the options

### A phased struct per batch

- Good: the standard's own unit, its phases typed.
- Good: additive growth.
- Bad: the type and the manifest key have different names.

### A flat operation list

- Good: one enum, one fold, precise per-operation diagnostics.
- Bad: loses the batch boundary the plan retains and the phase rule the standard states.
- Bad: a document round trip regroups operations and is canonical, not identical.

### A group struct named for the manifest key

- Good: no rename.
- Bad: every binding kind adds an `Option` field and a phase rule in the engine.
- Bad: the name describes the manifest, not what the value does.
