use std::collections::HashSet;

use camino::Utf8Path;
use indexmap::IndexMap;
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, MapAccess, Visitor},
};

use crate::{
    BindingsWire, Declarations, Edit, EntryEdit, EntryName, Error, Module, Origin, Selector,
    SelectorKey, Target, one_selector,
};

pub const MANIFEST_NAMES: [&str; 4] = [
    "game_data.yaml",
    "game_data.yml",
    "game_data.toml",
    "game_data.json",
];

/// Discovers source references independently of binding validation.
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
    target: Option<Target>,
    entries: Option<AuthoredEntries>,
    source: Option<String>,
    steps: Option<Vec<BindingsWire>>,
    #[serde(flatten)]
    bindings: BindingsWire,
}

/// An entry body: compact bindings and nothing else.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredEntry {
    source: Option<String>,
    #[serde(flatten)]
    bindings: BindingsWire,
}

/// An `entries` mapping in authored order. A repeated name is an error in every format.
struct AuthoredEntries(IndexMap<EntryName, AuthoredEntry>);

impl<'de> Deserialize<'de> for AuthoredEntries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntriesVisitor;
        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = AuthoredEntries;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a mapping of entry names to bindings")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = IndexMap::new();
                while let Some((name, body)) = map.next_entry::<EntryName, AuthoredEntry>()? {
                    if entries.insert(name.clone(), body).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate entry name `{name}`"
                        )));
                    }
                }
                Ok(AuthoredEntries(entries))
            }
        }
        deserializer.deserialize_map(EntriesVisitor)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    version: u32,
    steps: Option<Vec<BindingsWire>>,
    #[serde(flatten)]
    bindings: BindingsWire,
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

/// The edits of a body: its `steps`, or its compact bindings as one edit. Every step
/// carries at least one binding.
fn body(
    name: &str,
    steps: Option<Vec<BindingsWire>>,
    bindings: BindingsWire,
) -> Result<Vec<Edit>, Error> {
    if let Some(steps) = steps {
        if bindings.is_present() {
            return Err(Error::new(
                name,
                "steps and compact bindings are mutually exclusive",
            ));
        }
        if steps.is_empty() {
            return Err(Error::new(name, "steps requires at least one step"));
        }
        return steps
            .into_iter()
            .enumerate()
            .map(|(index, step)| {
                if !step.is_present() {
                    return Err(Error::new(
                        format!("{name}: step {index}"),
                        "step requires at least one binding",
                    ));
                }
                Ok(Edit::from(step))
            })
            .collect();
    }
    if !bindings.is_present() {
        return Err(Error::new(
            name,
            "module requires bindings, steps, or source",
        ));
    }
    Ok(vec![Edit::from(bindings)])
}

/// The edits of a `target` module: its local body or its source's body.
fn target_body(
    read_source: &mut impl FnMut(&str) -> Result<String, Error>,
    assignments: &mut HashSet<(u64, String)>,
    location: &str,
    target: &Target,
    source: Option<&str>,
    steps: Option<Vec<BindingsWire>>,
    bindings: BindingsWire,
) -> Result<Vec<Edit>, Error> {
    let Some(source) = source else {
        return body(location, steps, bindings);
    };
    if steps.is_some() || bindings.is_present() {
        return Err(Error::new(
            location,
            "source and local bindings are mutually exclusive",
        ));
    }
    if !assignments.insert((target.chunk_hash(), source.to_ascii_lowercase())) {
        return Err(Error::new(location, "duplicate target/source assignment"));
    }
    let text = read_source(source).map_err(|error| Error::new(location, error))?;
    let source_body: Source =
        parse(source, &text, true).map_err(|error| Error::new(location, error))?;
    version(source, source_body.version)?;
    body(source, source_body.steps, source_body.bindings)
}

/// The edit of every entry of an `entries` module, in mapping order.
fn entries_body(
    location: &str,
    entries: AuthoredEntries,
) -> Result<IndexMap<EntryName, EntryEdit>, Error> {
    if entries.0.is_empty() {
        return Err(Error::new(location, "entries requires at least one entry"));
    }
    entries
        .0
        .into_iter()
        .map(|(name, entry)| {
            let at = format!("{location}: entry {name}");
            if entry.source.is_some() {
                return Err(Error::new(at, "source is not permitted inside entries"));
            }
            if !entry.bindings.is_present() {
                return Err(Error::new(at, "entry requires at least one binding"));
            }
            Ok((name, EntryEdit::from(entry.bindings)))
        })
        .collect()
}

/// Loads a manifest and its sources. The reader resolves paths relative to the layer
/// and enforces containment and input classification.
pub fn load_declarations(
    manifest_name: &str,
    text: &str,
    mut read_source: impl FnMut(&str) -> Result<String, Error>,
) -> Result<Declarations, Error> {
    let manifest: Manifest = parse(manifest_name, text, true)?;
    version(manifest_name, manifest.version)?;
    let mut assignments = HashSet::new();
    let mut modules = Vec::new();
    for (index, module) in manifest.modules.into_iter().enumerate() {
        let location = format!("{manifest_name}: module {index}");
        let AuthoredModule {
            target,
            entries,
            source,
            steps,
            bindings,
        } = module;
        let selector = match one_selector(&location, target, entries)? {
            SelectorKey::Target(target) => {
                let edits = target_body(
                    &mut read_source,
                    &mut assignments,
                    &location,
                    &target,
                    source.as_deref(),
                    steps,
                    bindings,
                )?;
                Selector::Target { target, edits }
            }
            SelectorKey::Entries(entries) => {
                if source.is_some() || steps.is_some() || bindings.is_present() {
                    return Err(Error::new(&location, "entries takes no other bindings"));
                }
                Selector::Entries(entries_body(&location, entries)?)
            }
        };
        modules.push(Module {
            selector,
            origin: Origin {
                manifest: manifest_name.to_owned(),
                source,
                module_index: index,
            },
        });
    }
    let declarations = Declarations {
        version: 1,
        modules,
    };
    declarations.validate()?;
    Ok(declarations)
}
