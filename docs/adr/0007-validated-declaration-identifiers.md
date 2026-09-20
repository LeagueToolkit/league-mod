# ADR-0007: Validated declaration identifiers

- **Status:** Accepted
- **Date:** 2026-09-15
- **Crates:** `ltk_game_data`
- **Related:** #190, #225, `game-data.md` [section 4](../design/game-data.md#s4)

## Context and problem statement

Targets and authored link paths carry local constraints. Plain strings permit invalid values at every call site.

## Decision drivers

- Explicit consumer contracts.
- One interpretation across loading, storage, and execution.

## Considered options

1. **Validated identifier newtypes** - explicit domain contracts.
2. **Strings with execution-time validation** - fewer changes to existing callers.

## Decision

**Identifier construction enforces local validity.** `game-data.md` [section 4](../design/game-data.md#s4) specifies the interface.

## Consequences

- **Positive:** Every constructed identifier satisfies its local constraints.
- **Negative:** Callers handle fallible construction and use explicit string access.
- **Revisit when:** Identifier validity depends on execution context.

## Pros and cons of the options

### Validated identifier newtypes

- Good: Every constructed identifier satisfies its local constraints.
- Bad: Callers handle fallible construction and use explicit string access.

### Strings with execution-time validation

- Good: Existing callers retain their interface.
- Bad: Plain strings permit invalid values at every call site.
