//! Application of edits over a `PROP`: override files, entry edits, then link edits.

mod coerce;
mod entries;

use std::{collections::HashSet, io::Cursor};

use ltk_meta::{
    BinObject, BinOverride,
    concrete::{Bin, BinStream},
    path::{PatchError, ResolveErrorKind},
};
use serde::{Deserialize, Serialize};

use crate::{BinHash, Edit, EntryName, Error, ErrorKind, OverridePath, Schema};

/// The category of an application diagnostic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub enum ApplyDiagnosticKind {
    /// An override file the reader cannot supply. The file is skipped.
    OverrideUnreadable,
    /// An override file that is not a `PTCH`. The file is skipped.
    OverrideInvalid,
    /// One override record that does not apply to the target. The remaining records apply.
    OverrideRecordSkipped,
    /// A `-links` path the target's dependency list does not hold. The edit continues.
    LinkRemovalUnmatched,
    /// One property key whose edit does not apply. The remaining keys apply.
    PropertyEditSkipped,
    /// A property typed from the base, the schema saying nothing. Informational.
    SchemaFallback,
    /// A missing or unrecognized serialized category.
    #[default]
    #[serde(other)]
    Unknown,
}

/// Why an override record does not apply. The client's own rule is a skip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub enum RecordSkipReason {
    /// The target has no object with the record's hash.
    MissingObject,
    /// A path segment names a property the value does not have.
    MissingProperty,
    /// A path segment descends through a null pointer.
    NullPointer,
    /// A path segment descends into a value that is not a pointer or an embed.
    CannotDescend,
    /// A subscript on a value that is not a list, option, or map.
    NotIndexable,
    /// A list or option index past the end.
    IndexOutOfRange,
    /// A map key that does not convert to the map's key kind.
    InvalidKey,
    /// A map key no entry has.
    KeyNotFound,
    /// The record's value is not the property's type.
    TypeMismatch,
    /// A reason this crate does not name.
    #[default]
    #[serde(other)]
    Unknown,
}

impl From<ResolveErrorKind> for RecordSkipReason {
    fn from(kind: ResolveErrorKind) -> Self {
        match kind {
            ResolveErrorKind::MissingObject(_) => Self::MissingObject,
            ResolveErrorKind::MissingProperty(_) => Self::MissingProperty,
            ResolveErrorKind::NullPointer => Self::NullPointer,
            ResolveErrorKind::CannotDescend(_) => Self::CannotDescend,
            ResolveErrorKind::NotIndexable(_) => Self::NotIndexable,
            ResolveErrorKind::IndexOutOfRange { .. } => Self::IndexOutOfRange,
            ResolveErrorKind::InvalidKey(_) => Self::InvalidKey,
            ResolveErrorKind::KeyNotFound => Self::KeyNotFound,
            _ => Self::Unknown,
        }
    }
}

impl From<&PatchError> for RecordSkipReason {
    fn from(error: &PatchError) -> Self {
        match error {
            PatchError::Resolve(error) => error.kind().into(),
            PatchError::TypeMismatch { .. } => Self::TypeMismatch,
            _ => Self::Unknown,
        }
    }
}

/// Why a property edit does not apply.
///
/// The first nine codes are the [`RecordSkipReason`] codes of a path that does not resolve
/// or a value the tree refuses; the rest are the typing, pin, sign, container, and coercion
/// rules of `docs/design/game-data.md` section 6.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub enum PropertySkipReason {
    /// The target has no object with the entry's hash.
    MissingObject,
    /// A path segment names a property the value does not have.
    MissingProperty,
    /// A path segment descends through a null pointer.
    NullPointer,
    /// A path segment descends into a value that is not a pointer or an embed.
    CannotDescend,
    /// A subscript on a value that is not a list, option, or map.
    NotIndexable,
    /// A list or option index past the end.
    IndexOutOfRange,
    /// A map key that does not convert to the map's key kind.
    InvalidKey,
    /// A map key no entry has.
    KeyNotFound,
    /// The coerced value's shape is not the base value's shape.
    TypeMismatch,
    /// A key inside a block or a `set` that is not a property path.
    InvalidPath,
    /// The property has no type: the base omits it and the schema says nothing.
    Untypable,
    /// A struct pin's class the schema does not know.
    UnknownClass,
    /// A pin whose type name is not the property's kind.
    PinMismatch,
    /// A `+` or `-` on a property that is not a list or a map.
    SignOnScalar,
    /// A `-` on a container the base omits.
    ContainerAbsent,
    /// A removal that matches no element, index, or key.
    RemovalUnmatched,
    /// A value of a kind no coercion row accepts for the property's shape.
    KindMismatch,
    /// An integer outside the range of the property's kind.
    OutOfRange,
    /// An integer an `f32` does not represent exactly.
    PrecisionLoss,
    /// A list whose length is not the shape's.
    ArityMismatch,
    /// A reference whose entry the caller does not supply. The game lacks it, or the caller
    /// reads no game.
    ReferenceMissingEntry,
    /// A reference whose path the supplied entry does not resolve.
    ReferenceUnresolved,
    /// A reason this crate does not name.
    #[default]
    #[serde(other)]
    Unknown,
}

