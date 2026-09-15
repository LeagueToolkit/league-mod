use std::collections::HashSet;

use camino::Utf8Path;
use serde::{Deserialize, de::DeserializeOwned};

use crate::{Batch, Error, Module, Origin, Program, Target};

pub const MANIFEST_NAMES: [&str; 4] = [
    "game_data.yaml",
    "game_data.yml",
    "game_data.toml",
    "game_data.json",
];

/// Discover source references independently of binding validation.
pub fn referenced_sources(name: &str, text: &str) -> Result<Vec<String>, Error> {
    let document: super::discovery::Node = parse(name, text, false)?;
    Ok(document.sources())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    modules: Vec<AuthoredModule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredModule {
    target: Target,
    source: Option<String>,
    steps: Option<Vec<Batch>>,
    #[serde(alias = "+links")]
    links: Option<Vec<String>>,
    #[serde(rename = "-links")]
    remove_links: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    version: u32,
    steps: Option<Vec<Batch>>,
    #[serde(alias = "+links")]
    links: Option<Vec<String>>,
    #[serde(rename = "-links")]
    remove_links: Option<Vec<String>>,
}

fn parse<T: DeserializeOwned>(name: &str, text: &str, strict: bool) -> Result<T, Error> {
    match Utf8Path::new(name).extension() {
        Some("yaml" | "yml") => {
            let mut options = serde_saphyr::Options::default();
            options.strict_booleans = true;
            options.no_schema = true;
            options.reject_unsupported_tags = strict;
            options.duplicate_keys = if strict {
                serde_saphyr::DuplicateKeyPolicy::Error
            } else {
                serde_saphyr::DuplicateKeyPolicy::LastWins
            };
            serde_saphyr::from_str_with_options(text, options).map_err(|e| Error::new(name, e))
        }
        Some("json") => serde_json::from_str(text).map_err(|e| Error::new(name, e)),
        Some("toml") => toml::from_str(text).map_err(|e| Error::new(name, e)),
        _ => Err(Error::new(name, "expected YAML, TOML, or JSON")),
    }
}

fn version(name: &str, value: u32) -> Result<(), Error> {
    if value == 1 {
        Ok(())
    } else {
        Err(Error::new(name, "unsupported declaration version"))
    }
}

fn body(
    name: &str,
    steps: Option<Vec<Batch>>,
    links: Option<Vec<String>>,
    remove_links: Option<Vec<String>>,
) -> Result<Vec<Batch>, Error> {
    if let Some(steps) = steps {
        if links.is_some() || remove_links.is_some() {
            return Err(Error::new(
                name,
                "steps and compact bindings are mutually exclusive",
            ));
        }
        return Ok(steps);
    }
    if links.is_none() && remove_links.is_none() {
        return Err(Error::new(
            name,
            "module requires bindings, steps, or source",
        ));
    }
    Ok(vec![Batch {
        links: links.unwrap_or_default(),
        remove_links: remove_links.unwrap_or_default(),
    }])
}

/// Compile a manifest and its sources. The reader resolves paths relative to the layer
/// and enforces containment and input classification.
pub fn compile(
    manifest_name: &str,
    text: &str,
    mut read_source: impl FnMut(&str) -> Result<String, Error>,
) -> Result<Program, Error> {
    let manifest: Manifest = parse(manifest_name, text, true)?;
    version(manifest_name, manifest.version)?;
    let mut assignments = HashSet::new();
    let mut modules = Vec::new();
    for (index, module) in manifest.modules.into_iter().enumerate() {
        let hash = module.target.hash()?;
        let location = format!("{manifest_name}: module {index}");
        let steps = if let Some(source) = &module.source {
            if module.steps.is_some() || module.links.is_some() || module.remove_links.is_some() {
                return Err(Error::new(
                    &location,
                    "source and local bindings are mutually exclusive",
                ));
            }
            if !assignments.insert((hash, source.to_ascii_lowercase())) {
                return Err(Error::new(&location, "duplicate target/source assignment"));
            }
            let text = read_source(source).map_err(|error| Error::new(&location, error))?;
            let source_body: Source =
                parse(source, &text, true).map_err(|error| Error::new(&location, error))?;
            version(source, source_body.version)?;
            body(
                source,
                source_body.steps,
                source_body.links,
                source_body.remove_links,
            )?
        } else {
            body(&location, module.steps, module.links, module.remove_links)?
        };
        modules.push(Module {
            target: module.target,
            steps,
            origin: Origin {
                manifest: manifest_name.to_owned(),
                source: module.source,
                module: index,
            },
        });
    }
    let program = Program {
        version: 1,
        modules,
    };
    program.validate()?;
    Ok(program)
}
