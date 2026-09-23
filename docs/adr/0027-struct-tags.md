# ADR-0027: Struct tags

- **Status:** Accepted
- **Date:** 2026-09-23
- **Crates:** `ltk_game_data`
- **Related:** #191, `docs/design/game-data.md` [section 4](../design/game-data.md#s4),
  [section 3](../design/game-data.md#s3) and [section 8](../design/game-data.md#s8), ADR-0016,
  ADR-0019

## Context and problem statement

A struct pin is a one-key mapping keyed `pointer` or `embed` over a mapping of `class` and
`set` (ADR-0016). A YAML tag loads as the one-key mapping of its name. A YAML author writes
the pin as `!pointer {class: C, set: {...}}`. The fields of a new struct sit two levels under
the property, and a hash-form class, `"0x50db156b"`, needs quotes.

A pointer or embed is the most nested value in a bin, and a skin or map edit adds several.
Ritobin, the text form mod authors read bins in, spells one as `pointer = C { ... }`: the
class on the same line as the kind, the fields directly under it.

A YAML local tag is `!` and a run of URI characters. `(`, `)`, `:`, `.`, and `/` are among
them; `[`, `]`, `{`, `}`, and `,` are flow indicators and end the tag. `serde-saphyr` passes a
local tag through `Tagged` with its characters intact.

The declaration document is JSON (ADR-0016). A TOML or JSON author writes the one-key mapping
directly and has no tag.

## Decision drivers

- A new struct in YAML reads like the struct in ritobin: kind, class, fields.
- The declaration document, TOML, and JSON keep one shape.
- One YAML spelling per pin, in authored files and in `Value::to_yaml` output.

## Considered options

1. **Class in parentheses, the tag's value the `set`.** `!pointer(C) {f: 1}`; `!embed {f: 1}`
   takes the base's class.
2. **Class after a `:`, `.`, or `/`.** `!pointer:C {f: 1}`, `!pointer.C`, `!pointer/C`.
3. **Class in brackets.** `!pointer[C]`, the ritobin subscript look.
4. **Keep `class` and `set` under the tag.** `!pointer {class: C, set: {f: 1}}`.
5. **The class alone as the tag.** `!C {f: 1}`, the kind read from the schema.

## Decision

**A YAML struct tag names the class in parentheses, and its value is the pin's `set`.**
`docs/design/game-data.md` [section 4](../design/game-data.md#s4) states the loading rule and
[section 3](../design/game-data.md#s3) the writing rule.

`!pointer(C) {f: 1}` loads as `{pointer: {class: C, set: {f: 1}}}`. A mapping under a struct
tag is fields of the class, and `class` and `set` in it are field names. `Value::to_yaml`
writes every struct pin whose class a tag carries as a struct tag.

## Consequences

- **Positive:** the fields of a new struct sit one level under the property, and a hash-form
  class needs no quotes.
- **Positive:** the document form and the TOML and JSON spellings are unchanged.
- **Negative:** a YAML file written as `!pointer {class: C, set: {...}}` loads as a struct
  with fields named `class` and `set`. The build reports the unknown fields; the load does not.
- **Negative:** the YAML spelling and the document spelling of a struct pin differ in shape.
  A reader moving between YAML and JSON translates one into the other.
- **Negative:** a class spelled with a character outside a tag's alphabet has no tag
  spelling. `Value::to_yaml` writes such a pin as the one-key mapping.
- **Revisit when:** a bin class name holds a character a YAML tag cannot carry unescaped, or
  a consumer needs a struct pin's class and fields as separate YAML nodes.

## Pros and cons of the options

### Class in parentheses, the tag's value the `set`

- Good: reads as a constructor call; no other part of the syntax uses parentheses.
- Good: `(` and `)` are tag characters; the parser needs no escape.
- Bad: a YAML tag is not a place authors expect arguments.

### Class after a `:`, `.`, or `/`

- Good: equally valid tag characters.
- Bad: `:` is the reference separator (`!ref Entry:path`) and reads as a YAML key; `.` is the
  property path separator; `/` is the entry path separator. Each reads as another part of the
  syntax.

### Class in brackets

- Good: closest to ritobin's look.
- Bad: `[` is a flow indicator and ends the tag. The parser reads `!pointer[C]` as the tag
  `!pointer` over the list `[C]`.

### Keep `class` and `set` under the tag

- Good: the YAML spelling mirrors the document form.
- Bad: two extra keys and one extra level on every new struct; the hash-form class needs
  quotes.

### The class alone as the tag

- Good: the shortest spelling; the build knows whether the property is a pointer or an embed.
- Bad: the document form needs a new kind-neutral pin, a `Value` change and a second reading of
  the one-key mapping. A class named like one of the type-named Riot fields collides with
  ADR-0019's pins.