impl From<RecordSkipReason> for PropertySkipReason {
    /// Written arm by arm with no catch-all, so a code gained by one of the two enums and
    /// not the other is a compile error rather than a diagnostic that silently reads
    /// `Unknown`. The nine codes the two share are the same nine in the same order.
    fn from(reason: RecordSkipReason) -> Self {
        match reason {
            RecordSkipReason::MissingObject => Self::MissingObject,
            RecordSkipReason::MissingProperty => Self::MissingProperty,
            RecordSkipReason::NullPointer => Self::NullPointer,
            RecordSkipReason::CannotDescend => Self::CannotDescend,
            RecordSkipReason::NotIndexable => Self::NotIndexable,
            RecordSkipReason::IndexOutOfRange => Self::IndexOutOfRange,
            RecordSkipReason::InvalidKey => Self::InvalidKey,
            RecordSkipReason::KeyNotFound => Self::KeyNotFound,
            RecordSkipReason::TypeMismatch => Self::TypeMismatch,
            RecordSkipReason::Unknown => Self::Unknown,
        }
    }
}

impl From<ResolveErrorKind> for PropertySkipReason {
    fn from(kind: ResolveErrorKind) -> Self {
        RecordSkipReason::from(kind).into()
    }
}

impl From<&PatchError> for PropertySkipReason {
    fn from(error: &PatchError) -> Self {
        RecordSkipReason::from(error).into()
    }
}

/// One property edit that does not apply. The diagnostic's `path` is its signed key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedProperty {
    /// The entry the edit addresses, as spelled.
    pub entry: EntryName,
    /// Why the edit does not apply.
    pub reason: PropertySkipReason,
}

/// One override record that does not apply. The record index is zero-based within its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedRecord {
    /// The record's position in its override file.
    pub index: usize,
    /// The object the record addresses.
    pub object: BinHash,
    /// The property path the record addresses, as the file spells it.
    pub property: String,
    /// Why the record does not apply.
    pub reason: RecordSkipReason,
}

/// An application diagnostic. The edit index is zero-based.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyDiagnostic {
    #[serde(default)]
    pub kind: ApplyDiagnosticKind,
    #[serde(rename = "edit")]
    pub edit_index: usize,
    /// The link path, the override path, or the signed property key the diagnostic is about.
    pub path: String,
    /// The record of an `OverrideRecordSkipped` diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<SkippedRecord>,
    /// The property of a `PropertyEditSkipped` diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property: Option<SkippedProperty>,
    /// What a lower layer said, when it said something this crate's codes do not carry. An
    /// unreadable override carries the reader's error. An invalid one carries the decoder's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ApplyDiagnosticKind {
    /// The statement of a diagnostic of this category about `path`.
    ///
    /// The text lives beside the codes because a consumer that renders the codes itself
    /// writes one arm per variant and silently loses whichever variant it has not heard of.
    #[must_use]
    pub fn message(self, path: &str) -> String {
        match self {
            Self::OverrideUnreadable => format!("Override file cannot be read: {path}"),
            Self::OverrideInvalid => format!("Override file is not a PTCH: {path}"),
            Self::OverrideRecordSkipped => format!("Override record is skipped: {path}"),
            Self::LinkRemovalUnmatched => format!("Link removal is absent: {path}"),
            Self::PropertyEditSkipped => format!("Property edit is skipped: {path}"),
            Self::SchemaFallback => format!("Property is typed from the base: {path}"),
            Self::Unknown => format!("Application diagnostic: {path}"),
        }
    }
}

impl std::fmt::Display for ApplyDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.kind.message(&self.path))?;
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

/// What an application changed, counted across every edit.
///
/// Every skip is a diagnostic, but a list of skips does not say whether anything landed. An
/// application with no diagnostics at all is both an application where every edit applied
/// and an application where the caller passed no edits. These counts say which.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Applied {
    /// Override records that applied, over every override file of every edit.
    pub records: usize,
    /// Objects an override file added to, replaced in, or deleted from the target.
    pub objects: usize,
    /// Property keys whose patch landed. One key is one patch, whatever its sign.
    pub properties: usize,
    /// Dependencies added to the target.
    pub links_added: usize,
    /// Dependencies removed from the target.
    pub links_removed: usize,
}

