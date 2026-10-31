# ADR-0043: A string coerces to an 8-byte hash by XXH3 at an 8-byte property

- **Status:** Accepted
- **Date:** 2026-10-31
- **Crates:** `ltk_game_data`
- **Related:** `docs/design/game-data.md` [section 6](../design/game-data.md#s6), ADR-0030

## Context and problem statement

A bin stores a `hash` value in 4 or 8 bytes. Patch 16.21 stores `StaticMaterialDef.name` in 8
bytes, and the value is the XXH3 of the ASCII-lowercased object path. FNV-1a 32 of a path cannot
be converted to XXH3 of the same path.

ADR-0030 keeps an 8-byte `hash` property as it is on a clone. A clone with such a property holds
the name of its source. A meta database of a build older than the bin answers `string` for
`StaticMaterialDef.name`.

## Decision drivers

- A manifest author writes a path and does not compute a hash.
- A manifest stays valid across a patch that changes the stored form of a property.

## Considered options

1. **Require a spelled hash** - the author writes `0x` and 16 hexadecimal digits for each name.
2. **Read the width from the base value only** - a constructed object and a property that the base omits stay 4-byte.
3. **Add the width to `Shape`, and fall back to the base value** - the schema types a property that the base omits.

## Decision

**A string at a `hash` property of width 8 coerces to the XXH3 of its ASCII-lowercased spelling.**

`Shape::width` states the width. A shape that states no width takes the width of the base value.
A schema answer of `string` over an 8-byte `hash` base value yields to the base value.
`0x` and 8 hexadecimal digits at width 8 are `KindMismatch`. A clone rewrites a top-level 8-byte
`hash` property that holds the XXH3 of the source path to the XXH3 of the new name.

## Consequences

- **Positive:** A material name in a manifest is its path, and a clone holds its own name.
- **Negative:** `Shape` gains a public field, and a consumer that builds a `Shape` literal changes.
- **Negative:** A list item and a map key have no width in `Shape`. A string there reads as a 4-byte hash.
