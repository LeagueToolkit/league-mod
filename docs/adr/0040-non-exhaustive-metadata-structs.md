# ADR-0040: Non-exhaustive metadata structs

- **Status:** Proposed
- **Date:** 2026-10-26
- **Crates:** `ltk_mod_project`, `ltk_fantome`, `ltk_overlay`, `league-mod`
- **Related:** ADR-0039

## Context and problem statement

`ModProject` in `ltk_mod_project` and `FantomeInfo` in `ltk_fantome` are public structs with
public fields. Every field added to one of them breaks each struct literal of it outside its
crate, including a literal with `..Default::default()`. The `distributor` field of ADR-0039 is
such an addition to both.

Outside their crates, 18 struct literals build a `ModProject` and 9 build a `FantomeInfo`, in
`ltk_mod_project`, `ltk_overlay`, `league-mod` and their tests. LTK Manager builds no
`ModProject` and builds 5 `FantomeInfo` literals, all in test code.

## Decision drivers

- A field added to `ModProject` or `FantomeInfo` is a minor, non-breaking release.
- A caller outside the crate reads every field directly.
- Neither crate adds a constructor or builder that has to track the field list.

## Considered options

1. **`#[non_exhaustive]` on both structs.** A caller outside the crate starts from
   `Default::default()` and assigns the fields.
2. **Keep the structs exhaustive.** Each added field is a breaking release.
3. **Private fields with a builder.** Every field gets a setter and a getter.

## Decision

**`ModProject` and `FantomeInfo` are `#[non_exhaustive]`. A caller outside the defining crate
builds one from `Default::default()` and field assignment.** Inside the defining crate, struct
literals and struct update syntax are unchanged.

## Consequences

- **Positive:** a field added to either struct breaks no caller.
- **Negative:** the release that adds the attribute is breaking for both crates, and each struct
  literal outside the defining crate becomes a `let mut` and a sequence of assignments.
- **Negative:** the compiler no longer reports a caller that leaves a new field at its default.
- **Negative:** `FantomeLayerInfo`, `FantomeHashtable`, `ModProjectLayer` and the other nested
  metadata structs stay exhaustive. A field added to one of them is still a breaking release.
- **Revisit when:** a field of either struct gains an invariant that field assignment can break.

## Pros and cons of the options

### `#[non_exhaustive]` on both structs

- Good: field reads and serde stay unchanged.
- Bad: construction outside the defining crate is more verbose.

### Keep the structs exhaustive

- Good: struct literals stay available to every caller.
- Bad: each field addition is a breaking release for every consumer.

### Private fields with a builder

- Good: the crate can validate each field on assignment.
- Bad: a getter and a setter per field, and every reader of a field changes.
