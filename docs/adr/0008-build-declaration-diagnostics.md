# ADR-0008: Build declaration diagnostics

- **Status:** Accepted
- **Date:** 2026-09-15
- **Crates:** `ltk_overlay`, `ltk_game_data`
- **Related:** #190, #225, `game-data.md` [section 6](../design/game-data.md#s6)

## Context and problem statement

Consumers inspect build results. Separate builder accessors and free-text categories require extra calls and message interpretation.

## Decision drivers

- Explicit consumer contracts.
- One interpretation across loading, storage, and execution.

## Considered options

1. **Typed diagnostics in build results** - explicit domain contracts.
2. **Free-text diagnostics on builders** - fewer changes to existing callers.

## Decision

**Build results own typed declaration diagnostics.** `game-data.md` [section 6](../design/game-data.md#s6) specifies the interface.

## Consequences

- **Positive:** Fresh and cached results expose the same diagnostic interface.
- **Negative:** Consumers handle extensible categories and retain diagnostic data with results.
- **Revisit when:** Diagnostics require streaming before a build completes.

## Pros and cons of the options

### Typed diagnostics in build results

- Good: Fresh and cached results expose the same diagnostic interface.
- Bad: Consumers handle extensible categories and retain diagnostic data with results.

### Free-text diagnostics on builders

- Good: Existing callers retain their interface.
- Bad: Separate builder accessors and free-text categories require extra calls and message interpretation.
