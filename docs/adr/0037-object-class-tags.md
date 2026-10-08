# ADR-0037: Object class tags

- **Status:** Accepted
- **Date:** 2026-10-08
- **Crates:** `ltk_game_data`
- **Related:** `docs/design/game-data.md` [section 4](../design/game-data.md#s4) and
  [section 8](../design/game-data.md#s8), ADR-0016, ADR-0027, ADR-0029

## Context and problem statement

A constructed object is an object body of `class` beside a `set` mapping (ADR-0029). The
properties of the object sit two levels under its name, and each object spends two lines on
the keys `class` and `set`.

A tool that writes a whole bin as a declaration writes one constructed object per object of
the bin. In game 16.20, `data/characters/teemo/teemo.bin` holds 50 objects and
`data/maps/mapgeometry/sr/npe_1.materials.bin` holds 275.

Ritobin, the text form mod authors read bins in, spells an object as `"Name" = Class { ... }`:
the class on the line of the name, the properties directly under it. A struct inside the
object is written in YAML as a struct tag, `!embed(C)` on the mapping of its fields
(ADR-0027).

An object body is not a property value. No type pin, reference, or struct pin applies to it,
and the loader refuses a tag that is none of these. The body of an object under `objects` is
an object whatever its tag spells.

The declaration document is JSON (ADR-0016). A TOML or JSON author writes `class` and `set`
and has no tag.

## Decision drivers

- A constructed object in YAML reads like the object in ritobin: name, class, properties.
- The declaration document, TOML, and JSON keep one shape.
- A tag on an object body has one reading.

## Considered options

1. **The class alone as the tag, the tag's value the `set`.** `Name: !C {f: 1}`.
2. **A keyword tag with the class in parentheses.** `Name: !object(C) {f: 1}`, the shape of a
   struct tag.
3. **A struct tag on the object body.** `Name: !embed(C) {f: 1}`.
4. **Keep `class` and `set`.** `Name: {class: C, set: {f: 1}}`.

## Decision

**A YAML tag on an object body names the class, and its value is the body's `set`.**
`docs/design/game-data.md` [section 4](../design/game-data.md#s4) states the loading rule.

`Name: !C {f: 1}` loads as `Name: {class: C, set: {f: 1}}`. A mapping under a class tag is an
entry body, and `clone`, `class`, `set`, and `remove` in it are property paths. A tag that is a
type name, `ref`, or a struct tag is a syntax error on an object body.

## Consequences

- **Positive:** the properties of a constructed object sit one level under its name, and a
  hash-form class needs no quotes.
- **Positive:** the document form and the TOML and JSON spellings are unchanged.
- **Negative:** a YAML file written as `!C {class: D, set: {...}}` loads as an object of class
  `C` with properties named `class` and `set`. The build reports the skipped edits; the load
  does not.
- **Negative:** the YAML spelling and the document spelling of a constructed object differ in
  shape, and a clone and a removal have no tag spelling. One `objects` mapping holds bodies of
  two shapes.
- **Negative:** a class spelled with a character outside a tag's alphabet has no tag spelling.
  Its object body is `class` and `set`.
- **Revisit when:** a clone needs a tag spelling, or a bin class name holds a character a YAML
  tag cannot carry unescaped.

## Pros and cons of the options

### The class alone as the tag, the tag's value the `set`

- Good: the shortest spelling, and the position under `objects` already names the kind.
- Good: the tag lowers to the `class` and `set` of the document form; no model type changes.
- Bad: a struct tag written on an object body by habit is refused at loading.

### A keyword tag with the class in parentheses

- Good: one shape for a struct tag and an object tag.
- Bad: the keyword repeats what the `objects` key states. `pointer` and `embed` are two
  types, and a struct tag names one of them. An object body has one kind.

### A struct tag on the object body

- Good: no new spelling for an author to learn.
- Bad: an object is neither a pointer nor an embed. The tag names a type the body does not
  have, and `!pointer(C)` and `!embed(C)` are two spellings of one object.

### Keep `class` and `set`

- Good: the YAML spelling mirrors the document form.
- Bad: two extra keys and one extra level on every constructed object.
