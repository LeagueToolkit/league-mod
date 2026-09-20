# Game data declarations

## <a id="s1"></a>1. Summary

Game-data declarations travel from a mod project's layer manifest through archives to the
overlay builder. The binding syntax follows the [game-data reference](https://wiki.leaguetoolkit.dev/reference/mod-packages/game-data/).
The supported bindings are `overrides`, `links`, `+links`, `-links`, and entry bodies of
property edits. Other bindings are errors. A property edit's value is a literal or a reference
to a value of the installed game.

## <a id="s2"></a>2. Vocabulary

- **Declarations:** The versioned, ordered modules belonging to one layer.
- **Declaration document:** Preserved serialized declarations, including unsupported fields.
- **Module:** One selector with its edits and its origin.
- **Selector:** What a module edits: a chunk target with edits, or a mapping of entry names to entry edits.
- **Edit:** One batch of bindings on a chunk, applied phase by phase. The phases are override files, then entry edits, then link removals followed by additions.
- **Override file:** A `.ptch` file in a layer holding `PTCH` records applied over a target.
- **Override path:** The layer-relative, forward-slash path of an override file.
- **Entry edit:** The bindings of one entry: its property edits and, in an `entries` module, its links.
- **Entry body:** The mapping under an entry name: signed property paths to values, and in an `entries` module the link keys.
- **Property edit:** One signed property path with its value, on one entry.
- **Property path:** Riot's property path: dot-separated segments, each a name with an optional `[index]` or `{key}` subscript. `ltk_meta::path::PropertyPath` is normative.
- **Sign:** The operation of a property edit. A leading `+` on the key adds, a leading `-` removes, no sign sets.
- **Value:** The literal a property edit carries: null, boolean, integer, float, string, list, or mapping in spelled order.
- **Type name:** One of `bool`, `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `vec2`, `vec3`, `vec4`, `mtx44`, `rgba`, `string`, `hash`, `file`, `link`, `flag`, `option`, `pointer`, `embed`.
- **Type pin:** A one-key mapping whose key is a type name. In YAML a local tag `!name` on a value is the same pin. A pin fixes the type a value must have.
- **Struct pin:** A `pointer` or `embed` pin. Its value is null, or a mapping of `class` and `set`.
- **Schema:** The class schema of the installed patch: the shape of each field of each class, reached through the `Schema` trait.
- **Shape:** A property type: a kind, and for a container or an option its item kind, for a map its key kind and item kind.
- **Coercion:** The reading of a value as the shape of its property.
- **Reference:** A one-key mapping keyed `ref` whose value is an entry name, a `:`, and a property path. In YAML a local tag `!ref` on a string is the same reference. It names the value at that path in the installed game's copy of the entry.
- **Rendering:** The reading of a bin's property value as a `Value`. The inverse of coercion.
- **Names:** The plaintext a consumer holds for the field, class, entry, file, and hash values a bin carries, reached through the `Names` trait.
- **Leaf edit:** A property edit with block descent applied: a full path from the entry, a sign, and a value that is not a descent.
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

impl TryFrom<Declarations> for DeclarationDocument {
    type Error = Error;
}

pub fn apply<B: AsRef<[u8]>>(
    base: &[u8],
    edits: &[Edit],
    read_override: impl FnMut(&OverridePath) -> Result<B, Error>,
    read_entry: impl FnMut(&EntryName) -> Option<BinObject>,
    schema: &dyn Schema,
) -> Result<ApplyResult, Error>;

pub struct ApplyResult {
    pub bytes: Vec<u8>,
    pub dependencies: Vec<String>,
    pub diagnostics: Vec<ApplyDiagnostic>,
}

/// The class schema of the installed patch ([ADR-0015](../adr/0015-schema-trait.md)).
pub trait Schema {
    /// The shape of `field` on `class`. `None` is "the schema says nothing", never a mismatch.
    fn expected(&self, class: BinHash, field: BinHash) -> Option<Shape>;
    /// Whether the schema knows `class`. Asked of a class a struct pin names.
    fn has_class(&self, class: BinHash) -> bool;
}

pub struct Shape {
    pub kind: PropertyKind,
    /// A map's key kind.
    pub key: Option<PropertyKind>,
    /// A container's or an option's item kind; a map's value kind.
    pub item: Option<PropertyKind>,
}

/// The schema that says nothing. Every property is typed from the base, and no class is known.
pub struct NoSchema;

/// Plaintext for the hashes a rendered value carries (ADR-0020).
pub trait Names: ltk_meta::path::FieldNames {
    /// The name of a struct's class.
    fn class(&self, class: BinHash) -> Option<Cow<'_, str>>;
    /// The object path a `link` value hashes.
    fn entry(&self, entry: BinHash) -> Option<Cow<'_, str>>;
    /// The chunk path a `file` value hashes.
    fn file(&self, chunk: u64) -> Option<Cow<'_, str>>;
}

impl Value {
    /// The literal that coerces back to `value` under the value's own shape.
    pub fn render(value: &PropertyValueEnum, names: &dyn Names) -> Result<Value, Error>;
    /// The value as YAML text.
    pub fn to_yaml(&self) -> String;
}

/// A value of the installed game, named by entry and path (ADR-0021).
pub struct Reference {
    pub entry: EntryName,
    pub path: PropertyPath,
}
```

`Schema` is implemented for `&S`, `Box<S>`, and `Arc<S>` of any `S: Schema + ?Sized`.

`PropertyKind` is `ltk_meta::PropertyKind`; `PropertyPath` is `ltk_meta::path::PropertyPath`;
both are re-exported. `Shape` implements `Copy`, `PartialEq`, `Eq`, and `Hash`, and
`Shape::bare(kind)` is the shape with no key and no item. `NoSchema` implements `Schema` with
`expected` answering `None` and `has_class` answering `false`
([ADR-0022](../adr/0022-unattested-class-refusal.md)).

`FieldNames` is `ltk_meta::path::FieldNames`; `BinObject` is `ltk_meta::BinObject`; both are
re-exported. `Names` is implemented for `()`, which names nothing, and for `&N` of any
`N: Names + ?Sized`. `Value::render` follows the rendering table ([section 6](#s6)); a struct
field or a map key with no spelling is an error whose key is the rendered path
([ADR-0020](../adr/0020-value-rendering.md)). `Value::to_yaml` writes block style, a list whose
items are scalars in flow style, and quotes a string YAML reads as another type.
`Reference::parse(text)` splits `text` at its first `:`: the part before is an `EntryName`, the
part after a `PropertyPath`. `Reference` implements `Display` as the same spelling.

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
a contained document and validates the result; reading a document refuses a duplicate mapping
key anywhere in it. `Declarations` exposes mutable `version` and `modules` fields; `validate()`
checks supported declaration versions, and refuses a target module with no edit and an entries
module with no entry. `manifest_json()` validates and writes a direct JSON manifest.
A conversion to a serialized form refuses what that form cannot carry, and the manifest and
the document refuse the same things ([ADR-0023](../adr/0023-refusing-serialization.md)): a
property path or entry name spelling a binding keyword, one signed key held twice by an entry,
and an integer outside the union of the `i64` and `u64` ranges. `manifest_json()` output loads
to the declarations it was written from. Target and link-path validity is enforced by their
types ([section 4](#s4)). `apply()` runs each edit's phases in field order, each edit over the
result of the preceding one, returns bytes, and leaves its input unchanged. `schema` types
every property edit ([section 6](#s6)); a caller with no schema passes `&NoSchema`. `read_override`
supplies the bytes of an override file by its path in any `AsRef<[u8]>` container; it is
called once per listed path, in apply order. `read_entry` supplies the installed game's copy
of an entry a reference names, and `None` for an entry the game lacks; a caller with no game
passes `|_| None`. `dependencies` contains the resulting BIN dependency spellings, including
retained base entries.

## <a id="s4"></a>4. Authoring

A layer has at most one `game_data.yaml`, `game_data.yml`, `game_data.toml`, or
`game_data.json`. A manifest or source file's extension names its format, compared ASCII
case-insensitively. The manifest requires integer `version: 1` and a `modules` array.
A module contains one selector. A `target` selector takes a compact binding body, `edits`, or
`source`. An `entries` selector is a mapping of entry names to entry bodies and takes
nothing else; an `entries` module is one batch. A module with both keys, or neither, is an
error. A compact body and every edit carry at least one binding. A binding body's keys are
`overrides`, `links`, `+links`, `-links`, and entry names; an entry name at a body root carries
a slash or is hash-form, and every other key is an unsupported binding and an error.
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
in `.ptch`, compared ASCII case-insensitively. A first segment holding a `:` spells a drive
path, `C:/a.ptch` or `C:a.ptch`, and is an error. A `.rito` path is an error naming the
unsupported extension. A path that leaves the layer is a loading error.

An entry name is one nonempty string. `0x` followed by exactly 8 ASCII hexadecimal digits
identifies an object hash; every other spelling identifies an object path. A hash-form entry
name requires quotes in YAML, as a numeric-looking target does. `EntryName` has the
same construction and string-access traits as `Target`, implements `Display`, and
`object_hash()` returns its `BinHash`.

`Module` contains `selector: Selector` and `origin: Origin`
([ADR-0010](../adr/0010-phased-declaration-edits.md)).
`Selector` is non-exhaustive.
`Selector::Target { target: Target, edits: Vec<Edit> }` edits one chunk.
`Selector::Entries(IndexMap<EntryName, EntryEdit>)` edits each named entry in every declaring
chunk, in mapping order.
`Edit` is one batch. It is non-exhaustive, implements `Default`, and its fields are its phases
in apply order: `overrides: Vec<OverridePath>`, then
`entries: IndexMap<EntryName, Vec<PropertyEdit>>`, then `links: LinkEdit`. `EntryEdit` is the
bindings of one entry, non-exhaustive with `Default`, with the fields
`properties: Vec<PropertyEdit>` and `links: LinkEdit`, the links as an extension of the
game-data reference; `overrides` in an entry body is an error. `LinkEdit` contains
`add: Vec<LinkPath>` and `remove: Vec<LinkPath>`. `Origin` contains `manifest`, optional
`source`, and `module_index`.

An entry body is a mapping of property edits ([ADR-0016](../adr/0016-literal-property-values.md)).
Each key is a sign and a property path; each value is a `Value`:

```rust
pub struct PropertyEdit {
    pub path: PropertyPath,
    pub sign: Sign,
    pub value: Value,
}

pub enum Sign { Set, Add, Remove }

#[non_exhaustive]
pub enum Value {
    Null,
    Bool(bool),
    /// Any integer of the union of the `i64` and `u64` ranges.
    Integer(i128),
    Float(f64),
    String(String),
    List(Vec<Value>),
    Mapping(IndexMap<String, Value>),
}
```

`Sign::of(key)` splits a leading `+` or `-` from a key and returns the sign with the rest;
`Sign::as_str()` is `""`, `"+"`, or `"-"`. `PropertyEdit::key()` is the signed key as spelled.
A key's path is parsed by `PropertyPath::new` and a refused path is an error; the sign is not
part of the path. `Value` implements `PartialEq`, `Serialize`, and `Deserialize`; a mapping
refuses a duplicate key in every format; an integer past the ranges named is what the format's
parser makes of it, a float. `Value::Integer` holds an `i128`; serializing one outside the
union of the `i64` and `u64` ranges is an error.
A YAML local tag on a value loads as the one-key mapping of its name: `!f32 1.0`
loads as `{f32: 1.0}`, and `!ref a:b` loads as `{ref: "a:b"}`; a tag whose name is neither a
type name nor `ref` is an error. `Value::pin()` is the
type name of a one-key mapping whose key is a type name, or `None`; `Value::reference()` is the
text of a one-key mapping keyed `ref`, or `None`; `Value::is_struct_pin()`
is whether that name is `pointer` or `embed`; `Value::check_pins()` is the struct-pin and
reference check loading runs; `Value` implements `Display` as a JSON-like rendering. `kind_named(name)` and
`name_of(kind)` map type names to `PropertyKind` and back. `EntryName::is_hash()` is whether
the name is spelled as a hash.

Loading checks the structure of every value. A one-key mapping keyed `pointer` or `embed`,
anywhere in a value, is a struct pin: its value is null or the empty mapping (`pointer`
only, the null pointer), or a mapping whose keys are `class`, a string, and `set`, a mapping,
with at least one of the two; any other shape is an error. A one-key mapping keyed by any
other type name is a type pin on every property ([ADR-0019](../adr/0019-uniform-type-pins.md)).
A one-key mapping keyed `ref`, anywhere in a value, is a reference
([ADR-0021](../adr/0021-game-copy-references.md)): its value is a string `Reference::parse`
accepts; any other shape is an error.
Every other mapping is read at apply time by the property's type ([section 6](#s6)). A mapping
that is not a pin on a struct property descends into it (block nesting): each key of the
mapping is itself a signed property path relative to the struct, in any format; the dotted
form `a.b: 1` and the block form `a: {b: 1}` are one edit. A block whose only key is a type
name is a pin; the dotted form `a.hash: 1` reaches a field with a type's name. A block whose
only key is `ref` is a reference; the dotted form `a.ref: 1` reaches a field named `ref`. An index stays
a path segment, `bankUnits[0]: {...}`.

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
binding is a legal document value; `manifest_json()` writes `links` for every edit,
`overrides` for an edit with override paths, and one key per entry name for an edit with
entry edits, each value the entry body with every `Value` as its literal.
Declaration version 1 identifies the supported format.

Rust names and serialized names have the following mapping:

| Rust field | Serialized field |
| --- | --- |
| `Edit::overrides` | `overrides` |
| `Edit::entries` | one key per entry name at the body root, its value the entry body |
| `EntryEdit::properties` | the signed property keys of an entry body |
| `PropertyEdit` | `path: value`, `+path: value`, or `-path: value` |
| `Value` | the JSON literal; a YAML tag `!name value` is written `{name: value}` |
| `LinkEdit::add` | `links` (`+links` accepted on input) |
| `LinkEdit::remove` | `-links` |
| `Selector::Target` | `target` and `edits`, one compact body per edit |
| `Selector::Entries` | `entries`, a mapping of entry name to one compact body |
| `Origin::module_index` | `module` |
| Diagnostic `edit_index` | `edit` |
| `OverlayState::game_data_diagnostics` | `gameDataReports` |

## <a id="s6"></a>6. Overlay

`OverlayBuilder::with_game_data_schema(schema)` registers the installed patch's class schema,
any `Schema + Send + Sync + 'static`; a builder without one applies with `NoSchema`
([ADR-0015](../adr/0015-schema-trait.md)). LTK Manager implements `Schema` over the schema it
holds for the installed patch.

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
in mapping order, before base selection. The application is one `Edit` whose `entries` holds
the entry's property edits under its name and whose `links` are the entry's links. `ObjectIndex::declarations` supplies the declaring
chunks. An entry with several declaring chunks is edited in every one; an `EntryFanOut`
diagnostic names them. An entry with no declaring chunk produces `EntryUnresolved` and its
edits are skipped. A diagnostic of an entry carries the entry name as its `target`; a
lowered application names its chunk by hex hash and carries it in `chunk`. The overlay loads or builds the object index only for a build in which an
enabled layer declares an `entries` module, from `object_index.bin` beside `game_index.bin`,
under the `IndexingObjects` build stage. An object index that fails to load and build produces
`IndexUnavailable` for every `entries` module and every module holding a reference, and a
warning in the log; the build continues. An object index build the cancellation poll stops
ends the build. The overlay also loads or builds the object index for a build in which an
enabled layer declares a reference. It answers `read_entry` with the entry's object in the
first of its declaring chunks in `ltk_game_index` archive order, read from the game before any
mod content applies; a build reads and decodes each such chunk once.
The target must be PROP version 2 or 3. Invalid declarations refuse the layer's declarations;
ordinary content remains available. Missing or invalid targets produce diagnostics and retain
their original bytes. `ltk_meta` decodes the complete base.

Each edit applies its override files in listed order, then its entry edits, then its link
edits. An override file
the provider cannot supply produces `OverrideUnreadable`; one that does not read as a `PTCH`
produces `OverrideInvalid`; either file is skipped. `ltk_meta` lays an override over the
target in the client's order: deletions, added objects, then records in file order. A record
that does not apply produces `OverrideRecordSkipped` carrying a `SkippedRecord`: the record
index, object hash, property path, and a `RecordSkipReason` code; the remaining records
continue. Records apply as authored, without coercion. A target with an applied
override file or an applied property edit is written from the decoded tree at PROP version 3
([ADR-0012](../adr/0012-eager-tree-override-application.md)). A target with neither keeps its
object bytes and PROP version; application replaces only the dependency header.

**Entry edits.** Each entry of `Edit::entries` names an object of the target by its hash; an
absent object skips every edit of the entry with `MissingObject`. The entry's property edits
lower to leaf edits ([ADR-0017](../adr/0017-per-key-patch-lowering.md)): an edit whose
property is a `pointer` or `embed` in the base and whose value is a mapping that is not a
type pin descends, each key a signed path relative to the struct, joined to the outer path;
a null pointer in the base is `NullPointer`. Leaf edits are grouped by path in first-occurrence
order. Per path: the set value, coerced, replaces the base value; the removals then the
additions apply to the result; one `Bin::patch` sets the property. A property the object
lacks is created; a path with a subscript the base lacks is a report by its resolution
reason. A sign on a property whose shape is not a list, list2, or map is `SignOnScalar`.
A map edit is a whole-map replacement; no `{key}` record is emitted.

**Typing.** The shape of a property is the schema's answer for the field on the class of the
struct holding it. Where the schema says nothing the base value's shape is the type and one
`SchemaFallback` diagnostic names the path. A property the base omits with no schema answer is
`Untypable`. A subscripted path is typed by the container's item kind, or the map's value
kind. A struct pin's `class` is a name, hashed FNV-1a lowercased, or `0x` and 8 hexadecimal
digits; a class the pin names and the schema does not know is `UnknownClass`. A pin without a
`class` key takes the class of the base value, which the shipped bin attests, and the schema
is not consulted ([ADR-0022](../adr/0022-unattested-class-refusal.md)). Inside a `set`, each
key is one field name of the pinned class typed by the schema; a nested struct is a nested
struct pin.

**Coercion.** A value coerces to a shape by these rules; any other pair is `KindMismatch`.

| Value | Shape | Rule |
| --- | --- | --- |
| integer | `i8` to `i64`, `u8` to `u64` | In range, else `OutOfRange` |
| integer | `flag` | `0` or `1`, else `OutOfRange` |
| integer | `f32` | Exact, else `PrecisionLoss` |
| float | `f32` | Rounded to single precision |
| boolean | `bool`, `flag` | As is |
| string | `string` | As is |
| string | `hash`, `link` | FNV-1a 32 of the ASCII-lowercased string; `0x` and 8 hexadecimal digits pass through; `""` is `0` |
| string | `file` | XXH64 of the ASCII-lowercased string; `0x` and 16 hexadecimal digits pass through; `""` is `0` |
| null | `hash`, `link`, `file` | `0` |
| null | `pointer` | The null pointer |
| null | `option` | The empty option |
| list | `list`, `list2` | Each element to the item kind |
| list | `option` | Zero or one element to the item kind, else `ArityMismatch` |
| any other value | `option` | The one element, to the item kind |
| list | `vec2`, `vec3`, `vec4`, `mtx44` | Exactly 2, 3, 4, or 16 numbers to `f32`, else `ArityMismatch` |
| list | `rgba` | Exactly 4 integers from 0 to 255, else `ArityMismatch` or `OutOfRange` |
| mapping | `map` | Each key, a string, to the key kind by the string rules; each value to the value kind |
| mapping | `pointer`, `embed` | A struct pin constructs the struct; a pin of any other type name is `PinMismatch`; any other mapping descends ([section 4](#s4)) |
| `{}` | `pointer`, `option` | Inside a struct pin or an `option` pin only, the null pointer or the empty option |
| reference | any | The value at the reference in the game's copy, where `Shape::of` the value is the shape read; an entry `read_entry` does not supply is `ReferenceMissingEntry`, a path the entry does not resolve is `ReferenceUnresolved`, any other shape is `KindMismatch` |

A type pin on a value fixes the shape: a pin whose type name is not the property's kind is
`PinMismatch`, and the pinned value coerces by the row of that kind. On a list, an option, or
a map, signed or not, a pin names the item kind. A pin on a map value pins its values. A pin is read by one rule on every property kind,
including a field inside a struct pin's `set`
([ADR-0019](../adr/0019-uniform-type-pins.md)). An `option` pin wraps one bare or pinned
value, or null.

**Rendering.** A property value renders to a `Value` by these rules
([ADR-0020](../adr/0020-value-rendering.md)). A rendered value coerces back to the same value
under the value's own shape, and carries no type pin.

| Kind | Rendered |
| --- | --- |
| `bool`, `flag` | The boolean |
| `i8` to `i64`, `u8` to `u64` | The integer |
| `f32` | The float with the shortest spelling that rounds to the same single-precision bits |
| `vec2`, `vec3`, `vec4`, `mtx44` | A list of 2, 3, 4, or 16 floats, in the order coercion reads them |
| `rgba` | A list of 4 integers |
| `string` | The string |
| `hash` | `FieldNames::hash` of the value, else `0x` and 8 hexadecimal digits |
| `link` | `Names::entry` of the value, else `0x` and 8 hexadecimal digits |
| `file` | `Names::file` of the value, else `0x` and 16 hexadecimal digits |
| `list`, `list2` | A list of the rendered items |
| `option` | Null when empty; else the rendered element, or a one-element list where the element renders as a list |
| `map` | A mapping of each key, rendered as a string, to its rendered value |
| `pointer` | Null for the null pointer; else `{pointer: {class, set}}` |
| `embed` | `{embed: {class, set}}` |

A struct's `class` is `Names::class`, else `0x` and 8 hexadecimal digits. Its `set` holds one
key per field, the name `FieldNames::field` answers for the field on the class, and is absent
for a struct with no fields. A map key renders as a `bool`, integer, or `f32` spelling, or as a
`string`, `hash`, or `file` value renders. A field with no name, and a key of any other kind,
is an error.

**Additions and removals.** `+` on a list appends each element, coerced to the item kind, to
the base's elements; on a map it adds or replaces by key; on a property the base omits it
creates the container with the schema's shape, or is `Untypable`. `-` on a property the base
omits is `ContainerAbsent`. `-` on a list removes by value where the item kind is not `pointer` or
`embed`: each removal coerces to the item kind and removes every equal element; a removal
that matches nothing is `RemovalUnmatched`. Where the item kind is `pointer` or `embed`, `-`
removes by index: each removal is an integer index into
the base's list, and one out of range is `RemovalUnmatched`. `-` on a map removes by key; a
key the base lacks is `RemovalUnmatched`. A report on any of a key's operations skips the
key: its set, removals, and additions together. The diagnostic's `path` carries the sign of
the operation that failed; a base container whose kinds are not the schema's shape, and a
value `Bin::patch` refuses, are `TypeMismatch` under the sign of the key's first operation.

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
`OverrideUnreadable`, `OverrideInvalid`, `OverrideRecordSkipped`, `LinkRemovalUnmatched`,
`PropertyEditSkipped`, `SchemaFallback`, and `Unknown`. `EntryFanOut` and `SchemaFallback` are
informational. A `GameDataDiagnostic` of kind `OverrideRecordSkipped` carries the
`SkippedRecord` in its optional `record` field, and one of kind `PropertyEditSkipped` carries
the `SkippedProperty` in its optional `property` field; each is absent otherwise and decodes
absent as `None`. `ApplyDiagnostic` contains `kind`, `edit_index`, `path`, optional `record`,
and optional `property`; `path` is the link path, the override path, or the signed property
key the diagnostic is about, a block's inner key joined to its outer path. Its non-exhaustive
`ApplyDiagnosticKind` distinguishes `OverrideUnreadable`, `OverrideInvalid`,
`OverrideRecordSkipped`, `LinkRemovalUnmatched`, `PropertyEditSkipped`, `SchemaFallback`, and
`Unknown` ([ADR-0018](../adr/0018-property-edit-diagnostics.md)). `SkippedProperty` contains
`entry`, an `EntryName`, and `reason`; the non-exhaustive `PropertySkipReason` distinguishes
`MissingObject`, `MissingProperty`, `NullPointer`, `CannotDescend`, `NotIndexable`,
`IndexOutOfRange`, `InvalidKey`, `KeyNotFound`, `TypeMismatch`, `InvalidPath`, `Untypable`,
`UnknownClass`, `PinMismatch`, `SignOnScalar`, `ContainerAbsent`, `RemovalUnmatched`,
`KindMismatch`, `OutOfRange`, `PrecisionLoss`, `ArityMismatch`, `ReferenceMissingEntry`,
`ReferenceUnresolved`, and `Unknown`. The first nine
are the `RecordSkipReason` codes of a path that does not resolve or a value `Bin::patch`
refuses; `InvalidPath` is a key inside a block or a `set` that is not a property path.
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
Missing final dependencies use `LinkedBinOffender`. Overlay state uses schema version 8.

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
override files round-tripping through both archives, entry bodies in every format with tags
and one-key pins loading to one model, block and dotted forms loading to one edit, structural
refusals of paths, tags, and struct pins, coercion of every row of the table against a
hand-written schema and against the base alone, additions and removals on lists and maps,
per-key order, every `PropertySkipReason`, `SchemaFallback`, entry bodies packed and extracted
through both archives, and an overlay build with a schema and a cached replay of the two kinds.
Rendering cases cover every row of the rendering table coerced back to the same value, `f32`
spellings, an `option` of a vector, a nameless field, and YAML output reloaded. Reference cases
cover the tag and the one-key mapping in every format, the dotted escape, a reference through a
hand-written `read_entry` as a set, an addition, a removal, a map value, a list item, and a
`set` field, both reference reasons, a shape mismatch, and an overlay build resolving a
reference from the game.

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
| D18 | The class schema enters through a `Schema` trait | A schema crate; schema data | The library is testable without a dump; the manager adapts what it holds | [ADR-0015](../adr/0015-schema-trait.md) |
| D19 | A property edit carries a literal value; a YAML tag lowers to the one-key mapping | Load-time coercion; a pinned document form | The type is the installed patch's on the day of the build | [ADR-0016](../adr/0016-literal-property-values.md) |
| D20 | Property edits lower to one `Bin::patch` per key; a container edit is a whole replacement | In-place container operations; `{key}` records | The format crate performs every write | [ADR-0017](../adr/0017-per-key-patch-lowering.md) |
| D21 | A skipped property edit is one `PropertyEditSkipped` with a reason code | One kind per condition | The override pattern | [ADR-0018](../adr/0018-property-edit-diagnostics.md) |
| D22 | An entry name at a body root carries a slash or is hash-form | Any key | The standard's words and the game's never share a mapping | [section 4](#s4) |
| D23 | A struct pin's `set` keys are single field names | Dotted paths in a `set` | A new struct has no base to descend through | [section 6](#s6) |
| D24 | A one-key mapping keyed by a type name is a pin on every property kind | A descent on a struct | A tag and the one-key mapping are one value; a pin never turns into a field | [ADR-0019](../adr/0019-uniform-type-pins.md) |
| D25 | Rendering lives beside coercion, with names through a trait over `FieldNames` | Rendering in each consumer | A coercion row and its inverse change under one test | [ADR-0020](../adr/0020-value-rendering.md) |
| D26 | A rendered value carries no type pin | A pin on every value | The installed patch's schema types the value at the build | [section 6](#s6) |
| D27 | A reference is a value resolved against the installed game's copy | A reference into the build state; an object binding only | The result depends on the game and the declaration, never on mod order | [ADR-0021](../adr/0021-game-copy-references.md) |
| D28 | `ref` is a reserved one-key mapping key | A field lookup | A tag and the one-key mapping are one value, as with pins; no Riot field hashes to `ref` | [ADR-0021](../adr/0021-game-copy-references.md) |
| D29 | A reference splits at its first `:` | A split at a `.` | LTK Manager's Copy path writes `<entry>:<path>`; no known entry name holds a `:` | [section 4](#s4) |
