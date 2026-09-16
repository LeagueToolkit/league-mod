# Game data declarations

## <a id="s1"></a>1. Summary

Game-data declarations travel from a mod project's layer manifest through archives to the
overlay builder. The binding syntax follows the [game-data reference](https://wiki.leaguetoolkit.dev/reference/mod-packages/game-data/).
The supported bindings are `overrides`, `links`, `+links`, and `-links`. Other bindings
are errors.

## <a id="s2"></a>2. Vocabulary

- **Declarations:** The versioned, ordered modules belonging to one layer.
- **Declaration document:** Preserved serialized declarations, including unsupported fields.
- **Module:** One selector with its edits and its origin.
- **Selector:** What a module edits: a chunk target with edits, or a mapping of entry names to entry edits.
- **Edit:** One batch of bindings on a chunk, applied phase by phase. The phases are override files, then link removals followed by additions.
- **Override file:** A `.ptch` file in a layer holding `PTCH` records applied over a target.
- **Override path:** The layer-relative, forward-slash path of an override file.
- **Entry edit:** The bindings of one entry, applied in every chunk declaring it.
- **Origin:** A manifest, optional source, and zero-based module index.
- **Target:** A nonempty literal game path or bare chunk hash.
- **Entry name:** A nonempty bin object path, or its hash as `0x` and 8 hexadecimal digits. The object hash of a path is the FNV-1a of its ASCII-lowercased spelling.
- **Declaring chunk:** A game bin chunk containing an entry, reported by the object index.
- **Link path:** An authored dependency path containing 1 to 65535 UTF-8 bytes.
- **Diagnostic:** One nonfatal declaration or application outcome, with a typed category.
- **Build resource:** A manifest, referenced source, or override file excluded from game content.

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

pub fn apply<B: AsRef<[u8]>>(
    base: &[u8],
    edits: &[Edit],
    read_override: impl FnMut(&OverridePath) -> Result<B, Error>,
) -> Result<ApplyResult, Error>;

pub struct ApplyResult {
    pub bytes: Vec<u8>,
    pub dependencies: Vec<String>,
    pub diagnostics: Vec<ApplyDiagnostic>,
}
```

`Error` is a code with a typed place ([ADR-0014](../adr/0014-coded-declaration-errors.md)):

```rust
pub struct Error {
    pub kind: ErrorKind,
    pub location: Box<Location>,
}

#[non_exhaustive]
pub enum ErrorKind { /* one code per condition; `Syntax` and `Io` carry the source's statement as `detail` */ }

pub struct Location {
    pub document: Option<String>,
    pub module: Option<usize>,
    pub edit: Option<usize>,
    pub entry: Option<String>,
    pub key: Option<String>,
    pub span: Option<Span>,
}

pub struct Span { pub start: usize, pub end: usize }
```

`Error::new(kind)` has no place; `Error::at_key(kind, key)` and `Error::in_document(kind, name)`
set one part; `Error::io(name, error)` is the `Io` code at a document. The methods `document`,
`module`, `edit`, `entry`, `key`, and `span` fill an unset part and leave a set one alone; an
error raised at an inner site keeps its own parts and gains its outer context. A `Syntax` error
from a parser carries the byte `span` of the reported position; a `Location` with every part
unset displays as the code's statement alone. `Display` is a log rendering; a consumer matches
`kind` and navigates by `location`.

`load_declarations()` expands source files and validates the declarations. `parse()` interprets
a contained document and validates the result. `Declarations` exposes mutable `version` and
`modules` fields; `validate()` checks supported declaration versions. `manifest_json()` validates
and writes a direct JSON manifest. Target and link-path validity is enforced by their
types ([section 4](#s4)). `apply()` runs each edit's phases in field order, each edit over the
result of the preceding one, returns bytes, and leaves its input unchanged. `read_override`
supplies the bytes of an override file by its path in any `AsRef<[u8]>` container; it is
called once per listed path, in apply order. `dependencies` contains the resulting BIN dependency spellings, including
retained base entries.

## <a id="s4"></a>4. Authoring

A layer has at most one `game_data.yaml`, `game_data.yml`, `game_data.toml`, or
`game_data.json`. The manifest requires integer `version: 1` and a `modules` array.
A module contains one selector. A `target` selector takes a compact binding body, `edits`, or
`source`. An `entries` selector is a mapping of entry names to compact binding bodies and takes
nothing else; an `entries` module is one batch. A module with both keys, or neither, is an
error. A compact body and every edit carry at least one binding.
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

`overrides` lists override paths. An authored path is relative to the source file naming it,
or to the layer directory in a manifest body. `load_declarations()` resolves it against that
file lexically; a loaded declaration and a declaration document carry the layer-relative
spelling ([ADR-0013](../adr/0013-override-file-placement.md)). `OverridePath` has the same
construction and string-access traits as `LinkPath` and implements `Display`. It is nonempty,
relative, has no backslash, no empty, `.`, or `..` segment, and a nonempty file stem ending
in `.ptch`, compared ASCII case-insensitively. A `.rito` path is an error naming the unsupported extension. A path that
leaves the layer is a loading error.

An entry name is one nonempty string. `0x` followed by exactly 8 ASCII hexadecimal digits
identifies an object hash; every other spelling identifies an object path. `EntryName` has the
same construction and string-access traits as `Target`, implements `Display`, and
`object_hash()` returns its `BinHash`.

`Module` contains `selector: Selector` and `origin: Origin`
([ADR-0010](../adr/0010-phased-declaration-edits.md)).
`Selector` is non-exhaustive.
`Selector::Target { target: Target, edits: Vec<Edit> }` edits one chunk.
`Selector::Entries(IndexMap<EntryName, EntryEdit>)` edits each named entry in every declaring
chunk, in mapping order.
`Edit` is one batch. It is non-exhaustive, implements `Default`, and its fields are its phases
in apply order: `overrides: Vec<OverridePath>`, then `links: LinkEdit`. `EntryEdit` is the
bindings of one entry, non-exhaustive with `Default`, with the field `links: LinkEdit` as an
extension of the game-data reference; `overrides` in an entry body is an error. `LinkEdit`
contains `add: Vec<LinkPath>` and `remove: Vec<LinkPath>`. `Origin` contains `manifest`,
optional `source`, and `module_index`.

Source files require their own version and a compact body or `edits`. Sources have no target
or recursive includes. Source paths remain within the layer through symlink resolution.
Duplicate target/source assignments, duplicate mapping keys, unknown keys, mixed bodies,
unsupported versions, missing inputs, and ignored required inputs are errors.
`links` and `+links` are aliases; a body cannot contain both.

`ReferencedInputs::discover(name, text)` reports the `sources` and authored `overrides` a
manifest or source document references, independently of binding validation.
`ltk_mod_project::game_data::load_layer()` returns `LayerDeclarations`. Its `declarations`
field is `Result<Option<Declarations>, Error>`; `is_declaration_input(path)` identifies build
resources, including inputs discovered in rejected declarations. `override_files()` lists
the override files of accepted declarations as `OverrideFile` values, each with its `path`
and its `source` file, one per distinct path in first-reference order. Loading reads every
override file; a missing, ignored, or escaping file, or one that does not read as a `PTCH`,
refuses the layer's declarations.

## <a id="s5"></a>5. Containers

Packing expands sources into ordered declarations. Manifests, sources, and override files
are excluded from ordinary content, including inputs inside WAD directories. Declarations
retain their origins. Archive declarations use the target strings specified in
[section 4](#s4). An override file travels under its layer-relative path
([ADR-0013](../adr/0013-override-file-placement.md)): modpkg stores it as a chunk of its layer
with no WAD; Fantome stores it as `META/game_data/<layer>/<path>`, classified as
`FantomeEntry::GameData`. Extraction reconstructs a direct `game_data.json` manifest per
layer and places every override file at its path under the layer's content directory.
The modpkg layer metadata field is `game_data`; the Fantome layer field is `GameData`.
Modpkg metadata uses schema version 4. Absent fields represent no declarations.
`DeclarationDocument` retains unsupported fields; its `parse()` method validates the complete
layer before execution. A document and a manifest carry an `entries` mapping in authored
order ([ADR-0011](../adr/0011-insertion-ordered-declaration-documents.md)). An edit with no
binding is a legal document value; `manifest_json()` writes `links` for every edit and
`overrides` for an edit with override paths.
Declaration version 1 identifies the supported format.

Rust names and serialized names have the following mapping:

| Rust field | Serialized field |
| --- | --- |
| `Edit::overrides` | `overrides` |
| `LinkEdit::add` | `links` (`+links` accepted on input) |
| `LinkEdit::remove` | `-links` |
| `Selector::Target` | `target` and `edits`, one compact body per edit |
| `Selector::Entries` | `entries`, a mapping of entry name to one compact body |
| `Origin::module_index` | `module` |
| Diagnostic `edit_index` | `edit` |
| `OverlayState::game_data_diagnostics` | `gameDataReports` |

## <a id="s6"></a>6. Overlay

`ModContentProvider::game_data_declarations(layer)` returns `Result<Option<Declarations>, Error>`.
Its default is `Ok(None)`. `ModContentProvider::read_game_data_resource(layer, path)` returns
the bytes of the layer's override file at a layer-relative path; its default is
`Err(ModContentError::GameDataResourceUnsupported)`, and a provider that carries override
files answers a path it does not hold with `ModContentError::GameDataResourceMissing`.
Filesystem, modpkg, and Fantome providers load declarations and read override files.

Modules execute from lowest to highest mod precedence, ascending layer priority with name
as a tie-breaker, module order, and edit order. The highest-precedence enabled mod copy is
the target base; the game supplies a base absent from mod content. The game copy is the first
holder in `ltk_game_index` archive order.

An `entries` selector lowers to one chunk application per declaring chunk of each entry,
in mapping order, before base selection. `ObjectIndex::declarations` supplies the declaring
chunks. An entry with several declaring chunks is edited in every one; an `EntryFanOut`
diagnostic names them. An entry with no declaring chunk produces `EntryUnresolved` and its
edits are skipped. A diagnostic of an entry carries the entry name as its `target`; a
lowered application names its chunk by hex hash and carries it in `chunk`. The overlay loads or builds the object index only for a build in which an
enabled layer declares an `entries` module, from `object_index.bin` beside `game_index.bin`,
under the `IndexingObjects` build stage. An object index that fails to load and build produces
`IndexUnavailable` for every `entries` module and a warning in the log; the build continues.
An object index build the cancellation poll stops ends the build.
The target must be PROP version 2 or 3. Invalid declarations refuse the layer's declarations;
ordinary content remains available. Missing or invalid targets produce diagnostics and retain
their original bytes. `ltk_meta` decodes the complete base.

Each edit applies its override files in listed order before its link edits. An override file
the provider cannot supply produces `OverrideUnreadable`; one that does not read as a `PTCH`
produces `OverrideInvalid`; either file is skipped. `ltk_meta` lays an override over the
target in the client's order: deletions, added objects, then records in file order. A record
that does not apply produces `OverrideRecordSkipped` carrying a `SkippedRecord`: the record
index, object hash, property path, and a `RecordSkipReason` code; the remaining records
continue. Records apply as authored, without coercion. A target with an applied
override file is written from the decoded tree at PROP version 3
([ADR-0012](../adr/0012-eager-tree-override-application.md)). A target with no applied
override file keeps its object bytes and PROP version; application replaces only the
dependency header.

Removals compare ASCII-lowercased paths; missing removals produce diagnostics. Additions
retain written casing and order and omit case-insensitive duplicates. The overlay reads each
override file once per build.

`OverlayBuildResult::game_data_diagnostics` contains `GameDataDiagnostic` values from the build,
including cached builds ([ADR-0008](../adr/0008-build-declaration-diagnostics.md)). Each diagnostic
contains `kind`, `mod_id`, `layer`, optional `target`, optional `chunk`, optional `origin`,
optional `edit_index`, and a human-readable `message`. `chunk` is the chunk the diagnostic is
about, absent from a module-level diagnostic; it serializes as the `WadHash` number and decodes
absent as `None`. `GameDataDiagnosticKind` is non-exhaustive and distinguishes
`DeclarationsRejected`, `TargetSkipped`, `EntryUnresolved`, `EntryFanOut`, `IndexUnavailable`,
`OverrideUnreadable`, `OverrideInvalid`, `OverrideRecordSkipped`, `LinkRemovalUnmatched`, and
`Unknown`. `EntryFanOut` is informational. A `GameDataDiagnostic` of kind
`OverrideRecordSkipped` carries the `SkippedRecord` in its optional `record` field, absent
otherwise and decoding absent as `None`. `ApplyDiagnostic` contains `kind`, `edit_index`,
`path`, and optional `record`; `path` is the link path or the override path the diagnostic is
about. Its non-exhaustive `ApplyDiagnosticKind` distinguishes `OverrideUnreadable`,
`OverrideInvalid`, `OverrideRecordSkipped`, `LinkRemovalUnmatched`, and `Unknown`.
`SkippedRecord` contains `index`, `object` (a `BinHash`), `property`, and `reason`; the
non-exhaustive `RecordSkipReason` distinguishes `MissingObject`, `MissingProperty`,
`NullPointer`, `CannotDescend`, `NotIndexable`, `IndexOutOfRange`, `InvalidKey`,
`KeyNotFound`, `TypeMismatch`, and `Unknown`, mapped from the `ltk_meta` patch error. A
diagnostic carries codes and typed fields; the consumer renders text.

Diagnostic kinds serialize in camelCase. Missing or unrecognized serialized kinds decode to
`Unknown`; existing messages and origins remain available. Fresh diagnostics carry explicit
kinds. Diagnostics persist in `overlay.json` and survive cache reuse. Declaration changes
participate in build invalidation. Declared dependencies participate in linked-bin validation.

Declared targets are applied before WAD distribution. Their final bytes remain shared
until compression. This memory cost scales with declared targets. Directory declarations
disable provider metadata caching and exact-match skipping; final content hashes permit
unchanged WAD reuse. Archive providers retain fingerprint-based exact-match skipping.
Base selection reads unfiltered mod metadata; game-identical copies remain candidates.
Missing final dependencies use `LinkedBinOffender`. Overlay state uses schema version 7.

`OverlayBuilder::with_called_off(poll)` registers a cancellation poll. The build polls it before
the chunk index loads, before overrides are collected, between the archives of an object index
build, and before each WAD is patched. A poll returning `true` ends `build()` with
`Error::CalledOff`; the state directory is as it was before the build.

## <a id="s7"></a>7. Validation

Public seams are declaration loading and application, project/archive round trips, and
overlay builds. Cases cover ordering, input classification, invalid declarations, identifier
construction, serialized field compatibility, target selection, enabled layers, typed
diagnostics, cached builds with missing or unknown diagnostic kinds, a called-off build,
override path resolution, override application with skipped records and unreadable files,
and override files round-tripping through both archives.

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
| D11 | An edit is a phased struct; an entry body takes `links` | Flat operations; entry bodies without `links` | The standard's unit is the batch; an entry names a chunk the author cannot spell | ADR-0010 |
| D12 | Declaration documents keep mapping order | Sorted JSON objects | `entries` apply in authored order | ADR-0011 |
| D13 | An override file is referenced by its layer-relative path in every container | Per-container references | One spelling from manifest to build | ADR-0013 |
| D14 | A target with an applied override is rewritten from the eager tree | A patched byte splice | The published `ltk_meta` owns matching and skipping | ADR-0012 |
| D15 | `overrides` binds a `target`; an entry body refuses it | `overrides` inside `entries` | An override names its objects itself | [section 4](#s4) |
| D16 | An override file is `.ptch`; `.rito` is an error naming the extension | Silent acceptance | A text override needs a `PTCH` text parser | [section 4](#s4) |
| D17 | An error is a code with a typed location | A location string and a message | The consumer matches the code and navigates by the place | [ADR-0014](../adr/0014-coded-declaration-errors.md) |
