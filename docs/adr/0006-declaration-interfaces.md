# ADR-0006: Declaration interfaces

- **Status:** Accepted
- **Date:** 2026-09-15
- **Crates:** `ltk_game_data`, `ltk_mod_project`, `ltk_overlay`
- **Related:** #190, #225, `game-data.md` [section 3](../design/game-data.md#s3)

## Context and problem statement

Domain vocabulary describes declaration contents. Compiler vocabulary implies executable machinery and validation guarantees that mutable declarations do not provide.

## Decision drivers

- Explicit consumer contracts.
- One interpretation across loading, storage, and execution.

## Considered options

1. **Declaration-oriented interfaces** - explicit domain contracts.
2. **Compiler-oriented interfaces** - fewer changes to existing callers.

## Decision

**Public names describe declaration contents and application operations.** `game-data.md` [section 3](../design/game-data.md#s3) specifies the interface.

## Consequences

- **Positive:** The Rust interface uses domain vocabulary.
- **Negative:** Rust callers require source changes.
- **Revisit when:** Consumers require a separately executable instruction representation.

## Pros and cons of the options

### Declaration-oriented interfaces

- Good: The Rust interface uses domain vocabulary.
- Bad: Rust callers require source changes.

### Compiler-oriented interfaces

- Good: Existing callers retain their interface.
- Bad: Compiler vocabulary implies executable machinery and validation guarantees that mutable declarations do not provide.
