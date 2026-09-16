//! Application of edits over a `PROP`: override files, entry edits, then link edits.

mod coerce;
mod entries;

use std::{collections::HashSet, io::Cursor};

use ltk_meta::{
    BinOverride,
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
    /// A reason this crate does not name.
    #[default]
    #[serde(other)]
    Unknown,
}

impl From<RecordSkipReason> for PropertySkipReason {
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
            _ => Self::Unknown,
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
}

/// The outcome of one application: the target's bytes, its dependency list, and every
/// nonfatal outcome.
#[derive(Debug)]
pub struct ApplyResult {
    /// The target after every edit, a `PROP`.
    pub bytes: Vec<u8>,
    /// The dependency spellings of `bytes`, retained base entries included.
    pub dependencies: Vec<String>,
    /// Every diagnostic in apply order.
    pub diagnostics: Vec<ApplyDiagnostic>,
}

/// Applies ordered edits to a PROP v2 or v3.
///
/// Each edit runs its phases in field order and reads the result of the preceding edit.
/// `read_override` supplies the bytes of an override file by its path, once per listed path
/// in apply order, in any byte container; a caller sharing one file across several targets
/// hands over an `Arc<[u8]>`. `schema` types every property edit; a caller with no schema
/// passes `&NoSchema`. A target with an applied override file or an applied property edit is
/// written from the decoded tree at PROP version 3; a target with neither keeps its object
/// bytes and header version.
///
/// # Errors
///
/// The base is not a PROP version 2 or 3, or its object table does not decode.
pub fn apply<B: AsRef<[u8]>>(
    base: &[u8],
    edits: &[Edit],
    mut read_override: impl FnMut(&OverridePath) -> Result<B, Error>,
    schema: &dyn Schema,
) -> Result<ApplyResult, Error> {
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
    let mut rewritten = false;
    for (index, edit) in edits.iter().enumerate() {
        for path in &edit.overrides {
            let mut report = |kind, record| {
                diagnostics.push(ApplyDiagnostic {
                    kind,
                    edit_index: index,
                    path: path.as_str().to_owned(),
                    record,
                    property: None,
                });
            };
            let Ok(bytes) = read_override(path) else {
                report(ApplyDiagnosticKind::OverrideUnreadable, None);
                continue;
            };
            let Ok(patch) = BinOverride::from_reader(&mut Cursor::new(bytes.as_ref())) else {
                report(ApplyDiagnosticKind::OverrideInvalid, None);
                continue;
            };
            let applied = patch.apply(&mut bin);
            for skipped in applied.skipped {
                report(
                    ApplyDiagnosticKind::OverrideRecordSkipped,
                    Some(SkippedRecord {
                        index: skipped.index,
                        object: skipped.object_hash,
                        property: skipped.path.as_str().to_owned(),
                        reason: RecordSkipReason::from(&skipped.error),
                    }),
                );
            }
            rewritten = true;
        }
        let outcome = entries::run(&mut bin, schema, &edit.entries);
        rewritten |= outcome.patched;
        diagnostics.extend(outcome.reports.into_iter().map(|report| ApplyDiagnostic {
            kind: report.kind,
            edit_index: index,
            path: report.path,
            record: None,
            property: report.property,
        }));
        for path in &edit.links.remove {
            let count = bin.dependencies.len();
            bin.dependencies
                .retain(|value| !value.eq_ignore_ascii_case(path.as_str()));
            if bin.dependencies.len() == count {
                diagnostics.push(ApplyDiagnostic {
                    kind: ApplyDiagnosticKind::LinkRemovalUnmatched,
                    edit_index: index,
                    path: path.as_str().to_owned(),
                    record: None,
                    property: None,
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
            }
        }
    }
    let bytes = if rewritten {
        let mut cursor = Cursor::new(Vec::new());
        bin.to_writer(&mut cursor).map_err(|e| bin_error(&e))?;
        cursor.into_inner()
    } else {
        header_rewrite(base, body_offset, &bin.dependencies)?
    };
    Ok(ApplyResult {
        bytes,
        dependencies: bin.dependencies,
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
