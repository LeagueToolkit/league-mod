//! The manifest a layer is edited through, and the source files it names.
//!
//! A [`DocumentPath`] names one file by its layer-relative path and resolves the override
//! paths the file spells. [`Manifest`] and [`Source`] are the two file shapes; [`Body`] is the
//! edits either one carries. [`Manifest::load`] expands sources into [`Declarations`];
//! [`Manifest::from`] a `&Declarations` is the manifest that loads to them.

mod body;
mod source;

use std::{collections::HashSet, fmt};

use camino::Utf8Path;
use indexmap::IndexMap;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{DeserializeOwned, MapAccess, Visitor},
};

use crate::{
    Declarations, Edit, EntryEdit, EntryName, Error, ErrorKind, ModuleName, Origin, OverridePath,
    Selector, Span, Target,
    document::{Accepts, Bindings, Fields, SelectorKey},
};

use body::Body;
use source::Source;

pub const MANIFEST_NAMES: [&str; 4] = [
    "game_data.yaml",
    "game_data.yml",
    "game_data.toml",
    "game_data.json",
];

/// The serialization format of a manifest or source file, from its file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Yaml,
    Json,
    Toml,
}

/// How a file is read. `Execution` refuses duplicate keys; `Discovery` keeps the last of
/// duplicate keys. A local YAML tag is a type pin and is read in both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    Execution,
    Discovery,
}

impl Format {
    /// The format of the file `name`, from its extension compared ASCII case-insensitively.
    fn of(name: &str) -> Result<Self, Error> {
        let extension = Utf8Path::new(name)
            .extension()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match extension.as_str() {
            "yaml" | "yml" => Ok(Self::Yaml),
            "json" => Ok(Self::Json),
            "toml" => Ok(Self::Toml),
            _ => Err(Error::in_document(ErrorKind::UnknownFormat, name)),
        }
    }

    fn parse<T: DeserializeOwned>(
        self,
        name: &str,
        text: &str,
        reading: Reading,
    ) -> Result<T, Error> {
        let syntax = |error: &dyn fmt::Display, span: Option<Span>| {
            let error = Error::in_document(
                ErrorKind::Syntax {
                    detail: error.to_string(),
                },
                name,
            );
            match span {
                Some(span) => error.span(span),
                None => error,
            }
        };
        match self {
            Self::Yaml => {
                let strict = reading == Reading::Execution;
                let mut options = serde_saphyr::Options::default();
                options.strict_booleans = true;
                options.no_schema = true;
                options.reject_unsupported_tags = false;
                options.duplicate_keys = if strict {
                    serde_saphyr::DuplicateKeyPolicy::Error
                } else {
                    serde_saphyr::DuplicateKeyPolicy::LastWins
                };
                serde_saphyr::from_str_with_options(text, options).map_err(|e| {
                    let span = e.location().map(|at| {
                        let clamp = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
                        Span::at_line_column(text, clamp(at.line()), clamp(at.column()))
                    });
                    syntax(&e, span)
                })
            }
            Self::Json => serde_json::from_str(text).map_err(|e| {
                let span = Span::at_line_byte_column(text, e.line(), e.column());
                syntax(&e, Some(span))
            }),
            Self::Toml => toml::from_str(text).map_err(|e| {
                let span = e.span().map(|range| Span {
                    start: range.start,
                    end: range.end,
                });
                syntax(&e, span)
            }),
        }
    }
}

/// The layer-relative path of one manifest or source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocumentPath<'a>(&'a str);

impl<'a> DocumentPath<'a> {
    pub(crate) fn new(path: &'a str) -> Self {
        Self(path)
    }

    /// The path as spelled.
    pub(crate) fn as_str(&self) -> &'a str {
        self.0
    }

    /// The file's layer-relative directory. A manifest's is empty.
    fn directory(&self) -> &'a str {
        self.0
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory)
    }

    fn parse<T: DeserializeOwned>(&self, text: &str, reading: Reading) -> Result<T, Error> {
        Format::of(self.0)?.parse(self.0, text, reading)
    }

    /// Resolves an override path lexically against the file's directory.
    ///
    /// # Errors
    ///
    /// The path is empty, absolute, or leaves the layer, or its resolved spelling is not an
    /// [`OverridePath`].
    pub(crate) fn resolve_override(&self, path: &str) -> Result<OverridePath, Error> {
        if path.is_empty() {
            return Err(Error::at_key(ErrorKind::EmptyOverridePath, "overrides"));
        }
        if path.starts_with('/') {
            return Err(Error::at_key(ErrorKind::OverridePathAbsolute, "overrides"));
        }
        let mut segments: Vec<&str> = self
            .directory()
            .split('/')
            .filter(|s| !matches!(*s, "" | "."))
            .collect();
        for segment in path.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    if segments.pop().is_none() {
                        return Err(Error::at_key(ErrorKind::OverridePathEscapes, "overrides"));
                    }
                }
                other => segments.push(other),
            }
        }
        OverridePath::try_from(segments.join("/"))
    }
}

