//! Ordered game-data declarations shared by projects, archives, and overlay consumers.

mod apply;
mod authoring;
mod discovery;

pub use apply::{ApplyDiagnostic, ApplyDiagnosticKind, ApplyResult, apply};
pub use authoring::{MANIFEST_NAMES, load_declarations, referenced_sources};

use serde::{Deserialize, Serialize};

/// A declaration that cannot be loaded or applied.
#[derive(Debug, thiserror::Error)]
#[error("{location}: {message}")]
pub struct Error {
    pub location: String,
    pub message: String,
}

impl Error {
    pub fn new(location: impl Into<String>, message: impl ToString) -> Self {
        Self {
            location: location.into(),
            message: message.to_string(),
        }
    }
}

/// An archive's versioned declarations, including fields an older consumer cannot execute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeclarationDocument(serde_json::Value);

impl DeclarationDocument {
    /// Validates the complete layer declarations before execution.
    pub fn parse(&self) -> Result<Declarations, Error> {
        let declarations: Declarations = serde_json::from_value(self.0.clone())
            .map_err(|error| Error::new("game_data", error))?;
        declarations.validate()?;
        Ok(declarations)
    }
}

impl From<Declarations> for DeclarationDocument {
    fn from(declarations: Declarations) -> Self {
        Self(
            serde_json::to_value(declarations)
                .expect("declarations contain JSON-compatible fields"),
        )
    }
}

/// A layer's modules in execution order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declarations {
    pub version: u32,
    pub modules: Vec<Module>,
}

impl Declarations {
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::new("game_data", "unsupported declarations version"));
        }
        Ok(())
    }

    /// Reconstructs a direct JSON authoring manifest.
    pub fn manifest_json(&self) -> Result<String, Error> {
        self.validate()?;
        let modules: Vec<_> = self
            .modules
            .iter()
            .map(|module| serde_json::json!({"target": module.target, "steps": module.steps}))
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({"version": 1, "modules": modules}))
            .map_err(|error| Error::new("game_data", error))
    }
}

/// One target and its ordered steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub target: Target,
    pub steps: Vec<Step>,
    #[serde(rename = "origin")]
    pub location: DeclarationLocation,
}

/// An authored location. Indices are zero-based.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclarationLocation {
    pub manifest: String,
    pub source: Option<String>,
    #[serde(rename = "module")]
    pub module_index: usize,
}

/// A nonempty game lookup path or bare 16-digit hexadecimal chunk hash.
/// Construction classifies the spelling and preserves it verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Target(TargetKind);

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetKind {
    Path(String),
    Hash(String),
}

impl TryFrom<String> for Target {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty() {
            return Err(Error::new("target", "expected a nonempty target string"));
        }
        Ok(Self(
            if value.len() == 16 && value.bytes().all(|c| c.is_ascii_hexdigit()) {
                TargetKind::Hash(value)
            } else {
                TargetKind::Path(value)
            },
        ))
    }
}

impl TryFrom<&str> for Target {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        Self::try_from(value.to_owned())
    }
}

impl From<Target> for String {
    fn from(target: Target) -> Self {
        match target.0 {
            TargetKind::Path(value) | TargetKind::Hash(value) => value,
        }
    }
}

impl Target {
    /// The game's WAD chunk identifier.
    pub fn chunk_hash(&self) -> u64 {
        match &self.0 {
            TargetKind::Path(path) => path_hash(path),
            TargetKind::Hash(hash) => u64::from_str_radix(hash, 16).expect("validated hex"),
        }
    }

    /// The identifier's authored spelling.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            TargetKind::Path(value) | TargetKind::Hash(value) => value,
        }
    }
}

/// An authored dependency path containing 1 to 65535 UTF-8 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LinkPath(String);

impl TryFrom<String> for LinkPath {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty() || value.len() > u16::MAX as usize {
            return Err(Error::new(
                "links",
                "link paths require 1 to 65535 UTF-8 bytes",
            ));
        }
        Ok(Self(value))
    }
}

impl TryFrom<&str> for LinkPath {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        Self::try_from(value.to_owned())
    }
}

impl From<LinkPath> for String {
    fn from(path: LinkPath) -> Self {
        path.0
    }
}

impl LinkPath {
    /// The path's authored spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Link removals followed by link additions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    #[serde(
        default,
        rename = "links",
        alias = "+links",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub add_links: Vec<LinkPath>,
    #[serde(default, rename = "-links", skip_serializing_if = "Vec::is_empty")]
    pub remove_links: Vec<LinkPath>,
}

/// The game's chunk-path hash. File suffixes and separators are preserved.
pub fn path_hash(path: &str) -> u64 {
    xxhash_rust::xxh64::xxh64(path.to_ascii_lowercase().as_bytes(), 0)
}
