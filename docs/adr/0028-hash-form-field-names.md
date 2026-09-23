# ADR-0028: Hash-form field names

- **Status:** Accepted
- **Date:** 2026-09-23
- **Crates:** `ltk_game_data`
- **Related:** league-toolkit ADR-0005 and ADR-0021, ADR-0017, ADR-0020,
  `docs/design/game-data.md` [section 4](../design/game-data.md#s4) and
  [section 6](../design/game-data.md#s6)

## Context and problem statement

A bin stores field name hashes only. The hashtables name most fields and leave the rest
nameless. A property path names a field by the FNV-1a hash of its text, and a nameless field
has no path.

The game-data reference names a `0x` and 8 hexadecimal digit segment as the hash-form escape
for a field. Entry names, `hash` values, `link` values and a struct pin's `class` read the same
spelling as a hash.

`ltk_meta` resolves and patches a `PropertyPath` the way the client does: the client hashes a
`0x1234abcd` segment as text (league-toolkit `ptch-property-patches.md` section 8.1).
league-toolkit ADR-0005 keeps `PropertyPath` the client's language, and ADR-0021 there gives a
`ValuePath`, addressed by hash, `resolve_at` and `patch_at`.

`Value::render` has no key to write for a nameless struct field, and refuses the whole value.

## Decision drivers

- Every field a bin holds is editable and renderable.
- One reading of `0x` and 8 hexadecimal digits across the standard.
- `ltk_meta` performs every write (ADR-0017).
- A `PropertyPath` keeps the client's reading in `ltk_meta`.

## Considered options

1. **A declaration path lowers to a `ValuePath`.** Each segment name is a field hash, a
   hash-form name spelling the hash itself; `resolve_at` and `patch_at` walk it.
2. **A naming strategy in `ltk_meta`.** `resolve` and `patch` take a rule reading a `0x`
   segment of a `PropertyPath` as its hash.
3. **A synthetic name.** A hash-form segment is rewritten to a name whose FNV-1a hash is the
   hash, and handed to `Bin::patch`.
4. **A walk in this crate.** `ltk_game_data` resolves and writes by hash itself.

## Decision

**A hash-form name in a declaration path names the field with that hash, and a declaration
path resolves and patches as a `ValuePath`.** `docs/design/game-data.md`
[section 4](../design/game-data.md#s4) states the path rule and
[section 6](../design/game-data.md#s6) the resolution. A nameless struct field renders under its
hash-form name.

## Consequences

- **Positive:** every field is addressable by an edit, a `set` key and a reference, and every
  struct renders.
- **Positive:** `ltk_meta` performs the walk and the type rule for both path types.
- **Negative:** a Riot field whose plaintext is `0x` and 8 hexadecimal digits is unreachable by
  its name. No such field is known.
- **Negative:** a bare hash-form key needs quotes in YAML. Unquoted, YAML reads it as an
  integer.
- **Negative:** a `{key}` literal converts against the map the base holds before the walk, one
  extra resolution per subscripted key.
- **Revisit when:** a Riot field name spells a hash form.

## Pros and cons of the options

### A declaration path lowers to a `ValuePath`

- Good: league-toolkit ADR-0005's address type; no second reading of a `PropertyPath`.
- Bad: a conversion step, and an `ltk_meta` release carrying `resolve_at` and `patch_at`.

### A naming strategy in `ltk_meta`

- Good: no conversion; the key literal stays inside the walk.
- Bad: one `PropertyPath` text resolves two ways in `ltk_meta`, against league-toolkit ADR-0005.

### A synthetic name

- Good: no `ltk_meta` change; a 32-bit FNV-1a preimage of six characters is a meet-in-the-middle
  search of a few milliseconds.
- Bad: a name nobody wrote travels through `ltk_meta`; the spelling is correct only for as long
  as the hash function is the one searched against.

### A walk in this crate

- Good: no `ltk_meta` change.
- Bad: a second copy of the traversal and type rules, and writes outside the format crate,
  against ADR-0017.