/// The inputs a manifest or source file references, discovered structurally.
///
/// Discovery is independent of binding validation: a file with duplicate keys or
/// unsupported bindings still reports its references. Each override path is as spelled,
/// relative to the file naming it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferencedInputs {
    /// The `source` paths of the file's modules, layer-relative.
    pub sources: Vec<String>,
    /// The override paths as spelled, relative to the file.
    pub overrides: Vec<String>,
}

impl ReferencedInputs {
    /// Discovers the references of the file `name` with content `text`.
    ///
    /// # Errors
    ///
    /// The file is not parseable in its format.
    pub fn discover(name: &str, text: &str) -> Result<Self, Error> {
        let document: crate::discovery::Node =
            DocumentPath::new(name).parse(text, Reading::Discovery)?;
        Ok(Self {
            sources: document.sources(),
            overrides: document.overrides(),
        })
    }
}

/// A declaration version this crate executes. Its serialized value is `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(super) struct Version;

impl Version {
    /// The one supported declaration version.
    const SUPPORTED: u32 = 1;
}

impl TryFrom<u32> for Version {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self, Error> {
        if value == Self::SUPPORTED {
            Ok(Self)
        } else {
            Err(Error::at_key(ErrorKind::UnsupportedVersion, "version"))
        }
    }
}

impl From<Version> for u32 {
    fn from(_: Version) -> Self {
        Version::SUPPORTED
    }
}

/// The layer manifest: a version and its modules in execution order.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    version: Version,
    modules: Vec<Module>,
}

/// A manifest module. A key that is not `name`, a selector, `source`, `edits`, or a binding
/// is an entry name.
#[derive(Debug, Serialize)]
struct Module {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<Target>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entries: Option<Entries>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(flatten)]
    body: Body,
}

impl<'de> Deserialize<'de> for Module {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let accepts = Accepts {
            name: true,
            selector: true,
            source: true,
            edits: true,
            version: false,
        };
        let fields: Fields<Entries> = deserializer.deserialize_map(Fields::visitor(accepts))?;
        Ok(Self {
            name: fields.name,
            target: fields.target,
            entries: fields.entries,
            source: fields.source,
            body: Body {
                edits: fields.edits,
                bindings: fields.bindings,
            },
        })
    }
}

/// An entry body: property edits and link bindings. `source` is read and refused.
#[derive(Debug, Serialize)]
struct Entry {
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(flatten)]
    bindings: Bindings,
}

impl<'de> Deserialize<'de> for Entry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let accepts = Accepts {
            source: true,
            ..Accepts::default()
        };
        let fields: Fields<()> = deserializer.deserialize_map(Fields::visitor(accepts))?;
        Ok(Self {
            source: fields.source,
            bindings: fields.bindings,
        })
    }
}

impl Entry {
    fn into_edit(self) -> Result<EntryEdit, Error> {
        if self.source.is_some() {
            return Err(Error::new(ErrorKind::SourceInEntry));
        }
        if !self.bindings.is_present() {
            return Err(Error::new(ErrorKind::EntryWithoutBindings));
        }
        EntryEdit::try_from(self.bindings)
    }
}

/// An `entries` mapping in spelled order. A repeated name is an error in every format.
#[derive(Debug)]
struct Entries(IndexMap<EntryName, Entry>);

impl Entries {
    /// The edit of every entry, in mapping order, none for an empty mapping. An error names
    /// its entry.
    fn into_edits(self) -> Result<IndexMap<EntryName, EntryEdit>, Error> {
        self.0
            .into_iter()
            .map(|(name, entry)| {
                let edit = entry
                    .into_edit()
                    .map_err(|error| error.entry(name.as_str()))?;
                Ok((name, edit))
            })
            .collect()
    }
}

impl Serialize for Entries {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(&self.0)
    }
}

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntriesVisitor;
        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = Entries;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a mapping of entry names to bindings")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = IndexMap::new();
                while let Some((name, body)) = map.next_entry::<EntryName, Entry>()? {
                    if entries.insert(name.clone(), body).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate entry name `{name}`"
                        )));
                    }
                }
                Ok(Entries(entries))
            }
        }
        deserializer.deserialize_map(EntriesVisitor)
    }
}

/// One load of a manifest: the sources read so far and the target/source assignments seen.
struct Loading<'a, R> {
    manifest: DocumentPath<'a>,
    read_source: R,
    assignments: HashSet<(u64, String)>,
}

