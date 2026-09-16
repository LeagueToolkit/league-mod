# ADR-0014: Coded declaration errors

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_data`, `ltk_mod_project`, `ltk_overlay`
- **Related:** #190, #191, `docs/design/game-data.md` [section 3](../design/game-data.md#s3)
  and [section 4](../design/game-data.md#s4), ADR-0008

## Context and problem statement

`ltk_game_data::Error` is the one error type of declaration loading, document parsing, and
application. LTK Manager renders it per mod, in the user's language, next to the typed
diagnostics of ADR-0008. A diagnostic carries a code and typed fields; the manager matches
the code. An error carrying an English sentence cannot be matched, localized, or rendered
differently per surface.

Property edits add error sites for path grammar, sign legality, pin names, and pin shape.
Every one of them is a condition the manager's editor names to the author.

An error's place is a manifest or source file, a module index, an edit index, an entry
name, and a binding key or property path. The manager's editor positions a cursor from
these parts.

## Decision drivers

- One translation point: the consumer renders, the library states.
- A place is data the editor navigates to, not text it parses.
- Callers outside the crate construct errors for inputs they own: missing, ignored, or
  escaping files, I/O failures.

## Considered options

1. **A code and a typed location.** `Error { kind: ErrorKind, location: Location }`; the
   kind is a `#[non_exhaustive]` enum; a parser's own statement travels as a field of the
   `Syntax` code.
2. **Keep the prose pair.** `Error { location: String, message: String }`, with a code added
   beside the message.
3. **One error enum per stage.** A loading error, a document error, an application error,
   each with its own location shape.

## Decision

**An error is a code with a typed location.** `docs/design/game-data.md`
[section 3](../design/game-data.md#s3) states the shape and
[section 4](../design/game-data.md#s4) states the codes a loader reports.

A `Location` names the document, module index, edit index, entry name, and key that apply;
every field is optional. An error constructed at an inner site gains its outer context
through builder methods that fill an unset field and leave a set one alone. `Display`
renders the location and the code's statement for logs; the statement is not the
interface.

## Consequences

- **Positive:** the manager matches `ErrorKind` and positions its editor from `Location`.
- **Positive:** a parser's statement stays available as the `Syntax` code's `detail` field.
- **Negative:** every caller that constructed an error from prose names a code. The kinds a
  caller outside the crate needs (`Io`, `InputMissing`, `InputIgnored`, `InputEscapes`,
  `RoleConflict`, `OverrideNotPtch`) are part of the library's vocabulary.
- **Negative:** `ErrorKind` grows with every new validation; a consumer matching it
  exhaustively cannot, the enum is `#[non_exhaustive]`.
- **Revisit when:** a consumer needs a location the five fields do not express, such as a
  byte offset inside a source file.

## Pros and cons of the options

### A code and a typed location

- Good: one shape across loading, documents, and application; one translation point.
- Bad: the enum is wide; a caller names a code for every condition it reports.

### Keep the prose pair

- Good: no caller changes.
- Bad: the manager parses strings to know what happened and where.

### One error enum per stage

- Good: each stage's codes are small and exact.
- Bad: three location shapes and three conversions at the seams; the source-reading
  callback and the document parser share conditions and cannot share a type.
