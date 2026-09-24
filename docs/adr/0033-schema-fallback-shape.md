# ADR-0033: Schema fallback shape

- **Status:** Accepted
- **Date:** 2026-09-24
- **Crates:** `ltk_game_data`, `ltk_overlay`
- **Related:** ADR-0015, ADR-0018, ADR-0022, `docs/design/game-data.md`
  [section 3](../design/game-data.md#s3) and [section 6](../design/game-data.md#s6)

## Context and problem statement

`Schema::expected(class, field)` types a property edit (ADR-0015). A property the base holds
and `expected` does not answer is typed from the base value and reported as `SchemaFallback`.
A property the base omits and `expected` does not answer is `Untypable`, and the edit is
skipped.

LTK Manager implements `Schema` over a meta database keyed on game builds. Its `expected`
answers only for a build the database describes. A game patch ships before the database
describes its build. On that build `expected` answers `None` for every field, and every edit
that adds a field the game's copy omits is `Untypable` until the database catches up.

The database describes the preceding build. A field's shape across two adjacent builds is
almost always the same.

## Decision drivers

- An edit adding a field applies on a game build newer than the schema.
- An answer for a described build and an answer carried over from another build stay
  distinguishable to the library and to the diagnostics.
- The base value outranks any schema guess for a property it holds.
- Existing `Schema` implementations keep compiling.

## Considered options

1. **A second, defaulted trait method `Schema::fallback(class, field)`.** Asked only where
   `expected` is `None` and the base omits the property.
2. **The consumer answers the newest described build inside `expected`.** No trait change.
3. **`Untypable` until the schema describes the build.** No change.

## Decision

**`Schema` gains `fallback(&self, class: BinHash, field: BinHash) -> Option<Shape>`, with a
default body answering `None`. A property the base omits takes the shape `fallback` answers
where `expected` is `None`. `expected` outranks it everywhere, and a base value outranks it for
a property the base holds.** The same rule types a field reached by block descent, a struct
pin's `set` field, and an object construction's `set`. A property typed through `fallback` is
one `SchemaFallback` diagnostic, the kind a property typed from the base reports.
`NoSchema` answers `None`. `docs/design/game-data.md` [section 3](../design/game-data.md#s3)
states the trait and [section 6](../design/game-data.md#s6) states the typing rule.

## Consequences

- **Positive:** a declaration adding a field applies on the first day of a patch, typed by the
  shape the preceding build records.
- **Positive:** `expected` keeps its meaning. A consumer that validates or renders against
  `expected` does not see a guess as a fact.
- **Positive:** the method is defaulted, and the blanket implementations for `&S`, `Box<S>`,
  and `Arc<S>` forward it. An existing implementation compiles unchanged. Adding a defaulted
  method to a trait is a minor change under the Rust API evolution rules (RFC 1105).
- **Positive:** the diagnostic kinds are unchanged. A consumer rendering
  `SchemaFallback` as informational renders the new case the same way.
- **Negative:** a field whose shape changed on the undescribed build is written under the old
  shape. The client reads a value of the wrong kind as absent or rejects the bin.
- **Negative:** `SchemaFallback` covers two sources, the base value and `fallback`. The
  diagnostic does not say which.
- **Revisit when:** the schema carries shapes per build range, or a consumer needs to tell a
  base-typed property from a fallback-typed one.

## Pros and cons of the options

### A defaulted `Schema::fallback`

- Good: the library knows which answer is a guess and asks for it only where the base has no
  shape of its own.
- Good: no break for an implementation that does not answer it.
- Bad: one more method in the trait's contract.

### The newest described build inside `expected`

- Good: no trait change.
- Bad: a guess outranks the base value for a property the base holds, and a changed field is
  written under the stale shape where the base carries the right one.
- Bad: `expected` no longer means the build's own schema, and every other reader of it inherits
  the guess.

### `Untypable` until the schema describes the build

- Good: no value is written under a shape the installed build does not attest.
- Bad: every declaration adding a field fails after each patch, until the database catches up.
