# ADR-0019: Uniform type pins

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_data`
- **Related:** #191, `docs/design/game-data.md` [section 4](../design/game-data.md#s4)
  and [section 6](../design/game-data.md#s6), ADR-0016

## Context and problem statement

A type pin is a one-key mapping keyed by a type name; a YAML tag loads as that mapping
(ADR-0016). The game-data reference reads the one-key mapping by the property's type: a pin
on a scalar, a list, a map, or an option, and on a struct a descent into a field of that name
unless the key is `pointer` or `embed`. Riot has fields named `Flag`, `Hash`, `Map`, `Option`,
and `String`, and hashes names case-insensitively; no Riot field is named `Pointer` or
`Embed`.

Under that reading `!hash x` on a struct property is a descent into a field named `hash`. The
report the author receives names a missing or untypable field, not the pin. A tag in YAML
carries a structural guarantee in the author's eyes that the reading removes.

## Decision drivers

- One reading of a pin on every property kind, in every format.
- A wrong pin is reported as a wrong pin.
- No new value variant and no new document form.

## Considered options

1. **A pin on every property kind.** A one-key mapping keyed by a type name is a pin
   everywhere; on a `pointer` or `embed` property only a `pointer` or `embed` pin fits.
2. **A pinned value variant.** `Value::Pinned { name, value }` from a YAML tag, serialized in
   the declaration document under a reserved spelling; the one-key mapping keeps the by-type
   reading.
3. **Keep the by-type reading.** ADR-0016 as decided.

## Decision

**A one-key mapping whose key is a type name is a type pin on every property kind.**
`docs/design/game-data.md` [section 4](../design/game-data.md#s4) states the load-time
reading and [section 6](../design/game-data.md#s6) states the coercion.

On a `pointer` or `embed` property a `pointer` or `embed` pin constructs the struct; a pin of
any other type name is `PropertyEditSkipped` with `PinMismatch` on the key. The rule holds
inside a struct pin's `set`. A block whose only key is one of the five type-named Riot fields
is a pin; the dotted path, `a.hash: 1`, is the spelling for that field.

## Consequences

- **Positive:** a tag and the one-key mapping are one value with one meaning in every format.
- **Positive:** a wrong pin on a struct is reported under the key it sits on.
- **Negative:** a one-field block on `Flag`, `Hash`, `Map`, `Option`, or `String` is a pin.
  The dotted form and a second key are the escapes.
- **Negative:** the refusal is an apply-time diagnostic; a load has no schema to refuse
  earlier.
- **Revisit when:** a Riot field named `Pointer` or `Embed` appears, or a consumer needs a
  one-field block on a type-named field without the dotted spelling.

## Pros and cons of the options

### A pin on every property kind

- Good: one rule; no change to `Value` or to the document form.
- Bad: five field names need the dotted spelling as the only key of a block.

### A pinned value variant

- Good: a YAML tag keeps a structural distinction from a mapping.
- Bad: the declaration document is JSON and needs a reserved spelling nobody authors; a mod
  authored in TOML or JSON keeps the by-type reading, one spelling with two meanings by
  format.

### Keep the by-type reading

- Good: no change.
- Bad: a tag on a struct becomes a field lookup and the report names the wrong thing.
