//! Errors returned when loading and saving a mod project configuration.

use camino::Utf8PathBuf;
use thiserror::Error;

use crate::ConfigFormat;

/// Failure to load or save a mod project configuration.
///
/// Every variant that touches the filesystem carries the path it failed on. A
/// project directory can hold more than one config file, and a bare
/// "permission denied" leaves the author guessing which.
///
/// The `Display` of each variant states what was being done, not what the
/// source said. Printing the source is the job of whoever walks the error
/// chain, and a variant that inlines it makes the message appear twice.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ModProjectError {
    /// The directory holds neither `mod.config.json` nor `mod.config.toml`.
    #[error("No config file in {0} (expected mod.config.json or mod.config.toml)")]
    ConfigNotFound(Utf8PathBuf),

    /// A config file could not be read or written.
    #[error("Failed to access {path}")]
    Io {
        /// The file the operation failed on.
        path: Utf8PathBuf,
        /// The underlying IO failure.
        #[source]
        source: std::io::Error,
    },

    /// A config file is not valid JSON, or does not describe a project.
    #[error("Failed to parse {path} as JSON")]
    Json {
        /// The file that could not be parsed.
        path: Utf8PathBuf,
        /// The parse failure, carrying the line and column it stopped at.
        #[source]
        source: serde_json::Error,
    },

    /// A config file is not valid TOML, or does not describe a project.
    #[error("Failed to parse {path} as TOML")]
    Toml {
        /// The file that could not be parsed.
        path: Utf8PathBuf,
        /// The parse failure, carrying the span it stopped at.
        ///
        /// Boxed because it is 96 bytes against 8 for every other source here,
        /// and it would otherwise set the size of every `Result` this crate
        /// returns.
        #[source]
        source: Box<toml::de::Error>,
    },

    /// The project could not be serialized.
    #[error("Failed to serialize the project as {format}")]
    Serialize {
        /// The format that was being written.
        format: ConfigFormat,
        /// The serialization failure.
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
/// Serialization fails only on data the format cannot represent, so in practice
/// this means a TOML value ordering problem rather than anything an author
/// typed.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SerializeError {
    /// The project could not be written as JSON.
    #[error("JSON serialization failed")]
    Json(#[from] serde_json::Error),

    /// The project could not be written as TOML.
    #[error("TOML serialization failed")]
    Toml(#[from] toml::ser::Error),
}
