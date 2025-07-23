use thiserror::Error;

use crate::ModpkgCompression;

#[derive(Error, Debug)]
pub enum ModpkgError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("IO error: {0}")]
    IoExtError(#[from] io_ext::ReaderError),
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

    #[error("Msgpack decode error: {0}")]
    MsgpackDecode(#[from] rmp_serde::decode::Error),
    #[error("Msgpack encode error: {0}")]
    MsgpackEncode(#[from] rmp_serde::encode::Error),
}
