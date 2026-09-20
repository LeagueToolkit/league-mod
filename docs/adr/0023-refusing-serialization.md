# ADR-0023: Refusing serialization

- **Status:** Accepted
- **Date:** 2026-09-20
- **Crates:** `ltk_game_data`, `ltk_mod_project`
- **Related:** #260, #261, ADR-0006, ADR-0011, ADR-0016, ADR-0020,
  `docs/design/game-data.md` [section 3](../design/game-data.md#s3) and
  [section 4](../design/game-data.md#s4)

## Context and problem statement

`Declarations` is the in-memory model. `DeclarationDocument` and the JSON manifest are its
serialized forms. The model holds values the forms cannot carry:

- An entry body is a flat mapping. The binding keywords `overrides`, `links`, `+links` and
  `-links` share it with the entry names of a target body and the signed property paths of an
  entry body. `EntryEdit { properties: [PropertyEdit { path: "links" }] }` writes
  `{"links": [], "links": 1}`, a duplicate JSON key. `Edit { entries: {"links": ...} }` writes
  a mapping whose entry and every property edit under it are gone.
- `EntryEdit::properties` is a `Vec`. Two edits under one signed key are what `Group.additions`
  in `apply::entries` exists to express. A mapping holds one value per key, so all but the last
  are dropped.
- `Value::Integer` holds an `i128`. The manifest formats carry the union of the `i64` and `u64`
  ranges.

None of the three is reachable by parsing a manifest: `Fields::read` intercepts the binding
keywords, a mapping refuses a duplicate key, and a parser demotes an out-of-range integer to a
float. All three are reachable from a consumer building `Declarations` by hand, which is the
authoring path ADR-0020 and ADR-0021 describe, and LTK Manager is that consumer.

`Serialize` was reached through `#[serde(into = "document::Bindings")]`, an infallible
conversion, and `From<Declarations> for DeclarationDocument` carried an `expect`. Every
unrepresentable value was silently dropped or panicked.

## Decision drivers

- What a consumer writes it can read back, or it learns why not.
- A panic is not a report.
- The parser's rules and the writer's rules are one set of rules.

## Considered options

1. **Serialization refuses.** The conversions to the document form are fallible, and
   `DeclarationDocument` is built with `TryFrom`.
2. **Construction refuses.** `PropertyEdit::parse` and `EntryName::try_from` refuse a binding
   keyword, so an unrepresentable `Declarations` cannot be built.
3. **Serialization normalizes.** Same-key additions fold into one list value, an out-of-range
   integer clamps, a colliding key is renamed.

## Decision

**A conversion to a serialized form refuses what that form cannot carry, and names what it
refused.**

`TryFrom<Edit> for Bindings` and `TryFrom<EntryEdit> for Bindings` replace the `From` impls.
`Edit` and `EntryEdit` implement `Serialize` by building a `Bindings` and reporting a refusal
as a serializer error. `TryFrom<Declarations> for DeclarationDocument` replaces
`From<Declarations>`. `Value`'s `Serialize` refuses an integer outside the union of the `i64`
and `u64` ranges.

The codes are `ErrorKind::ReservedBindingKey` for a property path or entry name spelling a
binding keyword, `ErrorKind::DuplicatePropertyKey` for one signed key held twice, and
`ErrorKind::Serialize` for what `serde_json` reports. `Declarations::validate` refuses a
target module with no edit and an entries module with no entry, which a manifest also refuses,
so `manifest_json()` output loads.

`DeclarationDocument`'s `Deserialize` refuses a duplicate mapping key anywhere in the
document, the rule the manifest path already obeys.

## Consequences

- **Positive:** every `DeclarationDocument` and every `manifest_json()` output reloads to the
  declarations it was written from.
- **Positive:** a consumer holding `Value::Integer(i128::MAX)` gets an error where it got a
  panic.
- **Positive:** the archive-metadata path no longer drops the first of two bindings an author
  wrote.
- **Negative:** `From<Declarations> for DeclarationDocument` is gone. A caller writes
  `DeclarationDocument::try_from(declarations)?`.
- **Negative:** `Edit` and `EntryEdit` clone themselves to serialize. A document is written
  once per pack, so the copy is not on a hot path.
- **Revisit when:** an entry body stops being a flat mapping, which would remove the
  collision the refusal exists for.

## Pros and cons of the options

### Serialization refuses

- Good: one place holds the representability rule, and it is the place that needs it.
- Good: the in-memory model keeps the shape `apply` wants, a `Vec` of edits per key.
- Bad: `Serialize` can fail, which a caller collecting into a `String` has to handle.

### Construction refuses

- Good: an unrepresentable value never exists.
- Bad: `PropertyEdit::parse` is also how block descent reads an inner key, so refusing `links`
  there makes a struct field named `links` unreachable in the block form.
- Bad: it does not reach the duplicate signed key, which lives in a public `Vec` field.

### Serialization normalizes

- Good: no error to handle.
- Bad: folding two additions into one list is only correct for a list, and the writer does
  not know the property's type.
- Bad: a clamp writes a number the consumer did not ask for.