impl<R: FnMut(&str) -> Result<String, Error>> Loading<'_, R> {
    /// The edits of a `target` module: its local body or its source's body.
    fn target_edits(
        &mut self,
        target: &Target,
        source: Option<&str>,
        body: Body,
    ) -> Result<Vec<Edit>, Error> {
        let Some(source) = source else {
            return body.into_edits(self.manifest);
        };
        if body.is_present() {
            return Err(Error::new(ErrorKind::SourceWithBindings));
        }
        if !self
            .assignments
            .insert((target.chunk_hash(), source.to_ascii_lowercase()))
        {
            return Err(Error::new(ErrorKind::DuplicateAssignment));
        }
        let text = (self.read_source)(source).map_err(|error| error.document(source))?;
        Source::load(DocumentPath::new(source), &text)
    }

    /// The selector of a module. An error carries no module context.
    fn selector(
        &mut self,
        target: Option<Target>,
        entries: Option<Entries>,
        source: Option<&str>,
        body: Body,
    ) -> Result<Selector, Error> {
        Ok(match SelectorKey::one(target, entries)? {
            SelectorKey::Target(target) => {
                let edits = self.target_edits(&target, source, body)?;
                Selector::Target { target, edits }
            }
            SelectorKey::Entries(entries) => {
                if source.is_some() || body.is_present() {
                    return Err(Error::new(ErrorKind::EntriesWithBindings));
                }
                Selector::Entries(entries.into_edits()?)
            }
        })
    }

    fn module(&mut self, index: usize, module: Module) -> Result<crate::Module, Error> {
        let Module {
            name,
            target,
            entries,
            source,
            body,
        } = module;
        let manifest = self.manifest;
        let at = |error: Error| error.document(manifest.as_str()).module(index);
        let name = name.map(ModuleName::try_from).transpose().map_err(at)?;
        let selector = self
            .selector(target, entries, source.as_deref(), body)
            .map_err(at)?;
        Ok(crate::Module {
            name,
            selector,
            origin: Origin {
                manifest: self.manifest.as_str().to_owned(),
                source,
                module_index: index,
            },
        })
    }
}

impl Manifest {
    /// Loads the manifest at `path` and its sources, reading each source through the reader
    /// and refusing a repeated target/source assignment.
    pub(crate) fn load(
        self,
        path: DocumentPath<'_>,
        read_source: impl FnMut(&str) -> Result<String, Error>,
    ) -> Result<Declarations, Error> {
        let mut loading = Loading {
            manifest: path,
            read_source,
            assignments: HashSet::new(),
        };
        let modules = self
            .modules
            .into_iter()
            .enumerate()
            .map(|(index, module)| loading.module(index, module))
            .collect::<Result<Vec<_>, _>>()?;
        let declarations = Declarations {
            version: u32::from(self.version),
            modules,
        };
        declarations.validate()?;
        Ok(declarations)
    }
}

/// The bindings a manifest writes for an edit: `links` is present, empty or not.
fn written(mut bindings: Bindings) -> Bindings {
    bindings.add_links.get_or_insert_default();
    bindings
}

impl TryFrom<&Declarations> for Manifest {
    type Error = Error;

    /// The direct manifest that loads to `declarations`: no sources, every override path
    /// layer-relative, and an `edits` list for every target module.
    ///
    /// # Errors
    ///
    /// Whatever the body of an edit or an entry refuses, named by its module index.
    fn try_from(declarations: &Declarations) -> Result<Self, Error> {
        let modules = declarations
            .modules
            .iter()
            .enumerate()
            .map(|(index, module)| {
                let at =
                    |error: Error| error.document(module.origin.manifest.clone()).module(index);
                Ok(match &module.selector {
                    Selector::Target { target, edits } => Module {
                        name: module.name.clone().map(String::from),
                        target: Some(target.clone()),
                        entries: None,
                        source: None,
                        body: Body {
                            edits: Some(
                                edits
                                    .iter()
                                    .map(|edit| {
                                        Bindings::try_from(edit.clone()).map(written).map_err(at)
                                    })
                                    .collect::<Result<Vec<_>, Error>>()?,
                            ),
                            bindings: Bindings::default(),
                        },
                    },
                    Selector::Entries(entries) => Module {
                        name: module.name.clone().map(String::from),
                        target: None,
                        entries: Some(Entries(
                            entries
                                .iter()
                                .map(|(name, edit)| {
                                    let bindings = Bindings::try_from(edit.clone())
                                        .map_err(|error| at(error.entry(name.as_str())))?;
                                    let entry = Entry {
                                        source: None,
                                        bindings: written(bindings),
                                    };
                                    Ok((name.clone(), entry))
                                })
                                .collect::<Result<IndexMap<_, _>, Error>>()?,
                        )),
                        source: None,
                        body: Body::default(),
                    },
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self {
            version: Version,
            modules,
        })
    }
}

/// Loads a manifest and its sources. The reader resolves paths relative to the layer
/// and enforces containment and input classification.
pub fn load_declarations(
    manifest_name: &str,
    text: &str,
    read_source: impl FnMut(&str) -> Result<String, Error>,
) -> Result<Declarations, Error> {
    let path = DocumentPath::new(manifest_name);
    let manifest: Manifest = path.parse(text, Reading::Execution)?;
    manifest.load(path, read_source)
}
