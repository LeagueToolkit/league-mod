# ADR-0016: Literal property values

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_data`
- **Related:** #190, #191, `docs/design/game-data.md` [section 4](../design/game-data.md#s4)
  and [section 5](../design/game-data.md#s5), ADR-0010, ADR-0011

## Context and problem statement

An entry body holds property edits: a signed property path and a value. The game-data
reference states that an unpinned value adapts to whatever type the property has on the day
the overlay is built, that a mapping on a struct property descends into it, and that a
mapping on a map property is a set. Which of these a mapping is depends on the property's
type, and the type is known at build time, not at load time.

A type pin is a YAML local tag, `!f32 1.0`, or in TOML and JSON a one-key mapping,
`{ f32 = 1.0 }`. A declaration document is JSON; a tag cannot travel in it. The reference
reads the one-key mapping by the property's type: a pin on a scalar or a container, a descent
on a struct unless the key is `pointer` or `embed`.

`serde-saphyr` captures a local tag through its `Tagged<T>` wrapper at every nesting level
when unsupported tags are not rejected; its strict mode refuses every local tag before the
target type is known.

## Decision drivers

- The declaration carries what the author wrote; the build decides what it means.
- One document form across YAML, TOML, and JSON.
- Structural validation at load: path grammar, tag names, the shape of a struct pin.
- Duplicate keys are refused in every format.

## Considered options

1. **A literal value, with a YAML tag lowered to the one-key mapping.** `Value` is null,
   boolean, integer, float, string, list, or mapping; a tag `!t v` loads as `{t: v}`; the
   build reads a one-key mapping by the property's type.
2. **A distinct pinned form in the document.** `Value::Pinned { name, value }` serialized
   with a reserved key, so a tag stays distinguishable from a one-key mapping.
3. **Coerce at load time.** The loader takes a schema and stores `ltk_meta` values.

## Decision

**A property edit carries a literal `Value`; a YAML tag is lowered to the one-key mapping
at load.** `docs/design/game-data.md` [section 4](../design/game-data.md#s4) states the value
model and the load-time checks; [section 5](../design/game-data.md#s5) states the document
form.

An execution read of YAML captures local tags; a tag whose name is not a type name is a load
error. A one-key mapping keyed `pointer` or `embed` is a struct pin in every format and its
shape is checked at load. Every other one-key mapping is read at build time by the property's
type. Every mapping refuses a duplicate key in every format.

## Consequences

- **Positive:** one `Value` type, one document form, no schema at load.
- **Positive:** a retype between patches does not fail a declaration.
- **Negative:** a YAML tag on a struct-typed property is read as a descent into a field named
  by the type name, the same as the one-key mapping. The reference's distinction between the
  two spellings on a struct is not preserved; both are a report.
- **Negative:** the parser accepts a local tag anywhere in a YAML manifest; only a tag on a
  value is validated.
- **Revisit when:** a declaration document format carries tags, or a consumer needs to
  distinguish a pinned value from a one-key mapping after loading.

## Pros and cons of the options

### A literal value, tag lowered to the one-key mapping

- Good: the reference's own rule; JSON and TOML authors write the document form directly.
- Bad: the tag's structural guarantee on a struct is lost.

### A distinct pinned form in the document

- Good: a tag and a mapping stay distinguishable everywhere.
- Bad: a reserved key in the document that no format's author writes; JSON and TOML authors
  cannot produce it, so the two spellings diverge in meaning by format.

### Coerce at load time

- Good: the document holds typed values; the build applies without a schema.
- Bad: the project crate needs a schema to pack; a retype between patches breaks a packed
  mod; the reference's "adapts on the day the overlay is built" is not met.
