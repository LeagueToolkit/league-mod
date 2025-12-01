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

    #[error("Fantome package contains unsupported RAW/ directory")]
    RawUnsupported,

    #[error("Fantome package contains packed WAD file: {wad_name}")]
    PackedWadUnsupported { wad_name: String },

    #[error("Missing info.json metadata file")]
    MissingMetadata,
}
