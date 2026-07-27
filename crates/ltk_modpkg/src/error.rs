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

/// Failure to read a file as text, carrying the path it failed on.
#[derive(Debug, Error)]
#[error("Failed to read {path}")]
pub struct ReadTextError {
    path: Utf8PathBuf,
    source: std::io::Error,
}

impl ReadTextError {
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
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("IO error: {0}")]
    IoExtError(#[from] ltk_io_ext::ReaderError),
    #[error("Binrw error: {0}")]
    BinrwError(#[from] binrw::Error),

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
    #[error("Invalid modpkg version: {0}")]
    InvalidVersion(u32),
    #[error("Duplicate chunk: {0}")]
    DuplicateChunk(u64),
    #[error("Chunk not found: {0:x}")]
    MissingChunk(u64),
    #[error("Invalid meta chunk: must not belong to any layer or wad")]
    InvalidMetaChunk,

    #[error("Msgpack decode error: {0}")]
    MsgpackDecode(#[from] rmp_serde::decode::Error),
    #[error("Msgpack encode error: {0}")]
    MsgpackEncode(#[from] rmp_serde::encode::Error),
}
