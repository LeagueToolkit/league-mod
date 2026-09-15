use std::{collections::HashSet, io::Cursor};

use ltk_meta::concrete::BinStream;
use serde::{Deserialize, Serialize};

use crate::{Batch, Error};

/// One link removal whose path is absent. The step index is zero-based.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkReport {
    pub step: usize,
    pub path: String,
}

#[derive(Debug)]
pub struct Materialised {
    pub bytes: Vec<u8>,
    pub dependencies: Vec<String>,
    pub reports: Vec<LinkReport>,
}

/// Apply ordered batches to a PROP v2 or v3. Untouched object bytes pass through.
pub fn materialise(base: &[u8], steps: &[Batch]) -> Result<Materialised, Error> {
    for step in steps {
        step.validate()?;
    }
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
    dependencies.retain(|path| seen.insert(path.to_ascii_lowercase()));
    let mut reports = Vec::new();
    for (index, batch) in steps.iter().enumerate() {
        for path in &batch.remove_links {
            let count = dependencies.len();
            dependencies.retain(|value| !value.eq_ignore_ascii_case(path));
            if dependencies.len() == count {
                reports.push(LinkReport {
                    step: index,
                    path: path.clone(),
                });
            }
        }
        let mut seen: HashSet<_> = dependencies
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        for path in &batch.links {
            if seen.insert(path.to_ascii_lowercase()) {
                dependencies.push(path.clone());
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
    Ok(Materialised {
        bytes,
        dependencies,
        reports,
    })
}
