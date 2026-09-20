# ADR-0005: Scalar game-data targets

- **Status:** Accepted
- **Date:** 2026-09-15
- **Crates:** `ltk_game_data`
- **Related:** #190, `docs/design/game-data.md` [section 4](../design/game-data.md#s4)

## Context and problem statement

A declaration identifies one game chunk through a path or its hash. Authors need a compact spelling for either identifier.

## Decision drivers

- Concise declarations across YAML, TOML, and JSON.
- One interpretation across projects, archives, and overlays.

## Considered options

1. **Scalar string** - the spelling determines the identifier kind.
2. **Tagged object** - separate path and hash keys identify the kind explicitly.

## Decision

**Targets use scalar strings.** `game-data.md` [section 4](../design/game-data.md#s4) specifies classification and validation.

## Consequences

- **Positive:** Each target occupies one field without a nested mapping.
- **Negative:** A literal path with a hash-shaped spelling is unrepresentable. Object-form declarations require conversion.
- **Revisit when:** A game chunk requires a literal path with a reserved spelling.

## Pros and cons of the options

### Scalar string

- Good: Short declarations and a shared representation for every format.
- Bad: Identifier classification reserves part of the path namespace.

### Tagged object

- Good: Every literal path is representable without ambiguity.
- Bad: Every declaration carries a nested mapping for one identifier.
