//! A declaration error: a code and the place it is about.

use std::fmt;

/// A declaration that cannot be loaded or applied.
///
/// The `kind` is the code a consumer matches; the `location` names the place. `Display`
/// renders both for a log line. The location is boxed; an `Err` stays small.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct Error {
    /// The code.
    pub kind: ErrorKind,
    /// The place.
    pub location: Box<Location>,
}

impl Error {
    /// An error with no location.
    #[must_use]
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            location: Box::default(),
        }
    }

    /// An error about `key`: a binding key or a property path.
    #[must_use]
    pub fn at_key(kind: ErrorKind, key: impl Into<String>) -> Self {
        Self::new(kind).key(key)
    }

    /// An error about the document `name`.
    #[must_use]
    pub fn in_document(kind: ErrorKind, name: impl Into<String>) -> Self {
        Self::new(kind).document(name)
    }

    /// An I/O failure at the document `name`. `error` is the platform's or the reader's statement.
    #[must_use]
    pub fn io(name: impl Into<String>, error: &dyn fmt::Display) -> Self {
        Self::in_document(
            ErrorKind::Io {
                detail: error.to_string(),
            },
            name,
        )
    }

    /// The error with its document set, where unset.
    #[must_use]
    pub fn document(mut self, name: impl Into<String>) -> Self {
        self.location.document.get_or_insert_with(|| name.into());
        self
    }

    /// The error with its module index set, where unset.
    #[must_use]
    pub fn module(mut self, index: usize) -> Self {
        self.location.module.get_or_insert(index);
        self
    }

    /// The error with its edit index set, where unset.
    #[must_use]
    pub fn edit(mut self, index: usize) -> Self {
        self.location.edit.get_or_insert(index);
        self
    }

    /// The error with its entry name set, where unset.
    #[must_use]
    pub fn entry(mut self, name: impl Into<String>) -> Self {
        self.location.entry.get_or_insert_with(|| name.into());
        self
    }

    /// The error with its key set, where unset.
    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.location.key.get_or_insert_with(|| key.into());
        self
    }

    /// The error with its span set, where unset.
    #[must_use]
    pub fn span(mut self, span: Span) -> Self {
        self.location.span.get_or_insert(span);
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.location.is_empty() {
            write!(f, "{}", self.kind)
        } else {
            write!(f, "{}: {}", self.location, self.kind)
        }
    }
}

/// The place an error is about. Every part is optional; indices are zero-based.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Location {
    /// The manifest, source, or override file, or the layer, as the caller names it.
    pub document: Option<String>,
    /// The module's position in the manifest.
    pub module: Option<usize>,
    /// The edit's position in its module.
    pub edit: Option<usize>,
    /// The entry name as spelled.
    pub entry: Option<String>,
    /// The binding key or the property path as spelled.
    pub key: Option<String>,
    /// The byte range of the document the error is about.
    pub span: Option<Span>,
}

impl Location {
    /// Whether no part is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.document.is_none()
            && self.module.is_none()
            && self.edit.is_none()
            && self.entry.is_none()
            && self.key.is_none()
            && self.span.is_none()
    }
}

/// A byte range of a document: `start` inclusive, `end` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// The span of the character at a 1-based `line` and 1-based character `column` of `text`.
    ///
    /// A position past the end of `text` is the empty span at its end. `serde_saphyr` counts a
    /// column in characters.
    #[must_use]
    pub fn at_line_column(text: &str, line: usize, column: usize) -> Self {
        let line_start = line_start(text, line);
        let start = text[line_start..]
            .char_indices()
            .nth(column.saturating_sub(1))
            .map_or(text.len(), |(offset, _)| line_start + offset);
        Self::at(text, start)
    }

    /// The span of the character at a 1-based `line` and 1-based byte `column` of `text`.
    ///
    /// A position past the end of `text` is the empty span at its end; one inside a character
    /// is the span of that character. `serde_json` counts a column in bytes.
    #[must_use]
    pub fn at_line_byte_column(text: &str, line: usize, column: usize) -> Self {
        let mut start = line_start(text, line).saturating_add(column.saturating_sub(1));
        if start >= text.len() {
            return Self {
                start: text.len(),
                end: text.len(),
            };
        }
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        Self::at(text, start)
    }

    /// The span of the character `text` holds at the byte offset `start`.
    fn at(text: &str, start: usize) -> Self {
        let end = text[start..]
            .chars()
            .next()
            .map_or(start, |c| start + c.len_utf8());
        Self { start, end }
    }
}

