use std::{collections::HashSet, io::Cursor};

use ltk_meta::{
    BinOverride,
    concrete::{Bin, BinStream},
    path::{PatchError, ResolveErrorKind},
};
use serde::{Deserialize, Serialize};

use crate::{BinHash, Edit, Error, ErrorKind, OverridePath};

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
    LinkRemovalUnmatched,
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

impl From<&PatchError> for RecordSkipReason {
    fn from(error: &PatchError) -> Self {
        match error {
            PatchError::Resolve(error) => match error.kind() {
                ResolveErrorKind::MissingObject(_) => Self::MissingObject,
                ResolveErrorKind::MissingProperty(_) => Self::MissingProperty,
                ResolveErrorKind::NullPointer => Self::NullPointer,
                ResolveErrorKind::CannotDescend(_) => Self::CannotDescend,
                ResolveErrorKind::NotIndexable(_) => Self::NotIndexable,
                ResolveErrorKind::IndexOutOfRange { .. } => Self::IndexOutOfRange,
                ResolveErrorKind::InvalidKey(_) => Self::InvalidKey,
                ResolveErrorKind::KeyNotFound => Self::KeyNotFound,
                _ => Self::Unknown,
            },
            PatchError::TypeMismatch { .. } => Self::TypeMismatch,
            _ => Self::Unknown,
        }
    }
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
    /// The link path or override path the diagnostic is about.
    pub path: String,
    /// The record of an `OverrideRecordSkipped` diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<SkippedRecord>,
}

#[derive(Debug)]
pub struct ApplyResult {
    pub bytes: Vec<u8>,
    pub dependencies: Vec<String>,
    pub diagnostics: Vec<ApplyDiagnostic>,
}

/// Applies ordered edits to a PROP v2 or v3.
///
/// Each edit runs its phases in field order and reads the result of the preceding edit.
/// `read_override` supplies the bytes of an override file by its path, once per listed path
/// in apply order, in any byte container; a caller sharing one file across several targets
/// hands over an `Arc<[u8]>`. A target with an applied override file is written from the
/// decoded tree at PROP version 3; a target without one keeps its object bytes and header
/// version.
///
/// # Errors
///
/// The base is not a PROP version 2 or 3, or its object table does not decode.
pub fn apply<B: AsRef<[u8]>>(
    base: &[u8],
    edits: &[Edit],
    mut read_override: impl FnMut(&OverridePath) -> Result<B, Error>,
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
