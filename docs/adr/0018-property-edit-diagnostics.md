# ADR-0018: Property edit diagnostics

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_data`, `ltk_overlay`
- **Related:** #190, #191, `docs/design/game-data.md` [section 6](../design/game-data.md#s6),
  ADR-0008, ADR-0012, ADR-0014

## Context and problem statement

The game-data reference states that every property edit is checked against the base the way
an override record is, and reported on the same terms: a report on the edit, the edit
skipped, never a build failure. The conditions are many: the object is absent from the
target, a path segment does not resolve, the property has no type, a pin disagrees with the
type, a sign sits on a scalar, a removal matches nothing, a value does not coerce for one of
several reasons, a pinned class is unknown, a build typed a property from the base for want of
a schema.

An override record that does not apply is one `OverrideRecordSkipped` diagnostic carrying a
`SkippedRecord` with a `RecordSkipReason` code (ADR-0012). LTK Manager renders a diagnostic
from its kind and typed fields (ADR-0008); a message is not the interface.

## Decision drivers

- A consumer renders one property-edit report the way it renders one override report.
- Every condition is a code the consumer matches.
- A diagnostic names the entry and the property key the author wrote.

## Considered options

1. **One kind per condition.** `PropertyUntypable`, `PropertyCoercionFailed` with a reason,
   `PinMismatch`, `SignOnScalar`, `RemovalUnmatched`, each a variant of the diagnostic kind.
2. **One kind with a reason code.** `PropertyEditSkipped` carrying a `SkippedProperty` with
   the entry name and a `PropertySkipReason`; `SchemaFallback` as the one informational kind.

## Decision

**A skipped property edit is one `PropertyEditSkipped` diagnostic carrying a
`SkippedProperty`; a property typed from the base is one `SchemaFallback` diagnostic.**
`docs/design/game-data.md` [section 6](../design/game-data.md#s6) states the fields and the
reason codes.

The diagnostic's `path` is the signed property key as the lowering spells it, a block's inner
key joined to its outer path. `SkippedProperty` carries the entry name and a
`PropertySkipReason`. The overlay's `GameDataDiagnosticKind` mirrors the two kinds and its
diagnostic carries the `SkippedProperty` in an optional `property` field.

## Consequences

- **Positive:** the override pattern and the property pattern are the same shape; a consumer
  that renders one renders the other.
- **Positive:** a new reason is a variant of a `#[non_exhaustive]` reason enum, not a new
  diagnostic kind.
- **Negative:** a consumer filtering by kind sees one kind for every skip; the reason is one
  level down.
- **Negative:** `SchemaFallback` is one diagnostic per property typed from the base; a build
  with no schema and many edits reports many.
- **Revisit when:** a consumer needs a coercion failure's operands, such as the value and the
  range it missed; the reason code carries none.

## Pros and cons of the options

### One kind per condition

- Good: the kind alone says what happened.
- Bad: two levels of the same thing, kind and reason, for `PropertyCoercionFailed`; the
  diagnostic kind enum grows with every condition.

### One kind with a reason code

- Good: the override pattern; the detail struct is the one place the fields live.
- Bad: the reason is a field, not the kind.
