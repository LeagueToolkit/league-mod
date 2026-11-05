use std::io::{self, Cursor};
use std::{fs, path::Path};

use camino::Utf8Path;
use ltk_ritobin::Cst;

/// Failure to turn a ritobin text source into `.bin` bytes.
#[derive(Debug, thiserror::Error)]
pub enum RitobinTransformError {
    #[error("Failed to read {path}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },

    /// The text does not fit the ritobin grammar.
    #[error("Syntax errors in ritobin source:\n{}", .0.join("\n"))]
    Syntax(Vec<String>),

    /// The text parses but does not type-check into a valid bin.
    #[error("Invalid ritobin source:\n{}", .0.join("\n"))]
    Invalid(Vec<String>),

    /// The built tree could not be encoded as a binary bin.
    #[error("Failed to encode .bin: {0}")]
    Encode(String),
}

/// Whether `path` names a ritobin text source (`.py` or `.ritobin`,
/// case-insensitive).
pub fn is_ritobin_source(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("ritobin")
        })
}

/// The archive path a ritobin source lands at: same path, `.bin` extension.
/// Non-ritobin paths are returned unchanged. Uses `/` separators like
/// `PlannedFile::rel_path`.
pub fn bin_rel_path(rel_path: &str) -> String {
    if !is_ritobin_source(rel_path) {
        return rel_path.to_owned();
    }
    match rel_path.rsplit_once('.') {
        Some((stem, _)) => format!("{stem}.bin"),
        None => rel_path.to_owned(),
    }
}

/// Parse ritobin text and encode it as `.bin` bytes.
/// Any syntax or type diagnostic fails the transform: a pack must not ship a
/// best-effort bin the author never meant.
pub fn transform_text(text: &str) -> Result<Vec<u8>, RitobinTransformError> {
    let cst = Cst::parse(text);
    if !cst.errors.is_empty() {
        return Err(RitobinTransformError::Syntax(
            cst.errors.iter().map(|e| format!("{e:?}")).collect(),
        ));
    }

    let bin = cst.build_bin(text).into_result().map_err(|partial| {
        RitobinTransformError::Invalid(
            partial
                .diagnostics
                .iter()
                .map(|d| format!("{d:?}"))
                .collect(),
        )
    })?;

    let mut out = Cursor::new(Vec::new());
    bin.to_writer(&mut out)
        .map_err(|e| RitobinTransformError::Encode(e.to_string()))?;
    Ok(out.into_inner())
}

/// Read a ritobin source from disk and transform it.
pub fn transform_file(source: &Utf8Path) -> Result<Vec<u8>, RitobinTransformError> {
    let text = fs::read_to_string(source.as_std_path()).map_err(|e| {
        RitobinTransformError::Read {
            path: source.to_string(),
            source: e,
        }
    })?;
    transform_text(&text)
}