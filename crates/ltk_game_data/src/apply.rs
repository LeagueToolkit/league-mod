use std::{collections::HashSet, io::Cursor};

use ltk_meta::concrete::BinStream;
use serde::{Deserialize, Serialize};

use crate::{Edit, Error};

/// The category of an application diagnostic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub enum ApplyDiagnosticKind {
    LinkRemovalUnmatched,
    /// A missing or unrecognized serialized category.
    #[default]
    #[serde(other)]
    Unknown,
}

/// An application diagnostic. The edit index is zero-based.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyDiagnostic {
    #[serde(default)]
    pub kind: ApplyDiagnosticKind,
    #[serde(rename = "step")]
    pub edit_index: usize,
    pub path: String,
}

#[derive(Debug)]
pub struct ApplyResult {
    pub bytes: Vec<u8>,
    pub dependencies: Vec<String>,
    pub diagnostics: Vec<ApplyDiagnostic>,
}

/// Applies ordered edits to a PROP v2 or v3. Untouched object bytes pass through.
///
/// Each edit runs its phases in field order and reads the result of the preceding edit.
pub fn apply(base: &[u8], edits: &[Edit]) -> Result<ApplyResult, Error> {
    let stream = BinStream::mount(Cursor::new(base)).map_err(|e| Error::new("target", e))?;
    if !matches!(stream.version(), 2 | 3) {
        return Err(Error::new("target", "expected PROP version 2 or 3"));
    }
    let mut dependencies = stream.dependencies().to_vec();
    let body_offset = 12
        + dependencies
            .iter()
            .map(|path| 2 + path.len())
            .sum::<usize>();
    // Validate the object table without rewriting its bytes.
    stream.into_bin().map_err(|e| Error::new("target", e))?;
    let mut seen = HashSet::new();
    dependencies.retain(|path| seen.insert(path.as_str().to_ascii_lowercase()));
    let mut diagnostics = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
        for path in &edit.links.remove {
            let count = dependencies.len();
            dependencies.retain(|value| !value.eq_ignore_ascii_case(path.as_str()));
            if dependencies.len() == count {
                diagnostics.push(ApplyDiagnostic {
                    kind: ApplyDiagnosticKind::LinkRemovalUnmatched,
                    edit_index: index,
                    path: path.as_str().to_owned(),
                });
            }
        }
        let mut seen: HashSet<_> = dependencies
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        for path in &edit.links.add {
            if seen.insert(path.as_str().to_ascii_lowercase()) {
                dependencies.push(path.as_str().to_owned());
            }
        }
    }
    let count = u32::try_from(dependencies.len()).map_err(|e| Error::new("links", e))?;
    let mut bytes = base[..8].to_vec();
    bytes.extend_from_slice(&count.to_le_bytes());
    for path in &dependencies {
        bytes.extend_from_slice(&(path.len() as u16).to_le_bytes());
        bytes.extend_from_slice(path.as_bytes());
    }
    bytes.extend_from_slice(&base[body_offset..]);
    Ok(ApplyResult {
        bytes,
        dependencies,
        diagnostics,
    })
}
