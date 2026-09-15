//! Ordered game-data declarations shared by projects, archives, and overlay consumers.

mod authoring;
mod discovery;
mod materialise;

pub use authoring::{MANIFEST_NAMES, compile, referenced_sources};
pub use materialise::{LinkReport, Materialised, materialise};

use serde::{Deserialize, Serialize};

/// A declaration that cannot be loaded or materialised.
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

/// An archive's versioned program, including fields an older consumer cannot execute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Document(serde_json::Value);

impl Document {
    /// Validate the complete layer program before execution.
    pub fn program(&self) -> Result<Program, Error> {
        let program: Program = serde_json::from_value(self.0.clone())
            .map_err(|error| Error::new("game_data", error))?;
        program.validate()?;
        Ok(program)
    }
}

impl From<Program> for Document {
    fn from(program: Program) -> Self {
        Self(serde_json::to_value(program).expect("program contains JSON-compatible fields"))
    }
}

/// A layer's compiled modules in execution order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub version: u32,
    pub modules: Vec<Module>,
}

impl Program {
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::new("game_data", "unsupported program version"));
        }
        for module in &self.modules {
            module.target.hash()?;
            for batch in &module.steps {
                batch.validate()?;
            }
        }
        Ok(())
    }

    /// Reconstruct a direct JSON authoring manifest.
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

/// One target and its ordered batches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub target: Target,
    pub steps: Vec<Batch>,
    pub origin: Origin,
}

/// An authored location. Indices are zero-based.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub manifest: String,
    pub source: Option<String>,
    pub module: usize,
}

/// A scalar game lookup path or bare 16-digit hexadecimal chunk hash.
/// Construction classifies the spelling and preserves it verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct Target(TargetKind);

#[derive(Debug, Clone, PartialEq)]
enum TargetKind {
    Path(String),
    Hash(String),
}

impl From<String> for Target {
    fn from(value: String) -> Self {
        Self(
            if value.len() == 16 && value.bytes().all(|c| c.is_ascii_hexdigit()) {
                TargetKind::Hash(value)
            } else {
                TargetKind::Path(value)
            },
        )
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
    pub fn hash(&self) -> Result<u64, Error> {
        match &self.0 {
            TargetKind::Path(path) if !path.is_empty() => Ok(path_hash(path)),
            TargetKind::Hash(hash) => Ok(u64::from_str_radix(hash, 16).expect("validated hex")),
            _ => Err(Error::new("target", "expected a nonempty target string")),
        }
    }

    pub fn display_name(&self) -> &str {
        match &self.0 {
            TargetKind::Path(value) | TargetKind::Hash(value) => value,
        }
    }
}

/// Link removals followed by link additions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    #[serde(default, alias = "+links", skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    #[serde(default, rename = "-links", skip_serializing_if = "Vec::is_empty")]
    pub remove_links: Vec<String>,
}

impl Batch {
    fn validate(&self) -> Result<(), Error> {
        for link in self.links.iter().chain(&self.remove_links) {
            if link.is_empty() || link.len() > u16::MAX as usize {
                return Err(Error::new(
                    "links",
                    "link paths require 1 to 65535 UTF-8 bytes",
                ));
            }
        }
        Ok(())
    }
}

/// The game's chunk-path hash. File suffixes and separators are preserved.
pub fn path_hash(path: &str) -> u64 {
    xxhash_rust::xxh64::xxh64(path.to_ascii_lowercase().as_bytes(), 0)
}
