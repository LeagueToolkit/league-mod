use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

use crate::ModpkgCompression;

/// A value that could not be represented in the encoding this crate requires.
///
/// Paths are the common case: the OS hands back arbitrary bytes, while
/// `.modpkg` stores every path as UTF-8.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EncodingError {
    /// A path from an OS API that is not valid UTF-8.
    ///
    /// Carries the path rendered lossily, which is the only form left that can
    /// go in a message.
    #[error("Invalid UTF-8 path: {0}")]
    NonUtf8Path(String),
}

/// A value that is not a valid [`Slug`](crate::Slug).
#[derive(Debug, Error)]
#[error("Invalid slug: {value}")]
pub struct InvalidSlugError {
    value: String,
}

impl InvalidSlugError {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    /// The value that was rejected.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Failure to read a file, carrying the path it failed on.
#[derive(Debug, Error)]
#[error("Failed to read {path}")]
pub struct ReadFileError {
    path: Utf8PathBuf,
    source: std::io::Error,
}

impl ReadFileError {
    pub(crate) fn new(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self {
            path: path.into(),
            source,
        }
    }

    /// The file that could not be read.
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// The underlying IO failure.
    pub fn io_error(&self) -> &std::io::Error {
        &self.source
    }
}

/// A path that does not start with the base it was stripped against.
#[derive(Debug, Error)]
#[error("{path} is not inside {base}")]
pub struct StripPrefixError {
    path: Utf8PathBuf,
    base: Utf8PathBuf,
}

impl StripPrefixError {
    pub(crate) fn new(path: impl Into<Utf8PathBuf>, base: impl Into<Utf8PathBuf>) -> Self {
        Self {
            path: path.into(),
            base: base.into(),
        }
    }

    /// The path that was stripped.
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// The base it was stripped against.
    pub fn base(&self) -> &Utf8Path {
        &self.base
    }
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ModpkgError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Reader(#[from] ltk_io_ext::ReaderError),

    /// The archive's binary layout could not be parsed.
    ///
    /// The underlying parser error is boxed rather than named, so the parser
    /// this crate happens to use is not part of its public API.
    #[error("Malformed modpkg")]
    Malformed(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Invalid modpkg header size: {header_size}, actual size: {actual_size}")]
    InvalidHeaderSize { header_size: u32, actual_size: u64 },
    #[error("Chunks are not in ascending order: previous: {previous}, current: {current}")]
    UnsortedChunks { previous: u64, current: u64 },
    #[error("Missing metadata chunk")]
    MissingMetadata,
    #[error("Missing base layer")]
    MissingBaseLayer,
    #[error("Invalid modpkg compression type: {0}")]
    InvalidCompressionType(u8),
    #[error(
        "Unexpected compression type: chunk: {chunk:x}, expected: {expected}, actual: {actual}"
    )]
    UnexpectedCompressionType {
        chunk: u64,
        expected: ModpkgCompression,
        actual: ModpkgCompression,
    },
    #[error("Invalid modpkg license type: {0}")]
    InvalidLicenseType(u8),
    #[error("Invalid modpkg magic: {0}")]
    InvalidMagic(u64),
    /// The container format version, not the mod's own version.
    #[error("Unsupported modpkg format version: {0}")]
    UnsupportedFormatVersion(u32),
    #[error("Duplicate chunk: {0}")]
    DuplicateChunk(u64),
    #[error("Chunk not found: {0:x}")]
    MissingChunk(u64),
    #[error("Invalid meta chunk: must not belong to any layer or wad")]
    InvalidMetaChunk,

    #[error(transparent)]
    MsgpackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgpackEncode(#[from] rmp_serde::encode::Error),
}

// Kept as a `From` impl so `?` still works across the read path. The conversion
// names `binrw::Error`, but the variant does not, so matching on a decode
// failure never forces a caller to depend on binrw.
impl From<binrw::Error> for ModpkgError {
    fn from(error: binrw::Error) -> Self {
        Self::Malformed(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pass-through variants are `#[error(transparent)]` so that a caller which
    /// prints only the top-level error still sees the cause. Wrapping them in a
    /// message like "IO error" hides it from anyone not walking the chain.
    #[test]
    fn pass_through_display_carries_the_cause() {
        let error = ModpkgError::from(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the file is gone",
        ));

        assert!(error.to_string().contains("the file is gone"), "{error}");
    }

    /// A variant that adds context keeps its own message, and must not also
    /// inline the source, or a chain walker prints it twice.
    #[test]
    fn contextual_display_does_not_embed_its_source() {
        let error = ModpkgError::Malformed(Box::new(std::io::Error::other("inner detail")));
        let source = std::error::Error::source(&error).unwrap().to_string();

        assert!(!error.to_string().contains(&source), "{error}");
    }
}
