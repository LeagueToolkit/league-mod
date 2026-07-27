//! Errors returned when loading and saving a mod project configuration.

use camino::Utf8PathBuf;
use thiserror::Error;

use crate::ConfigFormat;

/// Failure to load or save a mod project configuration.
///
/// Every variant that touches the filesystem carries the path it failed on.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ModProjectError {
    /// The directory holds neither `mod.config.json` nor `mod.config.toml`.
    #[error("No config file in {0} (expected mod.config.json or mod.config.toml)")]
    ConfigNotFound(Utf8PathBuf),

    /// A config file could not be read or written.
    #[error("Failed to access {path}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A config file is not valid JSON, or does not describe a project.
    #[error("Failed to parse {path} as JSON")]
    Json {
        path: Utf8PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// A config file is not valid TOML, or does not describe a project.
    #[error("Failed to parse {path} as TOML")]
    Toml {
        path: Utf8PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    /// The project could not be serialized.
    #[error("Failed to serialize the project as {format}")]
    Serialize {
        format: ConfigFormat,
        #[source]
        source: SerializeError,
    },

    /// The file extension names no format this crate can read.
    #[error("Unsupported config file extension: `{0}` (expected `json` or `toml`)")]
    UnsupportedExtension(String),
}

impl ModProjectError {
    pub(crate) fn io(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Failure to turn a project into config file text.
///
/// Only data the format cannot represent gets here, never anything an author
/// typed.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SerializeError {
    #[error("JSON serialization failed")]
    Json(#[from] serde_json::Error),

    #[error("TOML serialization failed")]
    Toml(#[from] toml::ser::Error),
}
