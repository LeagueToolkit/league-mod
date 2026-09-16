# Game data declarations

## <a id="s1"></a>1. Summary

Game-data declarations travel from a mod project's layer manifest through archives to the
overlay builder. The binding syntax follows the [game-data reference](https://wiki.leaguetoolkit.dev/reference/mod-packages/game-data/).
The supported bindings are `links`, `+links`, and `-links`. Other bindings are errors.

## <a id="s2"></a>2. Vocabulary

- **Declarations:** The versioned, ordered modules belonging to one layer.
- **Declaration document:** Preserved serialized declarations, including unsupported fields.
- **Module:** One selector and its steps with diagnostic provenance.
- **Selector:** What a module edits: a chunk target with steps, or a mapping of entry names to steps.
- **Step:** Link removals followed by additions.
- **Declaration location:** A manifest, optional source, and zero-based module index.
- **Target:** A nonempty literal game path or bare chunk hash.
- **Entry name:** A nonempty bin object path. Its object hash is the FNV-1a of its ASCII-lowercased spelling.
- **Declaring chunk:** A game bin chunk containing an entry, reported by the object index.
- **Link path:** An authored dependency path containing 1 to 65535 UTF-8 bytes.
- **Diagnostic:** One nonfatal declaration or application outcome, with a typed category.
- **Build resource:** A manifest or referenced source excluded from game content.

## <a id="s3"></a>3. Crate boundary

`ltk_game_data` owns declaration types, loading, and BIN application
([ADR-0004](../adr/0004-game-data-engine.md), [ADR-0006](../adr/0006-declaration-interfaces.md)).
The project crate resolves layer-relative files and enforces `.modignore`. Archive crates
carry declaration documents as layer metadata. Providers expose the same declarations for
directories and archives. The overlay owns selector resolution through `ltk_game_index`
(`game-index.md` [section 10](game-index.md#s10)), mod precedence, WAD distribution, and
persisted diagnostics. `ltk_game_data` has no dependency on `ltk_game_index`.

The library owns standard binding semantics. Binding implementations are private. The public
substitution seams are `ModContentProvider` and the source-reading callback accepted by
`load_declarations()`. Consumer-defined binding registries are outside the interface.

The core interface is:

```rust
pub fn load_declarations(
    manifest_name: &str,
    text: &str,
    read_source: impl FnMut(&str) -> Result<String, Error>,
) -> Result<Declarations, Error>;

impl DeclarationDocument {
    pub fn parse(&self) -> Result<Declarations, Error>;
}

pub fn apply(base: &[u8], steps: &[Step]) -> Result<ApplyResult, Error>;

pub struct ApplyResult {
    pub bytes: Vec<u8>,
    pub dependencies: Vec<String>,
    pub diagnostics: Vec<ApplyDiagnostic>,
}
```

`load_declarations()` expands source files and validates the declarations. `parse()` interprets
a contained document and validates the result. `Declarations` exposes mutable `version` and
`modules` fields; `validate()` checks supported declaration versions. `manifest_json()` validates
and writes a direct JSON authoring manifest. Target and link-path validity is enforced by their
types ([section 4](#s4)). `apply()` returns bytes and leaves its input unchanged.
`dependencies` contains the resulting BIN dependency spellings, including retained base entries.

## <a id="s4"></a>4. Authoring

A layer has at most one `game_data.yaml`, `game_data.yml`, `game_data.toml`, or
`game_data.json`. The manifest requires integer `version: 1` and a `modules` array.
A module contains one selector. A `target` selector takes a compact binding body, `steps`, or
`source`. An `entries` selector is a mapping of entry names to compact binding bodies or `steps`,
and takes nothing else. A module with both keys, or neither, is an error.
A target is one nonempty string. Exactly 16 ASCII hexadecimal characters identify a chunk
hash without a prefix; every other spelling identifies a literal path. Hash-shaped spellings
are reserved and cannot identify literal paths. Objects and non-string values are errors.
Numeric-looking YAML targets require quotes. Paths hash with ASCII lowercase and XXH64 seed
zero. Extensionless paths are valid. Suffixes, separators, and whitespace remain literal.

`Target` privately distinguishes paths and hashes. `TryFrom<String>` and `TryFrom<&str>`
classify the spelling and reject empty strings. `chunk_hash()` returns an infallible `u64`;
`as_str()` returns the authored spelling. `LinkPath` has the same construction and string-access
traits and enforces its UTF-8 byte-length limit. Serde deserialization enforces these invariants
([ADR-0007](../adr/0007-validated-declaration-identifiers.md)).

An entry name is one nonempty string. `EntryName` has the same construction and string-access
traits as `Target`; `object_hash()` returns its `BinHash`.

`Module` contains `selector: Selector` and `location: DeclarationLocation`.
`Selector::Target { target: Target, steps: Vec<Step> }` edits one chunk.
`Selector::Entries(IndexMap<EntryName, Vec<Step>>)` edits each named entry in every declaring
chunk, in mapping order.
`Step` contains `add_links: Vec<LinkPath>` and `remove_links: Vec<LinkPath>`.
`DeclarationLocation` contains `manifest`, optional `source`, and `module_index`.

Source files require their own version and a compact body or steps. Sources have no target
or recursive includes. Source paths remain within the layer through symlink resolution.
Duplicate target/source assignments, duplicate mapping keys, unknown keys, mixed bodies,
unsupported versions, missing inputs, and ignored required inputs are errors.
`links` and `+links` are aliases; a step cannot contain both.

`ltk_mod_project::game_data::load_layer()` returns `LayerDeclarations`. Its `declarations`
field is `Result<Option<Declarations>, Error>`; `is_declaration_input(path)` identifies build
resources, including inputs discovered in rejected declarations.

## <a id="s5"></a>5. Containers

Packing expands sources into ordered declarations. Manifests and sources are excluded from
ordinary content, including sources inside WAD directories. Declarations retain their locations.
Archive declarations use the target strings specified in [section 4](#s4).
Extraction reconstructs a direct `game_data.json` manifest per layer.
The modpkg layer metadata field is `game_data`; the Fantome layer field is `GameData`.
Modpkg metadata uses schema version 4. Absent fields represent no declarations.
`DeclarationDocument` retains unsupported fields; its `parse()` method validates the complete
layer before execution. Declaration version 1 identifies the supported format.

Rust names and serialized names have the following mapping:

| Rust field | Serialized field |
| --- | --- |
| `Step::add_links` | `links` (`+links` accepted on input) |
| `Step::remove_links` | `-links` |
| `Selector::Target` | `target` and `steps` |
| `Selector::Entries` | `entries` |
| `Module::location` | `origin` |
| `DeclarationLocation::module_index` | `module` |
| Diagnostic `location` | `origin` |
| Diagnostic `step_index` | `step` |
| `OverlayState::game_data_diagnostics` | `gameDataReports` |

## <a id="s6"></a>6. Overlay

`ModContentProvider::game_data_declarations(layer)` returns `Result<Option<Declarations>, Error>`.
Its default is `Ok(None)`. Filesystem, modpkg, and Fantome providers load declarations.

Modules execute from lowest to highest mod precedence, ascending layer priority with name
as a tie-breaker, module order, and step order. The highest-precedence enabled mod copy is
the target base; the game supplies a base absent from mod content. The game copy is the first
holder in `ltk_game_index` archive order.

An `entries` selector lowers to one chunk application per declaring chunk of each entry,
in mapping order, before base selection. `ObjectIndex::declarations` supplies the declaring
chunks. An entry with several declaring chunks is edited in every one; an `EntryFanOut`
diagnostic names them. An entry with no declaring chunk produces `EntryUnresolved` and its
steps are skipped. The overlay loads or builds the object index only for a build in which an
enabled layer declares an `entries` module, from `object_index.bin` beside `game_index.bin`,
under the `IndexingObjects` build stage. An object index that fails to load and build produces
`IndexUnavailable` for every `entries` module and a build warning; the build continues.
The target must be PROP version 2 or 3. Invalid declarations refuse the layer's declarations;
ordinary content remains available. Missing or invalid targets produce diagnostics and retain
their original bytes. Removals compare ASCII-lowercased paths; missing removals produce diagnostics.
Additions retain written casing and order and omit case-insensitive duplicates.
`ltk_meta` validates the complete base. Application replaces the dependency header;
untouched object bytes and the PROP version remain identical.

`OverlayBuildResult::game_data_diagnostics` contains `GameDataDiagnostic` values from the build,
including cached builds ([ADR-0008](../adr/0008-build-declaration-diagnostics.md)). Each diagnostic
contains `kind`, `mod_id`, `layer`, optional `target`, optional `location`, optional `step_index`,
and a human-readable `message`. `GameDataDiagnosticKind` is non-exhaustive and distinguishes
`DeclarationsRejected`, `TargetSkipped`, `EntryUnresolved`, `EntryFanOut`, `IndexUnavailable`,
`LinkRemovalUnmatched`, and `Unknown`. `EntryFanOut` is informational.
`ApplyDiagnostic` contains `kind`, `step_index`, and `path`; its non-exhaustive
`ApplyDiagnosticKind` distinguishes `LinkRemovalUnmatched` and `Unknown`.

Diagnostic kinds serialize in camelCase. Missing or unrecognized serialized kinds decode to
`Unknown`; existing messages and locations remain available. Fresh diagnostics carry explicit
kinds. Diagnostics persist in `overlay.json` and survive cache reuse. Declaration changes
participate in build invalidation. Declared dependencies participate in linked-bin validation.

Declared targets are applied before WAD distribution. Their final bytes remain shared
until compression. This memory cost scales with declared targets. Directory declarations
disable provider metadata caching and exact-match skipping; final content hashes permit
unchanged WAD reuse. Archive providers retain fingerprint-based exact-match skipping.
Base selection reads unfiltered mod metadata; game-identical copies remain candidates.
Missing final dependencies use `LinkedBinOffender`. Overlay state uses schema version 7.

## <a id="s7"></a>7. Validation

Public seams are declaration loading and application, project/archive round trips, and
overlay builds. Cases cover ordering, input classification, invalid declarations, identifier
construction, serialized field compatibility, target selection, enabled layers, typed
diagnostics, and cached builds with missing or unknown diagnostic kinds.

## <a id="s8"></a>8. Rules

| ID | Rule | Instead of | Why | Spec |
| --- | --- | --- | --- | --- |
| D1 | A shared crate owns executable declarations | Container-specific engines | Consumers share one interpretation | ADR-0004 |
| D2 | Unsupported bindings are errors | Silent omission | Partial declarations misrepresent author intent | [section 4](#s4) |
| D3 | Targets use scalar strings | Tagged path/hash objects | Compact identifiers | [ADR-0005](../adr/0005-scalar-game-data-targets.md) |
| D4 | Rust names describe declarations | Compiler vocabulary | Types describe contents | [ADR-0006](../adr/0006-declaration-interfaces.md) |
| D5 | Identifier construction enforces local validity | Execution-time string checks | Invalid identifiers are unconstructible | [ADR-0007](../adr/0007-validated-declaration-identifiers.md) |
| D6 | Build results contain typed diagnostics | Separate free-text builder reports | Consumers inspect one result | [ADR-0008](../adr/0008-build-declaration-diagnostics.md) |
| D7 | A module has one selector: `target` or `entries` | Entry names inside `target` | An entry name and a chunk path share a spelling | [section 4](#s4) |
| D8 | The overlay resolves selectors through `ltk_game_index` | Index access in `ltk_game_data` | The library applies edits without an installation | [section 3](#s3), [ADR-0009](../adr/0009-game-index-crate.md) |
| D9 | Every declaring chunk of an entry is edited | First declaring chunk only | The client loads copies by load order | [section 6](#s6) |
| D10 | The object index is built only for a build with an `entries` module | Every build | A full bin read is paid only when used | [section 6](#s6) |
