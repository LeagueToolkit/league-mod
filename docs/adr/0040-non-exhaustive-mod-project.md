# ADR-0040: Non-exhaustive mod project

- **Status:** Proposed
- **Date:** 2026-10-26
- **Crates:** `ltk_mod_project`, `ltk_overlay`, `league-mod`
- **Related:** ADR-0039

## Context and problem statement

`ModProject` is a public struct with public fields. Every field added to it breaks each struct
literal outside the crate, including a literal with `..Default::default()`. The `distributor`
field of ADR-0039 is such an addition.

Outside the crate, 18 struct literals in `ltk_overlay`, `league-mod` and the crate's own
integration tests build a `ModProject`. LTK Manager builds none.

## Decision drivers

- A field added to `ModProject` is a minor, non-breaking release.
- A caller outside the crate reads every field directly.
- The crate adds no constructor or builder that has to track the field list.

## Considered options

1. **`#[non_exhaustive]` on `ModProject`.** A caller outside the crate starts from
   `ModProject::default()` and assigns the fields.
2. **Keep the struct exhaustive.** Each added field is a breaking release.
3. **Private fields with a builder.** Every field gets a setter and a getter.

## Decision

**`ModProject` is `#[non_exhaustive]`. A caller outside `ltk_mod_project` builds one from
`ModProject::default()` and field assignment.** Inside the crate, struct literals and struct
update syntax are unchanged.

## Consequences

- **Positive:** a field added to `ModProject` breaks no caller.
- **Negative:** the release that adds the attribute is breaking, and each struct literal outside
  the crate becomes a `let mut` and a sequence of assignments.
- **Negative:** the compiler no longer reports a caller that leaves a new field at its default.
- **Revisit when:** a field of `ModProject` gains an invariant that field assignment can break.

## Pros and cons of the options

### `#[non_exhaustive]` on `ModProject`

- Good: field reads and serde stay unchanged.
- Bad: construction outside the crate is more verbose.

### Keep the struct exhaustive

- Good: struct literals stay available to every caller.
- Bad: each field addition is a breaking release for every consumer.

### Private fields with a builder

- Good: the crate can validate each field on assignment.
- Bad: a getter and a setter per field, and every reader of a field changes.
