# ADR-0042: Fantome declaration entry

- **Status:** Accepted
- **Date:** 2026-10-30
- **Crates:** `ltk_fantome`, `ltk_mod_project`, `ltk_overlay`
- **Related:** #315, ADR-0013, ADR-0041, and the spec section that states the rule:
  `docs/design/game-data.md` [section 5](../design/game-data.md#s5)

## Context and problem statement

A `.fantome` stores each layer's declarations as the `GameData` field of its layer metadata in
`META/info.json`. `info.json` is mod metadata that other tools read and extend. Declarations are
the largest value in that file, and an author reads them as nested JSON or not at all.

`META/game_data/<layer>/` holds a layer's override files (ADR-0013), and every reader and extractor
of this repository classifies that directory as build resources. A `.modpkg` stores declarations in
`_meta_/info.msgpack`, a binary format no author reads and no other tool extends.

## Decision drivers

- `info.json` stays metadata.
- An author reads an archive's declarations as the format they write.
- Every existing archive keeps loading and importing.
- An import of a new archive places the manifest where a project holds it.

## Considered options

1. **A manifest entry per layer.** `META/game_data/<layer>/game_data.yaml`, the direct manifest,
   with `GameData` read for a layer with no entry.
2. **A sibling metadata file.** `META/game_data.json`, one document per layer, outside `info.json`.
3. **Keep the field.** `GameData` stays in `info.json`, with YAML as a string value.

## Decision

**A packed Fantome layer stores its declarations as the direct YAML manifest
`META/game_data/<layer>/game_data.yaml`, and `info.json` carries none. A reader takes the entry
first and reads `GameData` for a layer with no entry.**

`docs/design/game-data.md` [section 5](../design/game-data.md#s5) states the entry and the reading
order. An import extracts the entry with the layer's other `META/game_data/` entries, and writes a
manifest only for a layer whose declarations are in `GameData` alone.

## Consequences

- **Positive:** an import is placement: the entry lands as the project's manifest, with the
  override files it names beside it.
- **Positive:** `unzip` shows an author the declarations of any archive.
- **Negative:** a reader without this change finds no `GameData` in a new archive and applies no
  declarations, with no error. The release notes state this.
- **Negative:** a module read from the entry reports no origin source. A diagnostic on such a
  module names the manifest and the module index, not the authored source file.
- **Negative:** `GameData` stays in `FantomeLayerInfo` as long as archives carrying it are read.
- **Revisit when:** a container format gains a resource table that names build inputs by role.

## Pros and cons of the options

### A manifest entry per layer

- Good: the entry is the file an author writes, under the directory that holds the layer's other
  build resources.
- Good: no new classification and no new extraction path.
- Bad: origin sources do not survive, the direct manifest having no place for them.

### A sibling metadata file

- Good: origins survive, the document form carrying them.
- Bad: an author still reads nested JSON, and an import still has to write a manifest.

### Keep the field

- Good: no compatibility break for an older reader.
- Bad: `info.json` still holds declarations, and YAML inside a JSON string reads worse than either.
