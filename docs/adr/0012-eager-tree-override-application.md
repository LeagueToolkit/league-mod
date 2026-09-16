# ADR-0012: Eager-tree override application

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_data`, `ltk_overlay`
- **Related:** #191, `docs/design/game-data.md` [section 3](../design/game-data.md#s3) and
  [section 6](../design/game-data.md#s6), ADR-0010

## Context and problem statement

The `overrides` binding applies a `PTCH` file's records over a target at the first phase of
a batch. A record replaces one property value inside one object. The link engine rewrites
only the dependency header and copies the object table byte for byte; a record edit reaches
inside that table.

`ltk_meta` 0.8.2 on crates.io reads a `PTCH` as `BinOverride`, lays it over an eager `Bin`
with `BinOverride::apply` in the client's order, reports every skipped record, and writes the
`Bin` back at `PROP` version 3. The streaming surface of the same release mounts a `PROP` and
views objects in place. A patched write-back over the stream, `BinStream::write_patched` with
`BinDelta`, exists on the unreleased `feat/value-walk` branch of `league-toolkit` (PR 227,
open). The workspace pins `ltk_meta` at 0.8.2 and carries no git patch.

A rewritten object table re-encodes every object of the target. A rewritten target's
untouched objects differ from the base byte for byte.

## Decision drivers

- The published crate is enough for the whole binding; no git pin, no release wait.
- The client's own rule for a record that does not fit is a skip, reported, never a failure.
- The output is one plain `PROP` per target; the client never sees a `PTCH`.
- A target with no override keeps the guarantees the link engine gives.

## Considered options

1. **Decode the target to an eager `Bin`, lay each override over it, write the tree back.**
2. **Wait for the streaming patched write-back** on `feat/value-walk`, pinned by git or by a
   release, and splice patched objects into the base bytes.
3. **Splice records into the base bytes in `ltk_game_data`** with a private walk over the
   object table.

## Decision

**A batch with an applied override decodes the target to an eager `Bin`, lays every override
file over it in listed order with `BinOverride::apply`, and the target is written back from
the tree. A target with no applied override keeps its object bytes and header version.**

`docs/design/game-data.md` [section 6](../design/game-data.md#s6) states the rule. A rewritten
target is `PROP` version 3, the version `ltk_meta` writes. Every record `BinOverride::apply`
skips is one `OverrideRecordSkipped` diagnostic carrying the record and a reason code; an
override file that does not read as a `PTCH` is one `OverrideInvalid` diagnostic and the
file is skipped.

## Consequences

- **Positive:** the binding lands on the published `ltk_meta`, with the client's matching
  rules and skip semantics owned by the crate that owns the format.
- **Negative:** a target with an applied override is re-encoded whole. Its untouched objects
  and its header version are not byte-identical to the base; a version-2 base is written at
  version 3. The byte-identity tests hold only for a target without an applied override.
- **Negative:** the eager tree holds every object of the target in memory during
  application; a large map bin costs its decoded size.
- **Revisit when:** `ltk_meta` releases a patched write-back over the stream. The rewrite
  can then re-encode only patched objects and keep the base version.

## Pros and cons of the options

### Decode to an eager `Bin`, lay overrides over it, write the tree back

- Good: one call per override file; the format crate owns matching and skipping.
- Good: no dependency change.
- Bad: whole-table re-encode; version 2 becomes version 3.

### Wait for the streaming patched write-back

- Good: only patched objects are re-encoded; the base version and untouched bytes survive.
- Bad: a git pin on an open branch, or a release with no date; the binding waits on it.
- Bad: two `ltk_meta` sources across the workspace and LTK Manager until the release.

### Splice records into the base bytes privately

- Good: byte-identical untouched objects with no dependency change.
- Bad: a second walk over the object encoding, outside the crate that owns the format, that
  drifts from the client's matching rule as the format changes.
