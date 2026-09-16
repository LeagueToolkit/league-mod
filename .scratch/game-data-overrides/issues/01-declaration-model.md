---
issue: 236
title: "Game data: overrides binding"
labels: enhancement
---

Part of #191 (design: [`docs/design/game-data.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md)
[section 3](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md#s3)
and [section 4](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-data.md#s4)).
Adds the `overrides` binding to `ltk_game_data`: the path type, the first phase of `Edit`,
the wire key, and override application in `apply`.

## Proposed surface

```rust
/// The layer-relative, forward-slash path of a `.ptch` override file.
pub struct OverridePath(/* … */);
impl TryFrom<String> for OverridePath { type Error = Error; /* … */ }
impl TryFrom<&str> for OverridePath { type Error = Error; /* … */ }
impl From<OverridePath> for String { /* … */ }
impl fmt::Display for OverridePath { /* … */ }
impl OverridePath { pub fn as_str(&self) -> &str; }

#[non_exhaustive]
pub struct Edit {
    /// Override files applied in listed order, the first phase.
    pub overrides: Vec<OverridePath>,
    /// Dependency-list edits, the last phase.
    pub links: LinkEdit,
}

pub fn apply<B: AsRef<[u8]>>(
    base: &[u8],
    edits: &[Edit],
    read_override: impl FnMut(&OverridePath) -> Result<B, Error>,
) -> Result<ApplyResult, Error>;

#[non_exhaustive]
pub enum ApplyDiagnosticKind {
    OverrideUnreadable,
    OverrideInvalid,
    OverrideRecordSkipped,
    LinkRemovalUnmatched,
    Unknown,
}
#[non_exhaustive]
pub enum RecordSkipReason {
    MissingObject, MissingProperty, NullPointer, CannotDescend, NotIndexable,
    IndexOutOfRange, InvalidKey, KeyNotFound, TypeMismatch, Unknown,
}
pub struct SkippedRecord {
    pub index: usize,
    pub object: BinHash,
    pub property: String,
    pub reason: RecordSkipReason,
}
pub struct ApplyDiagnostic {
    pub kind: ApplyDiagnosticKind,
    pub edit_index: usize,
    /// The link path or override path the diagnostic is about.
    pub path: String,
    /// The record of an `OverrideRecordSkipped` diagnostic.
    pub record: Option<SkippedRecord>,
}

/// The inputs a manifest or source document references, discovered structurally.
pub struct ReferencedInputs { pub sources: Vec<String>, pub overrides: Vec<String> }
impl ReferencedInputs { pub fn discover(name: &str, text: &str) -> Result<Self, Error>; }
```

- `OverridePath` construction enforces nonempty, relative, no backslash, no `.` or `..`
  segment, `.ptch` extension compared ASCII case-insensitively; `.rito` is an error naming
  the extension (D5, D16).
- `load_declarations` resolves an authored path against the file naming it, lexically, and
  refuses one that leaves the layer; a loaded declaration carries the layer-relative
  spelling (ADR-0013). `overrides` in an entry body is an error (D15).
- `apply` applies each edit's override files before its link edits. A file the reader cannot
  supply is `OverrideUnreadable`; one that is not a `PTCH` is `OverrideInvalid`; both are
  skipped. `BinOverride::apply` lays a file over the decoded `Bin`; each skipped record is
  `OverrideRecordSkipped` with a `SkippedRecord` whose reason is a code mapped from the
  `ltk_meta` patch error. A diagnostic carries codes and typed fields, never prose. A target
  with an applied override is written from the tree at PROP version 3; one without keeps its
  object bytes and version (ADR-0012).
- `ReferencedInputs::discover` reports `source` and `overrides` references of a manifest or
  source document structurally, for input classification of rejected declarations. It
  replaces `referenced_sources`.
- `manifest_json` writes `overrides` for an edit with override paths. `DeclarationDocument`
  round-trips the key.

Breaking: `apply` gains a parameter, `ApplyDiagnostic` gains `record`, `referenced_sources`
becomes `ReferencedInputs::discover`.

- [ ] `OverridePath` accepts `a/b.ptch` and `X.PTCH`, refuses ``, `a\b.ptch`, `/a.ptch`, `../a.ptch`, `a/./b.ptch`, `a.rito` (with a message naming `.rito`), `a.bin`
- [ ] A source at `Test.wad.client/links.yaml` naming `../patch.ptch` loads as `patch.ptch`; one naming `../../patch.ptch` is an error
- [ ] `overrides` inside an `entries` body is an error from every format and from `DeclarationDocument::parse`
- [ ] `apply` with a `PTCH` setting one property rewrites that object, reports no diagnostic, and writes PROP version 3 from a version-2 base
- [ ] `apply` with a `PTCH` naming a missing object reports one `OverrideRecordSkipped` whose record is index 0, the object hash, `speed`, `MissingObject`, and applies the rest
- [ ] `apply` with a reader error reports `OverrideUnreadable`, with `PROP` bytes for an override reports `OverrideInvalid`, and continues to the link phase in both cases
- [ ] `apply` with no override paths keeps object bytes and the header version byte-identical
- [ ] `read_override` is called once per listed path in apply order across edits
- [ ] `ReferencedInputs::discover` returns the sources and override paths of a document with duplicate keys and unsupported bindings
- [ ] Lints, format, and docs clean
