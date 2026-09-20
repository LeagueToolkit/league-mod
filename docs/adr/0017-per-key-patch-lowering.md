# ADR-0017: Per-key patch lowering

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_data`
- **Related:** #190, #191, `docs/design/game-data.md` [section 6](../design/game-data.md#s6),
  ADR-0012, ADR-0016

## Context and problem statement

An entry body's property edits are applied over the decoded base tree. The game-data
reference states: `+` on a list is materialised as a whole-list replacement holding the
base's elements, less removals, followed by the additions; `+` on a map adds or replaces by
key and is materialised as a whole-map replacement; on one key within one batch the set
applies first, then the removals, then the additions; a block on a struct merges field by
field. The `{k}` map subscript has no shipped example and is an open question of the
research note (`docs/research/issue-190-declaring-bin-patches.md`
[section 5.1](../research/issue-190-declaring-bin-patches.md#s5.1), question 3).

`ltk_meta` 0.8.2 sets one property the way a `PTCH` record does: `Bin::patch(object, path,
value)` inserts a property the object lacks when the path's last segment has no subscript,
and refuses a value whose shape differs from the base's. `BinOverride::apply` runs the same
operation per record. Vector and colour values are `glam` and `ltk_primitives` types; both
crates are in the dependency tree through `ltk_meta` and neither is re-exported.

## Decision drivers

- One materialiser: an override record and a property edit set a property the same way.
- The reference's per-key order and whole-container replacement.
- A property the base omits is created; a container element the base lacks is a report.

## Considered options

1. **Group the leaf edits by property path, compute one value per key, set it with
   `Bin::patch`.** Descents are flattened to leaf edits first; a key's set, removals, and
   additions produce one container value from the base; `{k}` is never emitted.
2. **Build a `BinOverride` with `Builder` and lay it over the tree with `BinOverride::apply`.**
   The same records, batched.
3. **Mutate the tree in place with container operations.** Push to a list, insert into a map,
   through `ValueSlot`.

## Decision

**Property edits lower to one `Bin::patch` per property key, in first-occurrence order, each
carrying the whole value the key holds after its set, removals, and additions.**
`docs/design/game-data.md` [section 6](../design/game-data.md#s6) states the lowering, the
coercion table, and the removal rules.

A map edit is a whole-map replacement; no `{k}` record is emitted. A descent is flattened to
leaf edits on the struct's fields; the struct itself is never replaced by a block. `glam`
and `ltk_primitives` are direct dependencies of `ltk_game_data` at the versions `ltk_meta`
0.8.2 depends on; they construct vector, matrix, and colour values.

## Consequences

- **Positive:** the format crate performs every write; a property edit and an override
  record are reported on the same terms.
- **Positive:** the per-key order is a property of the grouping, not of the author's key
  order.
- **Negative:** a whole-container replacement re-encodes every element of a list or map the
  edit touches, including elements the edit does not name.
- **Negative:** a schema type that disagrees with the base's shape is a `Bin::patch` refusal
  and a report; the edit cannot retype a property the base carries.
- **Negative:** two direct dependencies whose versions follow `ltk_meta`'s.
- **Revisit when:** `ltk_meta` grows container operations over a patched stream, or a `{k}`
  record has a shipped example and a reason to exist.

## Pros and cons of the options

### Group by key, one `Bin::patch` per key

- Good: the reference's rule read literally; one write path.
- Bad: whole-container re-encode.

### Build a `BinOverride` and apply it

- Good: the same records as an override file; the batch is inspectable.
- Bad: the report comes back by record index and must be mapped to the edit; the records are
  the same `patch` calls with one indirection.

### Mutate the tree in place

- Good: no re-encode of untouched elements.
- Bad: a second write path beside the format crate's, with its own type checks and its own
  skip semantics.
