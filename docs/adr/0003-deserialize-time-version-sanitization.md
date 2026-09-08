# ADR-0003: Deserialize-time version sanitization

- **Status:** Accepted
- **Date:** 2026-09-08
- **Crates:** `ltk_fantome`
- **Related:** #193, LeagueToolkit/ltk-manager#321

## Context and problem statement

The `Version` field of a Fantome `META/info.json` is hand written, and a decade of tools write it.
The corpus holds JSON numbers (`"Version": 1.1`, `"Version": 1.3333333`), strings with more than
three segments (`"Version": "1.2.3.1615.2"`), `v` prefixes, leading zeros, and prose.

`FantomeInfo::version` is a `String`. A JSON number fails the whole parse with
`invalid type: floating point 1.1, expected a string`, and the archive is unreadable. The
RuneForge mod "Sausage dog Naafiri" carries such a field and fails the LTK Manager install flow.

A string that parses carries the failure further in. `ltk_mod_project`'s modpkg conversion parses
the field with `semver::Version::parse`. A four-segment tag is rejected at pack time.

Every consumer of a Fantome archive meets both failures.

## Decision drivers

- One repair, at the format boundary, for every consumer of the crate.
- A version field that reaches a caller is valid semver.
- An archive is readable whatever its `Version` field holds.
- The written spelling of a valid tag survives a round trip.

## Considered options

1. **Consumer-side pre-parse** - each consumer rewrites the JSON text before handing it to serde.
2. **Sanitizing deserializer** - `ltk_fantome` reads the field through a visitor that accepts any
   JSON type and yields a semver string.
3. **Typed field** - `FantomeInfo::version` holds a `semver::Version`.

## Decision

**`FantomeInfo::version` deserializes through a sanitizing visitor that never fails.**

The field keeps its `String` type and its serialized spelling. The visitor accepts a value of any
JSON type. A tag that parses as semver is kept verbatim; any other tag is rebuilt into one, and a
value holding no version yields `1.0.0`. The rebuild rules are documented on
`ltk_fantome`'s `version` module.

A number's fractional part is a fraction, not a segment. It is cut to its first digit, which is
the minor version the number names.

`"Version": 1.1` reads as `1.1.0`. `"Version": 1.3333333` reads as `1.3.0`.
`"Version": "1.2.3.1615.2"` reads as `1.2.3`. `"Version": "no idea"` reads as `1.0.0`.

A `Version` key absent from the object yields `1.0.0`. The field is the one optional key of the
four an info.json is written with.

## Consequences

- **Positive:** an archive with any `Version` field parses. The value a caller reads parses as
  semver, and the modpkg conversion accepts it. A consumer of the crate needs no repair of its
  own.
- **Negative:** the crate accepts metadata the format does not describe, and the original text of
  a mangled tag is lost to the caller. A `Version` field of the wrong JSON type is silently
  replaced in place of reported.
- **Revisit when:** a caller needs the tag as it was written, or the corpus grows a case the
  rebuild maps to a misleading version in place of `1.0.0`.

## Pros and cons of the options

### Consumer-side pre-parse

- Good: `ltk_fantome` stays a literal reader of the format.
- Bad: every consumer carries the same regex, and each one drifts from the others. The
  four-segment case is repaired nowhere.

### Sanitizing deserializer

- Good: one rule, one place, and every consumer of the crate inherits it.
- Bad: the crate reads a value the format forbids, and the repair is invisible to the caller.

### Typed field

- Good: the invariant is in the type, and no caller reparses.
- Bad: a breaking change to a public struct. `FantomeWriter` and the archive rewrite reserialize
  metadata, and a typed field forces a spelling on archives written by other tools.
