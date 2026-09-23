# ADR-0029: Object bindings

- **Status:** Accepted
- **Date:** 2026-09-23
- **Crates:** `ltk_game_data`, `ltk_overlay`
- **Related:** #191, ADR-0010, ADR-0012, ADR-0017, ADR-0022,
  `docs/design/game-data.md` [section 4](../design/game-data.md#s4) and
  [section 6](../design/game-data.md#s6)

## Context and problem statement

An entry edit on an object the target does not hold is `MissingObject`. A mod adding a
particle, a resolver, or a skin variant ships an override file with the object in it, or a
whole replaced bin. The game-data reference defines an `objects` binding: `clone` of an entry
of the target, or `class`, each with a `set` entry body; and `remove: true`. It orders a
batch as override files, object creation, entry edits, object removal, and dependency-list
edits, and reads every clone from the state at the start of the creation phase.

An `Edit` is a struct whose fields are its phases (ADR-0010). `ltk_meta` writes an object into
a `Bin` and deletes one; `BinOverride` carries added and deleted objects for the client.

A constructed object needs a class. The client constructs a live object from its class before
it reads the file, and a property the file omits holds the constructor's value. The class
schema answers a field's shape and whether a class exists, and carries no default values.

## Decision drivers

- The reference's phase order and clone reading, with no second reading of either.
- One entry-body grammar for an entry edit and an object's `set`.
- A creation or removal that does not apply is a code with the object's name, the rest of the
  batch applies.
- `ltk_meta` performs every write (ADR-0017).

## Considered options

1. **An `objects` field on `Edit` with its own two phases.** `ObjectEdit` is `Clone`,
   `Construct`, or `Remove`; creation follows the override files, removal follows the entry
   edits.
2. **Objects as a separate module selector.** An `objects` module beside `target` and
   `entries`, applied before every entry edit of the layer.
3. **Objects through override files only.** The binding lowers to a `BinOverride` with
   `objects` and `deleted`, applied as an override file.

## Decision

**`Edit::objects` holds the object edits of a batch. Creation runs after the override files
and removal after the entry edits.** `docs/design/game-data.md`
[section 4](../design/game-data.md#s4) states the body and
[section 6](../design/game-data.md#s6) the phases and their reports.

A constructed object holds its class and the properties its `set` names, and no other
property. A class the schema does not know is `UnknownClass`, the rule a struct pin's class
follows (ADR-0022). A skip is one `ObjectSkipped` diagnostic with an `ObjectSkipReason`.

## Consequences

- **Positive:** an entry edit in the same batch reaches a created object, and edits an object
  the batch then removes.
- **Positive:** a `set` is an entry body; coercion, pins, and references apply unchanged.
- **Negative:** a batch cannot clone an object it creates. Two edits express the dependency.
- **Negative:** a constructed object carries no default values. The written object omits every
  property its `set` does not name, and the client reads the constructor's value for each.
- **Negative:** `ApplyDiagnostic` and `GameDataDiagnostic` gain an `object` field, a breaking
  change for a consumer constructing either.
- **Revisit when:** a consumer needs a constructed object's defaults written out, or the
  reference changes the phase order.

## Pros and cons of the options

### An `objects` field on `Edit` with its own two phases

- Good: the reference's batch, phase for phase; a later edit reads what an earlier one
  created.
- Bad: two phases for one binding, and an `Edit` field whose entries apply at two points.

### Objects as a separate module selector

- Good: one phase, and an object is never both created and edited in one batch.
- Bad: the reference puts `objects` in a target body; a module cannot clone an object an
  earlier edit of the same target created without a second module.

### Objects through override files only

- Good: `BinOverride::apply` already adds and deletes objects.
- Bad: an override record carries no coercion, no pins, and no references. A `set` would
  need a second grammar; `BinOverride` replaces an existing object where the reference skips
  the creation.
