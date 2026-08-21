use thiserror::Error;

use crate::PathHash;

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
    #[error("Missing metadata chunk")]
    MissingMetadata,
    #[error("Missing base layer")]
    MissingBaseLayer,
    #[error("Invalid modpkg compression type: {0}")]
    InvalidCompressionType(u8),
    #[error("Invalid modpkg license type: {0}")]
    InvalidLicenseType(u8),
    #[error("Invalid modpkg magic: {0}")]
    InvalidMagic(u64),
    /// The container format version, not the mod's own version.
    #[error("Unsupported modpkg format version: {0}")]
    UnsupportedFormatVersion(u32),
    /// Inconsistent chunk content
    #[error("Inconsistent duplicate chunk: {0}")]
    ChunksInconsistent(PathHash),
    #[error("Chunk not found: {0}")]
    MissingChunk(PathHash),
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
