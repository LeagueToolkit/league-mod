# ADR-0011: Insertion-ordered declaration documents

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_data`
- **Related:** #233, `docs/design/game-data.md` [section 5](../design/game-data.md#s5)

## Context and problem statement

An `entries` selector is a mapping of entry names in authored order, and the overlay edits the
entries in that order. A `DeclarationDocument` is a `serde_json::Value` carried by modpkg and
Fantome layer metadata. `serde_json::Map` and `toml`'s table type sort keys unless their
`preserve_order` feature is on, and a feature is unified across the workspace.

## Decision drivers

- Mapping order survives packing, extraction, and the direct JSON manifest.
- The manifest syntax stays the wiki's.
- The smallest change to a document type every archive crate already carries.

## Considered options

1. **Enable `preserve_order`** on `serde_json` and `toml` in `ltk_game_data`.
2. **Replace the `Value` document** with `Declarations` plus a retained side table of unknown
   fields.
3. **Serialize `entries` as an array of pairs**, so no map ordering is at stake.

## Decision

**`ltk_game_data` enables `preserve_order` on `serde_json` and `toml`. Every JSON object and
TOML table in the workspace keeps insertion order.**

## Consequences

- **Positive:** `entries` order holds through every container and the JSON manifest, with no
  new type.
- **Negative:** every workspace crate's `serde_json::Map` is an `IndexMap`; JSON output that was
  key-sorted is now insertion-ordered. A consumer that relied on sorted output sorts itself.
- **Revisit when:** a consumer cannot take `preserve_order`, or the document type is replaced.

## Pros and cons of the options

### Enable `preserve_order`

- Good: one line per dependency; the common configuration of both crates.
- Bad: workspace-wide feature unification.

### Replace the `Value` document

- Good: ordering independent of a feature flag.
- Bad: a rewrite of a type three crates carry, for a guarantee the flag gives.

### Serialize `entries` as pairs

- Good: no map ordering at stake.
- Bad: a manifest syntax the wiki does not have.
