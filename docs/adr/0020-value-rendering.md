# ADR-0020: Value rendering

- **Status:** Accepted
- **Date:** 2026-09-19
- **Crates:** `ltk_game_data`
- **Related:** ADR-0015, ADR-0016, ADR-0019, league-toolkit PR #240 (`FieldNames`), LTK Manager
  ADR-0042, `docs/design/game-data.md` [section 3](../design/game-data.md#s3) and
  [section 6](../design/game-data.md#s6)

## Context and problem statement

Coercion reads a `Value` as the shape of its property
([section 6](../design/game-data.md#s6)). No function reads a `PropertyValueEnum` as a `Value`.
`Shape::of` is the one reader of a base value in the crate.

LTK Manager writes property edits from the values of a game bin (manager ADR-0042): a leaf, a
whole list, a map entry, a struct. The written literal has to coerce back to the value it came
from under the same shape. Five cases break a naive rendering:

- An `f32` widened to `f64` prints as `0.10000000149011612`. The shortest `f32` spelling, `0.1`,
  reads back to the same bits.
- An `option` whose item renders as a list, written bare, reads as a list of items: an
  `option<vec3>` written `[1, 2, 3]` is `ArityMismatch`.
- A struct value is a struct pin. Every `set` key is a field name, hashed from its spelling
  (rule D23). A field with no known name has no spelling.
- A `hash`, `link`, or `file` value reads back from its name or from its `0x` spelling. A reader
  of the declaration needs the name.
- A map key is a string read by the key kind ([section 6](../design/game-data.md#s6)). A key
  kind the key rule refuses has no spelling.

The names live in consumer tables. The crate holds no table (ADR-0015). League-toolkit PR #240
adds `ltk_meta::path::FieldNames`: a field name given the class it was read on, and the plaintext
of a `Hash`-kind key. LTK Manager adapts its tables to it for `ValuePath` rendering.

The crate reads YAML and writes JSON (`manifest_json`). Authors read and write YAML.

## Decision drivers

- `coerce(render(v), shape) == v` for every kind the coercion table reads.
- The coercion table and its inverse change in one crate, under one test.
- Names come from the consumer, through a trait.
- The output reads as an author writes it: no pin a schema makes redundant.

## Considered options

1. **Rendering in `ltk_game_data`, names through a trait over `FieldNames`.** `Value::render`
   inverts coercion; `Names` extends `ltk_meta::path::FieldNames` with class, entry, and file
   names.
2. **Rendering in `ltk_game_data`, names through a trait of its own.** The same function; a
   `Names` trait with all five lookups and no `ltk_meta` supertrait.
3. **Rendering in the consumer.** LTK Manager maps its bin values to `Value` itself.
4. **Rendering in `ltk_meta`.** A value-to-literal function beside `ValuePath` rendering.

## Decision

**`ltk_game_data` renders a property value as a `Value`, with names through a `Names` trait
whose supertrait is `ltk_meta::path::FieldNames`.** A scalar renders bare; a vector, matrix, or
color renders as a list; an `f32` renders with its shortest spelling; an `option` renders its
element bare, or as a one-element list where the element renders as a list; a struct renders as a
struct pin; a `hash`, `link`, or `file` renders as its name, else its `0x` spelling. A field or a
key with no spelling is an `Error` naming the path. `Value::to_yaml` writes a `Value` as YAML.
[Section 3](../design/game-data.md#s3) states the surface and
[section 6](../design/game-data.md#s6) the rendering table.

## Consequences

- **Positive:** a change to a coercion row breaks the round-trip test in the same crate.
- **Positive:** one table adapter in a consumer serves `ValuePath` rendering and value rendering.
- **Positive:** a rendered edit carries no pin; the installed patch's schema types it at the build
  (rule D19).
- **Negative:** the crate depends on an `ltk_meta` release carrying `FieldNames`.
- **Negative:** a struct holding one unnamed field renders nothing for the whole struct. The
  consumer falls back to a leaf path per named field, or reports.
- **Negative:** a rendered edit without a pin needs a schema answer at the build where the base
  omits the property; `NoSchema` reports it `Untypable`.
- **Revisit when:** the path grammar gains a hash escape for field names, which makes every struct
  renderable.

## Pros and cons of the options

### Rendering in `ltk_game_data`, names through a trait over `FieldNames`

- Good: one adapter per consumer for both renderings; field lookup keeps the class context
  `FieldNames` defines.
- Bad: the release waits on league-toolkit PR #240.

### Rendering in `ltk_game_data`, names through a trait of its own

- Good: no `ltk_meta` release in the path.
- Bad: two traits with the same field lookup; a consumer adapts its tables twice.

### Rendering in the consumer

- Good: no public API change in this crate.
- Bad: the inverse of the coercion table lives outside the crate that owns the table. A table row
  change breaks the consumer with no failing test here.

### Rendering in `ltk_meta`

- Good: sits beside `ValuePath` rendering.
- Bad: `Value` is this crate's type; `ltk_meta` holds no declaration literal and refuses a schema
  (league-toolkit ADR-0006).