impl Applied {
    /// Whether any edit took effect.
    ///
    /// `false` means the returned bytes carry nothing the caller declared. Every edit was
    /// skipped, or there were no edits at all. A caller that writes the result somewhere
    /// writes the base.
    #[must_use]
    pub fn any(&self) -> bool {
        self.records > 0
            || self.objects > 0
            || self.properties > 0
            || self.links_added > 0
            || self.links_removed > 0
    }
}

/// The outcome of one application: the target's bytes, its dependency list, what the edits
/// changed, and every nonfatal outcome.
#[derive(Debug)]
pub struct ApplyResult {
    /// The target after every edit, a `PROP`.
    pub bytes: Vec<u8>,
    /// The dependency spellings of `bytes`, retained base entries included.
    pub dependencies: Vec<String>,
    /// What the edits changed.
    pub applied: Applied,
    /// Every diagnostic in apply order.
    pub diagnostics: Vec<ApplyDiagnostic>,
}

impl ApplyResult {
    /// Whether any edit took effect. See [`Applied::any`].
    #[must_use]
    pub fn changed(&self) -> bool {
        self.applied.any()
    }
}

/// The object `name` names in a `PROP` chunk.
///
/// This is what a caller answers `apply`'s `read_entry` with once it holds the bytes of a
/// declaring chunk. Decoding lives here because the crate already owns `PROP` decoding, and
/// a consumer that resolves references should not have to take a bin library of its own.
///
/// `None` for bytes that are not a readable `PROP` and for a chunk that does not hold the
/// object. Both are references the build cannot resolve, and neither is worth telling apart
/// at the call site.
#[must_use]
pub fn read_entry(bytes: &[u8], name: &EntryName) -> Option<BinObject> {
    let mut stream = BinStream::mount(Cursor::new(bytes)).ok()?;
    stream.object(name.object_hash()).ok()??.read().ok()
}

/// The game's copy of every entry the edits reference.
///
/// A reference reads the game, not the target being built, so every reference of a batch is
/// answered from one reading taken before the first edit applies. Asking once per distinct
/// entry also keeps the cost of a reference off the caller: the overlay reads and decodes a
/// chunk per entry, however many references name it.
///
/// A reference the name rule refuses is not collected. Coercion reports it as it reports
/// every other reference it cannot resolve.
fn resolve_references(
    edits: &[Edit],
    mut read_entry: impl FnMut(&EntryName) -> Option<BinObject>,
) -> coerce::ResolvedReferences {
    let mut resolved = coerce::ResolvedReferences::new();
    let mut asked = HashSet::new();
    for reference in edits.iter().flat_map(Edit::references) {
        if !asked.insert(reference.entry.clone()) {
            continue;
        }
        if let Some(object) = read_entry(&reference.entry) {
            resolved.insert(reference.entry, object);
        }
    }
    resolved
}

