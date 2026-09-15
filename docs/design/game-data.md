# Game data declarations

## <a id="s1"></a>1. Summary

Game-data declarations travel from a mod project's layer manifest through archives to the
overlay builder. The binding syntax follows the [game-data reference](https://wiki.leaguetoolkit.dev/reference/mod-packages/game-data/).
The supported bindings are `links`, `+links`, and `-links`. Other bindings are errors.

## <a id="s2"></a>2. Vocabulary

- **Program:** The versioned, ordered modules belonging to one layer.
- **Module:** An explicit chunk target and ordered batches with diagnostic provenance.
- **Batch:** Link removals followed by additions.
- **Origin:** The manifest, source, module index, and step index identifying authored work.
- **Build resource:** A manifest or referenced source excluded from game content.

## <a id="s3"></a>3. Crate boundary

`ltk_game_data` owns the executable program, authoring parser, and bin materialisation
([ADR-0004](../adr/0004-game-data-engine.md)). The project crate resolves layer-relative
files and enforces `.modignore`. Archive crates carry programs as layer metadata. Providers
expose the same program for directories and archives. The overlay owns target resolution,
mod precedence, WAD distribution, and persisted diagnostics.

## <a id="s4"></a>4. Authoring

A layer has at most one `game_data.yaml`, `game_data.yml`, `game_data.toml`, or
`game_data.json`. The manifest requires integer `version: 1` and a `modules` array.
A module contains one `target` and a compact binding body, `steps`, or `source`.
A target is one nonempty string. Exactly 16 ASCII hexadecimal characters identify a chunk
hash without a prefix; every other spelling identifies a literal path. Hash-shaped spellings
are reserved and cannot identify literal paths. Objects and non-string values are errors.
Numeric-looking YAML targets require quotes. Paths hash with ASCII lowercase and XXH64 seed
zero. Extensionless paths are valid. Suffixes, separators, and whitespace remain literal.
The Rust `Target` type privately distinguishes paths and hashes. `Target::from(String)`
classifies the spelling; callers cannot construct a conflicting identifier kind.

Source files require their own version and a compact body or steps. Sources have no target
or recursive includes. Source paths remain within the layer through symlink resolution.
Duplicate target/source assignments, duplicate mapping keys, unknown keys, mixed bodies,
unsupported versions, missing inputs, and ignored required inputs are errors.
`links` and `+links` are aliases; a batch cannot contain both.

## <a id="s5"></a>5. Containers

Packing expands sources into the ordered program. Manifests and sources are excluded from
ordinary content, including sources inside WAD directories. Programs retain diagnostic origins.
Archive programs use the target strings specified in [section 4](#s4).
Extraction reconstructs a direct `game_data.json` manifest per layer.
The modpkg layer metadata field is `game_data`; the Fantome layer field is `GameData`.
Modpkg metadata uses schema version 4. Absent fields represent no declarations.
The executable program has its own version. `Document` retains unsupported program fields;
its `program()` method validates the complete layer before execution.

## <a id="s6"></a>6. Overlay

Modules execute from lowest to highest mod precedence, ascending layer priority with name
as a tie-breaker, module order, and step order. The highest-precedence enabled mod copy is
the target base; the game supplies a base absent from mod content.
The target must be PROP version 2 or 3. Invalid declarations refuse the layer's program;
ordinary content remains available. Missing or invalid targets produce reports and retain
their original bytes. Removals compare ASCII-lowercased paths; missing removals produce reports.
Additions retain written casing and order and omit case-insensitive duplicates.
`ltk_meta` validates the complete base. Materialisation replaces the dependency header;
untouched object bytes and the PROP version remain identical.

Reports identify the mod, layer, target, and origin. Reports persist in `overlay.json` and
survive cache reuse. Program changes participate in build invalidation. Declared dependencies
participate in linked-bin validation.

Declared targets are materialised before WAD distribution. Their final bytes remain shared
until compression. This memory cost scales with declared targets. Directory declarations
disable provider metadata caching and exact-match skipping; final content hashes permit
unchanged WAD reuse. Archive providers retain fingerprint-based exact-match skipping.
Base selection reads unfiltered mod metadata; game-identical copies remain candidates.
Declaration diagnostics use `GameDataReport`; missing final dependencies use
`LinkedBinOffender`. Overlay state uses schema version 7.

## <a id="s7"></a>7. Validation

Public seams are declaration loading and materialisation, project/archive round trips, and
overlay builds. Cases cover ordering, input classification, invalid declarations, target
selection, enabled layers, link reports, and cached builds.

## <a id="s8"></a>8. Rules

| ID | Rule | Instead of | Why | Spec |
| --- | --- | --- | --- | --- |
| D1 | A shared crate owns executable declarations | Container-specific engines | Consumers share one interpretation | ADR-0004 |
| D2 | Unsupported bindings are errors | Silent omission | Partial declarations misrepresent author intent | [section 4](#s4) |
| D3 | Targets use scalar strings | Tagged path/hash objects | Compact identifiers | [ADR-0005](../adr/0005-scalar-game-data-targets.md) |
