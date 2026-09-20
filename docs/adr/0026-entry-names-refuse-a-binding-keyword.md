# ADR-0026: Entry names refuse a binding keyword

- **Status:** Accepted
- **Date:** 2026-09-20
- **Crates:** `ltk_game_data`
- **Amends:** [ADR-0023](0023-refusing-serialization.md)
- **Related:** ADR-0007, #260, `docs/design/game-data.md`
  [section 3](../design/game-data.md#s3) and [section 4](../design/game-data.md#s4)

## Context and problem statement

ADR-0023 put the representability rule at serialization: a conversion to a serialized form
refuses what that form cannot carry. It rejected the alternative of refusing at construction
on two grounds, both about property paths. `PropertyEdit::parse` is also how block descent
reads an inner key, so refusing `links` there makes a struct field of that name unreachable;
and construction does not reach a signed key held twice, which lives in a public `Vec`.

Neither ground is about entry names. An entry name is never a struct field and never a block
key. `EntryName::try_from("links")` succeeds, and the declarations it goes into are refused
only at the pack, by which time the consumer is holding a `Declarations` rather than the name
it mistyped.

The crate's other identifiers already work the other way. `Target`, `LinkPath` and
`OverridePath` each refuse their invalid spellings at construction, which is rule D5 and
[ADR-0007](0007-validated-declaration-identifiers.md): an invalid identifier is unconstructible.
`EntryName` is the one identifier that carried an invariant it did not enforce.

## Decision drivers

- An identifier enforces its own invariant, or it is not one.
- A report names what the caller wrote, not what a later phase derived from it.
- ADR-0023's grounds for rejecting construction are specific to property paths.

## Considered options

1. **`EntryName` refuses a binding keyword.** The one case of ADR-0023's rejected option
   whose grounds do not apply.
2. **Leave it at serialization.** ADR-0023 as written.
3. **Refuse every name without a `/`.** A target body tells an entry name from a mistyped key
   by the `/`, so enforce that in the type too.

## Decision

**`EntryName::try_from` refuses `overrides`, `links`, `+links` and `-links`, with
`ErrorKind::ReservedBindingKey`.**

The serialization refusal stays exactly as ADR-0023 decided it. It still carries the property
path spelling a keyword and the signed key held twice, and the entry-name arm of
`TryFrom<Edit> for Bindings` stays as the backstop for a name built before this rule.

`BindingKeyword::of` is the one table of spellings. The reader that routes a body key, the
writer that refuses one, and `EntryName` all ask it, so a keyword gained or respelled is one
edit.

Option 3 is refused. A single-segment name is a legal bin object path, and
`Selector::Entries` accepts one; the `/` is how a target body disambiguates its own mapping,
which is a rule about that body, not about the name. It stays where it was, in
`Bindings::entries_of`.

## Consequences

- **Positive:** a consumer building `Declarations` by hand is told at the name it wrote.
- **Positive:** every identifier in the crate now refuses its invalid spellings at
  construction.
- **Negative:** `EntryName::try_from` gains an error case, so a caller that unwrapped a
  literal name gets a panic if that literal is a keyword. No such name is a real entry.
- **Revisit when:** an entry body stops sharing a mapping with the bindings, which is the
  same condition ADR-0023 records.

## Pros and cons of the options

### `EntryName` refuses a binding keyword

- Good: the type carries its own invariant, as every other identifier here does.
- Good: leaves ADR-0023's reasoning intact where that reasoning applies.
- Bad: the rule is now stated in two places, construction and serialization, for one of the
  three cases ADR-0023 covers.

### Leave it at serialization

- Good: one place holds the whole rule.
- Bad: the report arrives a phase after the mistake, naming a module index.

### Refuse every name without a `/`

- Good: one rule covers the keywords and the mistyped keys together.
- Bad: refuses a legal single-segment object path, which `Selector::Entries` accepts.