/// Applies ordered edits to a PROP v2 or v3.
///
/// Each edit runs its phases in field order and reads the result of the preceding edit.
/// `read_override` supplies the bytes of an override file by its path, once per listed path
/// in apply order, in any byte container; a caller sharing one file across several targets
/// hands over an `Arc<[u8]>`. `read_entry` supplies the installed game's copy of an entry a
/// reference names, once per distinct entry referenced and before any edit applies, and
/// `None` for an entry the game lacks; a caller with no game passes `|_| None`. `schema`
/// types every property edit; a caller with no schema passes `&NoSchema`. A target with an
/// applied override file or an applied property edit is written from the decoded tree at PROP
/// version 3; a target with neither keeps its object bytes and header version.
///
/// # Errors
///
/// The base is not a PROP version 2 or 3, or its object table does not decode.
pub fn apply<B: AsRef<[u8]>>(
    base: &[u8],
    edits: &[Edit],
    mut read_override: impl FnMut(&OverridePath) -> Result<B, Error>,
    read_entry: impl FnMut(&EntryName) -> Option<BinObject>,
    schema: &dyn Schema,
) -> Result<ApplyResult, Error> {
    let references = resolve_references(edits, read_entry);
    let stream = BinStream::mount(Cursor::new(base)).map_err(|e| bin_error(&e))?;
    if !matches!(stream.version(), 2 | 3) {
        return Err(Error::new(ErrorKind::UnsupportedBase));
    }
    let body_offset = 12
        + stream
            .dependencies()
            .iter()
            .map(|path| 2 + path.len())
            .sum::<usize>();
    let mut bin: Bin = stream.into_bin().map_err(|e| bin_error(&e))?;
    let mut seen = HashSet::new();
    bin.dependencies
        .retain(|path| seen.insert(path.as_str().to_ascii_lowercase()));
    let mut diagnostics = Vec::new();
    let mut applied = Applied::default();
    for (index, edit) in edits.iter().enumerate() {
        for path in &edit.overrides {
            let mut report = |kind, record, detail| {
                diagnostics.push(ApplyDiagnostic {
                    kind,
                    edit_index: index,
                    path: path.as_str().to_owned(),
                    record,
                    property: None,
                    detail,
                });
            };
            // The reader's own error says why the file could not be supplied. It can be
            // missing, outside the layer, or unreadable, and the caller has no other way
            // to learn which.
            let bytes = match read_override(path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    report(
                        ApplyDiagnosticKind::OverrideUnreadable,
                        None,
                        Some(error.to_string()),
                    );
                    continue;
                }
            };
            let patch = match BinOverride::from_reader(&mut Cursor::new(bytes.as_ref())) {
                Ok(patch) => patch,
                Err(error) => {
                    report(
                        ApplyDiagnosticKind::OverrideInvalid,
                        None,
                        Some(error.to_string()),
                    );
                    continue;
                }
            };
            let report_of_patch = patch.apply(&mut bin);
            applied.records += report_of_patch.applied;
            applied.objects += report_of_patch.added.len()
                + report_of_patch.replaced.len()
                + report_of_patch.deleted.len();
            for skipped in report_of_patch.skipped {
                let detail = skipped.error.to_string();
                report(
                    ApplyDiagnosticKind::OverrideRecordSkipped,
                    Some(SkippedRecord {
                        index: skipped.index,
                        object: skipped.object_hash,
                        property: skipped.path.as_str().to_owned(),
                        reason: RecordSkipReason::from(&skipped.error),
                    }),
                    Some(detail),
                );
            }
        }
        let outcome = entries::run(&mut bin, schema, &references, &edit.entries);
        applied.properties += outcome.properties;
        diagnostics.extend(outcome.reports.into_iter().map(|report| ApplyDiagnostic {
            kind: report.kind,
            edit_index: index,
            path: report.path,
            record: None,
            property: report.property,
            detail: report.detail,
        }));
        for path in &edit.links.remove {
            let count = bin.dependencies.len();
            bin.dependencies
                .retain(|value| !value.eq_ignore_ascii_case(path.as_str()));
            let removed = count - bin.dependencies.len();
            applied.links_removed += removed;
            if removed == 0 {
                diagnostics.push(ApplyDiagnostic {
                    kind: ApplyDiagnosticKind::LinkRemovalUnmatched,
                    edit_index: index,
                    path: path.as_str().to_owned(),
                    record: None,
                    property: None,
                    detail: None,
                });
            }
        }
        let mut seen: HashSet<_> = bin
            .dependencies
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        for path in &edit.links.add {
            if seen.insert(path.as_str().to_ascii_lowercase()) {
                bin.dependencies.push(path.as_str().to_owned());
                applied.links_added += 1;
            }
        }
    }
    // Only an edit that reached the object tree costs the re-encode, which writes PROP v3
    // and so changes the version of a v2 base. A link edit is a header edit, and an override
    // file whose every record skipped reached nothing.
    let tree_changed = applied.records > 0 || applied.objects > 0 || applied.properties > 0;
    let bytes = if tree_changed {
        let mut cursor = Cursor::new(Vec::new());
        bin.to_writer(&mut cursor).map_err(|e| bin_error(&e))?;
        cursor.into_inner()
    } else {
        header_rewrite(base, body_offset, &bin.dependencies)?
    };
    Ok(ApplyResult {
        bytes,
        dependencies: bin.dependencies,
        applied,
        diagnostics,
    })
}

/// The error of a base or output `ltk_meta` refuses.
fn bin_error(error: &dyn std::fmt::Display) -> Error {
    Error::new(ErrorKind::Bin {
        detail: error.to_string(),
    })
}

/// The base with its dependency header replaced and its object table copied byte for byte.
fn header_rewrite(
    base: &[u8],
    body_offset: usize,
    dependencies: &[String],
) -> Result<Vec<u8>, Error> {
    let count = u32::try_from(dependencies.len())
        .map_err(|_| Error::at_key(ErrorKind::DependencyOverflow, "links"))?;
    let mut bytes = base[..8].to_vec();
    bytes.extend_from_slice(&count.to_le_bytes());
    for path in dependencies {
        let length = u16::try_from(path.len())
            .map_err(|_| Error::at_key(ErrorKind::DependencyOverflow, "links"))?;
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(path.as_bytes());
    }
    bytes.extend_from_slice(&base[body_offset..]);
    Ok(bytes)
}
