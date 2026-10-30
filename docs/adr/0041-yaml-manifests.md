# ADR-0041: YAML manifests

- **Status:** Accepted
- **Date:** 2026-10-30
- **Crates:** `ltk_game_data`, `ltk_mod_project`, `ltk_overlay`
- **Related:** #314, #316, #317, LTK Manager #966 and #967, ADR-0020, and the spec sections that
  state the rule: `docs/design/game-data.md` [section 3](../design/game-data.md#s3),
  [section 4](../design/game-data.md#s4) and [section 5](../design/game-data.md#s5)

## Context and problem statement

A manifest or source file is YAML, JSON or TOML, named by its extension. Authors write YAML, and
LTK Manager edits YAML and refuses to edit a JSON manifest. `Declarations::manifest_json` writes
the manifest an archive import leaves on disk, and an imported project opens in the manager as a
project it cannot edit.

Three formats carry one schema. A local YAML tag has no JSON or TOML spelling, TOML has no null,
and each format has its own parser, its own duplicate-key behaviour and its own error spans.

## Decision drivers

- A project imported from an archive is a project the manager edits.
- One format for the author, the manager and the writer.
- A project on disk loads through the deprecation, and the manager converts it ahead of the removal.

## Considered options

1. **YAML only, reached in steps.** The writer is YAML, JSON and TOML files warn, and a separate
   breaking release removes both.
2. **Writer switches, formats stay.** `manifest_yaml` replaces `manifest_json`; JSON and TOML
   manifests load indefinitely.
3. **Two writers.** `manifest_yaml` beside `manifest_json`, the caller picks.
4. **Removal in one release.** The loader drops both formats together with the writer change.

## Decision

**YAML is the manifest format, in three steps: the writer is YAML, JSON and TOML files report a
warning on load, and a breaking release of `ltk_game_data` removes both.**

`Declarations::manifest_yaml` is the one manifest writer, and an import writes `game_data.yaml`. A project-directory pack and overlay build report every JSON or
TOML manifest and source file. The manager's conversion of projects on disk ships ahead of the removal.
`docs/design/game-data.md` [section 4](../design/game-data.md#s4) states the accepted formats and
[section 5](../design/game-data.md#s5) the extraction rule. `DeclarationDocument` stays a JSON
value in memory: it is what archive metadata deserializes into, and no author reads it.

## Consequences

- **Positive:** an imported project opens in the manager as an editable project.
- **Positive:** the removal drops `toml` from `ltk_game_data` and one parser path per format.
- **Negative:** the writer change is a breaking change to `ltk_game_data`.
- **Negative:** a reader of a written manifest sees a scalar pin as a `{f32: 1.0}` mapping, not as
  a `!f32 1.0` tag. Both spellings load to the same `Value`.
- **Negative:** TOML comments do not survive the manager's conversion.
- **Revisit when:** an author-facing format other than YAML has a consumer that needs it.

## Pros and cons of the options

### YAML only, reached in steps

- Good: a project loads through every step, and a warning reaches its author ahead of the removal.
- Good: the end state has one format.
- Bad: three releases carry the deprecation, and the manager has to ship before the last.

### Writer switches, formats stay

- Good: no breaking change to the loader.
- Bad: every format-specific rule stays, for files no tool of this project writes.

### Two writers

- Good: no removal.
- Bad: every caller picks, and a JSON caller writes the file the manager refuses.

### Removal in one release

- Good: the shortest path to one format.
- Bad: a project holding a `game_data.json` from an earlier import fails to load on upgrade, with
  no warning and no conversion.
