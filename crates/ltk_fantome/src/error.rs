//! Errors returned when packing, extracting, and loading Fantome data.
//!
//! Packing and extracting have separate error types rather than one shared
//! enum: a caller that packs can never see `MissingMetadata`, and a caller that
//! extracts can never see `MissingBaseLayer`, so a merged type would force
//! every match to handle cases that cannot happen.
//!
//! No variant's `Display` repeats what its source says. Printing the source is
//! the job of whoever walks the error chain, and a message that inlines it
//! appears twice.

use camino::Utf8PathBuf;
use thiserror::Error;

/// Failure to pack a mod project into a Fantome archive.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FantomePackError {
    /// The project has no `content/base` directory.
    ///
    /// Fantome archives carry exactly one layer, so a project without a base
    /// layer has nothing to pack.
    #[error("No base layer to pack: {0} does not exist")]
    MissingBaseLayer(Utf8PathBuf),

    /// A file in the project could not be read.
    #[error("Failed to read {path}")]
    Read {
        /// The file that could not be read.
        path: Utf8PathBuf,
        /// The underlying IO failure.
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

    /// The thumbnail could not be read or re-encoded as PNG.
    ///
    /// Fantome stores thumbnails as PNG, so a project thumbnail in another
    /// format is converted on the way in.
    #[error("Failed to convert the thumbnail {path}")]
    Thumbnail {
        /// The image that could not be converted.
        path: Utf8PathBuf,
        /// The decode or encode failure.
        #[source]
        source: Box<image::ImageError>,
    },

    /// A path inside the project is not valid UTF-8.
    ///
    /// Archive entry names are UTF-8, so a file the OS reports as arbitrary
    /// bytes cannot be given a name in the archive. Carries the path rendered
    /// lossily, which is the only form left that can go in a message.
    #[error("Invalid UTF-8 path: {0}")]
    NonUtf8Path(String),
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
        /// The file that could not be written.
        path: Utf8PathBuf,
        /// The underlying IO failure.
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
///
/// Separate from [`FantomeExtractError`] because loading a hashtable is not
/// extraction: a caller can build one without ever opening an archive, and had
/// to handle unreachable extraction failures to do it.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WadHashtableError {
    /// A hashtable file could not be read.
    #[error("Failed to read {path}")]
    Read {
        /// The file that could not be read.
        path: Utf8PathBuf,
        /// The underlying IO failure.
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
