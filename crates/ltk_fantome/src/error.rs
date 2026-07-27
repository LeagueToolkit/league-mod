//! Errors returned when packing, extracting, and loading Fantome data.
//!
//! Packing and extracting have separate types: neither can produce the other's
//! failures, so a merged enum would force matches on unreachable cases.

use camino::Utf8PathBuf;
use thiserror::Error;

/// Failure to pack a mod project into a Fantome archive.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FantomePackError {
    /// The project has no `content/base` directory, so there is nothing to pack.
    #[error("No base layer to pack: {0} does not exist")]
    MissingBaseLayer(Utf8PathBuf),

    /// A file in the project could not be read.
    #[error("Failed to read {path}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The archive could not be written.
    #[error("Failed to write the archive")]
    Zip(#[from] zip::result::ZipError),

    /// An IO failure with no single project file to blame.
    #[error("IO error")]
    Io(#[from] std::io::Error),

    /// `META/info.json` could not be produced from the project.
    #[error("Failed to serialize META/info.json")]
    Json(#[from] serde_json::Error),

    /// The thumbnail could not be read, or re-encoded as the PNG Fantome stores.
    #[error("Failed to convert the thumbnail {path}")]
    Thumbnail {
        path: Utf8PathBuf,
        #[source]
        source: Box<image::ImageError>,
    },
}

impl FantomePackError {
    pub(crate) fn read(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }
}

/// Failure to extract a Fantome archive.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FantomeExtractError {
    /// A file could not be written to the output directory.
    #[error("Failed to write {path}")]
    Write {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Reading from the archive failed.
    #[error("IO error")]
    Io(#[from] std::io::Error),

    /// The archive could not be read.
    #[error("Failed to read the archive")]
    Zip(#[from] zip::result::ZipError),

    /// `META/info.json` is not valid JSON, or does not describe a mod.
    #[error("Failed to parse META/info.json")]
    Json(#[from] serde_json::Error),

    /// The extracted project's `mod.config.json` could not be written.
    #[error("Failed to write the project config")]
    Config(#[from] ltk_mod_project::ModProjectError),

    /// A packed WAD inside the archive could not be extracted.
    #[error("Failed to extract a WAD")]
    Wad(#[from] ltk_wad::WadError),

    /// The archive has no `META/info.json`.
    #[error("Missing info.json metadata file")]
    MissingMetadata,

    /// `META/image.png` could not be decoded, or re-encoded as the project
    /// thumbnail.
    #[error("Failed to convert the thumbnail")]
    Thumbnail(#[source] Box<image::ImageError>),
}

impl FantomeExtractError {
    pub(crate) fn write(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Write {
            path: path.into(),
            source,
        }
    }
}

/// Failure to load a WAD hashtable.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WadHashtableError {
    /// A hashtable file could not be read.
    #[error("Failed to read {path}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl WadHashtableError {
    pub(crate) fn read(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }
}
