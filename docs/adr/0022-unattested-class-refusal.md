# ADR-0022: Unattested class refusal

- **Status:** Accepted
- **Date:** 2026-09-20
- **Crates:** `ltk_game_data`, `ltk_overlay`
- **Related:** #259, ADR-0015, ADR-0019,
  `docs/design/game-data.md` [section 3](../design/game-data.md#s3) and
  [section 6](../design/game-data.md#s6)

## Context and problem statement

A `pointer` or `embed` pin writes a bin object of a named class. The class name is hashed and
written into the bin. `Coercer::struct_pin` asks `Schema::has_class` whether the class exists,
and that question is the only guard against a class name that names nothing.

`NoSchema` answers `true` to every class. `OverlayBuilder::new` installs `NoSchema`, and no
consumer calls `with_game_data_schema`: LTK Manager holds a `MetaSchema` and does not pass it
in. Every production build therefore accepts every class name. An authoring typo,
`!embed { class: Charcter }`, is hashed and written, and `UnknownClass` is never raised. The
client cannot construct the object the chunk then declares.

The two questions the trait asks are not the same kind of question. `expected` returning `None`
is a safe fallback: the base value's shape types the property, and an absent property is
`Untypable` and skipped, so a value is never written under a guessed type. `has_class`
returning `true` is an assertion about the game, made by a schema that holds no game.

A class already present in the base tree is in a different position. The shipped bin carries
it, so the installed game constructs it. A pin without a `class` key takes that class, and a
schema has nothing to add to what the bin already attests.

## Decision drivers

- A class hash written into a bin is attested by the game or by a schema, never by neither.
- A schema that says nothing says nothing, rather than saying yes.
- An edit that only reshapes what the base already carries keeps working with no schema.

## Considered options

1. **`NoSchema::has_class` answers `false`, and the gate spares a class the base carries.**
2. **`NoSchema::has_class` answers `false`, and the gate applies to every class.** The literal
   reading: a struct pin is refused under the default schema whatever its class source.
3. **Ship a real schema by default.** `ltk_game_data` carries a class list of the installed
   patch and `NoSchema` disappears.
4. **Leave `has_class` answering `true`.** The typo reaches the bin; the author sees the
   result in game.

## Decision

**A class the base value carries is written without asking the schema. Every other class is
refused unless the schema knows it.**

`NoSchema::has_class` answers `false` for every class. `Coercer::struct_pin` consults
`has_class` only where the pinned class differs from the base value's, which covers a pin on
a property the base omits, a null pointer repointed at a class, and a pointer moved from one
class to another. A pin that names the class its base already carries is the same pin as one
that leaves `class` out, and neither consults the schema. A refused class is
`PropertySkipReason::UnknownClass`, a skip report on that property key, and the rest of the
edit applies.

## Consequences

- **Positive:** `!embed { class: Typo }` is refused under every schema, including the default
  one every build runs with today.
- **Positive:** a partial schema - one that types fields and lists no classes - keeps working
  for the edits that reshape a base object.
- **Negative:** a struct pin naming a class the base does not carry needs a schema. Under
  `NoSchema` the property is skipped where it was previously written. This is the intended
  change: the blast radius is a pin with an empty or absent `set`, since a pin with a nonempty
  `set` already needs `expected` and is already refused with `Untypable` under `NoSchema`.
- **Negative:** a consumer whose `has_class` is conservative refuses a class the game has.
  The report names the class and the property, so the author can pin the class from the base
  instead.
- **Revisit when:** a consumer installs a real schema, at which point the default's behaviour
  stops describing production.

## Pros and cons of the options

### `NoSchema::has_class` answers `false`, gate spares a class the base carries

- Good: the refusal lands on the one class nothing attests.
- Good: an edit on an existing struct is unaffected by the absence of a schema, and spelling
  the class out does not change the answer.
- Bad: the rule is a comparison rather than a single question to the schema.

### `NoSchema::has_class` answers `false`, gate on every class

- Good: one rule, no extra state.
- Bad: refuses a class the base bin carries, which the game constructs.
- Bad: a partial schema becomes unusable for block edits on struct properties.

### Ship a real schema by default

- Good: every class name is checked against the installed patch.
- Bad: ADR-0015 keeps the schema out of this crate; the dump, its cache and its network layer
  come with it.

### Leave `has_class` answering `true`

- Good: no change.
- Bad: the only guard against an invented class name is off in every production build.