/// The byte offset the 1-based `line` of `text` starts at. A line past the last is its end.
fn line_start(text: &str, line: usize) -> usize {
    text.split_inclusive('\n')
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len();
            Some(start)
        })
        .nth(line.saturating_sub(1))
        .unwrap_or(text.len())
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::with_capacity(5);
        if let Some(document) = &self.document {
            parts.push(document.clone());
        }
        if let Some(module) = self.module {
            parts.push(format!("module {module}"));
        }
        if let Some(edit) = self.edit {
            parts.push(format!("edit {edit}"));
        }
        if let Some(entry) = &self.entry {
            parts.push(format!("entry {entry}"));
        }
        if let Some(key) = &self.key {
            parts.push(key.clone());
        }
        if let Some(span) = self.span {
            parts.push(format!("bytes {}..{}", span.start, span.end));
        }
        f.write_str(&parts.join(": "))
    }
}

/// The code of a declaration error. Each variant's statement is its log rendering.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The declaration version is not one this crate executes.
    #[error("unsupported declaration version")]
    UnsupportedVersion,
    /// A document extension that is not YAML, TOML, or JSON.
    #[error("expected YAML, TOML, or JSON")]
    UnknownFormat,
    /// A document the parser refuses. `detail` is the parser's statement.
    #[error("{detail}")]
    Syntax { detail: String },
    /// A file that cannot be read or written. `detail` is the platform's or the reader's statement.
    #[error("{detail}")]
    Io { detail: String },
    /// An archive's layer metadata that cannot be read. `detail` is the archive reader's statement.
    #[error("{detail}")]
    Metadata { detail: String },
    /// Declarations that cannot be written. `detail` is the writer's statement.
    #[error("{detail}")]
    Serialize { detail: String },
    /// A referenced input that does not exist.
    #[error("input is missing")]
    InputMissing,
    /// A referenced input that `.modignore` excludes.
    #[error("input is excluded by .modignore")]
    InputIgnored,
    /// A referenced input that resolves outside its layer.
    #[error("input escapes the layer")]
    InputEscapes,
    /// A source that is also a manifest.
    #[error("manifest and source roles conflict")]
    RoleConflict,
    /// A layer with more than one manifest.
    #[error("multiple game-data manifests")]
    MultipleManifests,
    /// A source path that is absolute or has a backslash.
    #[error("source requires a layer-relative path with forward slashes")]
    SourcePathInvalid,
    /// An override file that does not read as a `PTCH`.
    #[error("override file is not a PTCH")]
    OverrideNotPtch,
    /// A layer name that is not a directory name.
    #[error("invalid layer name")]
    InvalidLayerName,
    /// A target that is the empty string.
    #[error("expected a nonempty target")]
    EmptyTarget,
    /// An entry name that is the empty string.
    #[error("expected a nonempty entry name")]
    EmptyEntryName,
    /// A link path that is empty or longer than the header's limit.
    #[error("link paths require 1 to 65535 UTF-8 bytes")]
    LinkPathLength,
    /// An override path that is the empty string.
    #[error("expected a nonempty override path")]
    EmptyOverridePath,
    /// An override path with a backslash.
    #[error("override paths use forward slashes")]
    OverridePathBackslash,
    /// An override path that starts with a slash, or whose first segment holds a drive `:`.
    #[error("override paths are relative")]
    OverridePathAbsolute,
    /// An override path with an empty, `.`, or `..` segment.
    #[error("override paths contain no empty, `.`, or `..` segment")]
    OverridePathSegment,
    /// An override path whose file is not `.ptch`.
    #[error("override files require the `.ptch` extension")]
    OverridePathExtension,
    /// A `.rito` override path. The text form needs a `PTCH` text parser.
    #[error("`.rito` override files are unsupported; convert the file to `.ptch`")]
    OverridePathRito,
    /// An override path whose `..` segments resolve above the layer.
    #[error("override path leaves the layer")]
    OverridePathEscapes,
    /// A module with neither `target` nor `entries`.
    #[error("module requires target or entries")]
    SelectorMissing,
    /// A module with both `target` and `entries`.
    #[error("target and entries are mutually exclusive")]
    SelectorConflict,
    /// A document module with `target` and no `edits`.
    #[error("target requires `edits`")]
    TargetWithoutEdits,
    /// A document module with both `entries` and `edits`.
    #[error("entries and `edits` are mutually exclusive")]
    EntriesWithEdits,
    /// An `entries` module with `source`, `edits`, or a binding beside the mapping.
    #[error("entries takes no other bindings")]
    EntriesWithBindings,
    /// An `entries` mapping with no entry.
    #[error("entries requires at least one entry")]
    EntriesEmpty,
    /// An entry body with a `source` key.
    #[error("source is not permitted inside entries")]
    SourceInEntry,
    /// An entry body with no key.
    #[error("entry requires at least one binding")]
    EntryWithoutBindings,
    /// An entry body with an `overrides` key.
    #[error("overrides is not permitted inside entries")]
    OverridesInEntry,
    /// A module with `source` and a local binding.
    #[error("source and local bindings are mutually exclusive")]
    SourceWithBindings,
    /// A target and source pair a manifest names twice.
    #[error("duplicate target/source assignment")]
    DuplicateAssignment,
    /// A body with `edits` and a compact binding.
    #[error("`edits` and compact bindings are mutually exclusive")]
    MixedBodies,
    /// An `edits` list with no edit.
    #[error("`edits` requires at least one edit")]
    EditsEmpty,
    /// An edit with no key.
    #[error("edit requires at least one binding")]
    EditWithoutBindings,
    /// A target module with no binding, no `edits`, and no `source`.
    #[error("module requires bindings, `edits`, or source")]
    ModuleWithoutBindings,
    /// A body key that is neither a binding keyword nor an entry name.
    #[error("unsupported binding `{key}`")]
    UnsupportedBinding { key: String },
    /// A property path or entry name spelling a binding keyword. A body mapping holds one of
    /// the two meanings.
    #[error("`{key}` is a binding keyword")]
    ReservedBindingKey { key: String },
    /// Two property edits of one entry under one signed key. An entry body holds one value
    /// per key.
    #[error("duplicate property key `{key}`")]
    DuplicatePropertyKey { key: String },
    /// An entry name whose value is not a mapping.
    #[error("entry body requires a mapping")]
    EntryBodyShape,
    /// A property key whose path does not parse. `detail` is the path parser's statement.
    #[error("invalid property path: {detail}")]
    InvalidPropertyPath { detail: String },
    /// A `pointer` or `embed` pin that is not null or a mapping of `class` and `set`.
    #[error("a pointer or embed pin takes `class` and `set`")]
    StructPinShape,
    /// A `ref` key whose value is not an entry name, a `:`, and a property path.
    #[error("a ref takes `<entry>:<property path>`")]
    ReferenceShape,
    /// A map key no string spells: a key of a kind the key rule refuses, a key the map
    /// holds twice, or the one key of a map that reads as a pin or a reference.
    #[error("a map key has no spelling")]
    UnrenderableKey,
    /// A value of kind `none`, which no literal reads back as.
    #[error("a value of kind none has no literal")]
    UnrenderableValue,
    /// An application base that is not a `PROP` version 2 or 3.
    #[error("expected PROP version 2 or 3")]
    UnsupportedBase,
    /// A base or output `ltk_meta` refuses. `detail` is its statement.
    #[error("{detail}")]
    Bin { detail: String },
    /// A dependency list the `PROP` header cannot count.
    #[error("dependency list exceeds the header's limits")]
    DependencyOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_fills_only_unset_parts() {
        let error = Error::in_document(ErrorKind::EmptyTarget, "shared.yaml")
            .key("target")
            .module(2)
            .document("game_data.yaml")
            .edit(1);
        assert_eq!(
            *error.location,
            Location {
                document: Some("shared.yaml".into()),
                module: Some(2),
                edit: Some(1),
                entry: None,
                key: Some("target".into()),
                span: None,
            }
        );
        assert_eq!(
            error.to_string(),
            "shared.yaml: module 2: edit 1: target: expected a nonempty target"
        );
        assert_eq!(
            Error::new(ErrorKind::UnsupportedVersion).to_string(),
            "unsupported declaration version"
        );
    }

    #[test]
    fn spans_locate_a_line_and_column() {
        let text = "ab\ncd\u{e9}f\n";
        assert_eq!(Span::at_line_column(text, 1, 1), Span { start: 0, end: 1 });
        assert_eq!(Span::at_line_column(text, 2, 3), Span { start: 5, end: 7 });
        assert_eq!(Span::at_line_column(text, 9, 9), Span { start: 9, end: 9 });
    }

    #[test]
    fn a_byte_column_locates_the_same_character_a_multibyte_line_holds() {
        let text = "ab\ncd\u{e9}f\n";
        // Character column 4 is `f`; byte column 4 is the second byte of `\u{e9}`.
        assert_eq!(Span::at_line_column(text, 2, 4), Span { start: 7, end: 8 });
        assert_eq!(
            Span::at_line_byte_column(text, 2, 4),
            Span { start: 5, end: 7 }
        );
        assert_eq!(
            Span::at_line_byte_column(text, 2, 5),
            Span { start: 7, end: 8 }
        );
        assert_eq!(
            Span::at_line_byte_column(text, 9, 9),
            Span { start: 9, end: 9 }
        );
    }
}
