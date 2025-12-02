use std::io;

use thiserror::Error;

/// Errors that can occur during Fantome extraction.
#[derive(Error, Debug)]
pub enum FantomeExtractError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("WAD error: {0}")]
    Wad(#[from] ltk_wad::WadError),

    #[error("Fantome package contains unsupported RAW/ directory")]
    RawUnsupported,

    #[error("Missing info.json metadata file")]
    MissingMetadata,
}
