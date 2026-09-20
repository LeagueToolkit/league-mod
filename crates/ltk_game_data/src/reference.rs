//! A value named by where it lives in the installed game.
//!
//! An author who copies a Riot value into a declaration copies the value of the day they
//! wrote it. A reference names the value instead, and the build reads it from the installed
//! game every time, so a later patch that changes the value changes what the mod applies.

use std::{fmt, str::FromStr};

use ltk_meta::path::PropertyPath;

use crate::{EntryName, Error, ErrorKind};

/// A value of the installed game, named by entry and path ([ADR-0021]).
///
/// The spelling is `<entry name>:<property path>`, which is what the author writes as the
/// value of a `ref` key or a `!ref` tag.
///
/// [ADR-0021]: https://github.com/LeagueToolkit/league-mod/blob/main/docs/adr/0021-game-copy-references.md
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reference {
    /// The entry holding the value.
    pub entry: EntryName,
    /// The path of the value inside that entry.
    pub path: PropertyPath,
}

impl Reference {
    /// The reference `text` spells.
    ///
    /// The split is at the first `:`, so a path holding one keeps it. An entry name never
    /// holds a `:`.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::ReferenceShape`] for text with no `:`, an entry name the name rule
    /// refuses, or a path the path rule refuses.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let shape = || Error::at_key(ErrorKind::ReferenceShape, text);
        let (entry, path) = text.split_once(':').ok_or_else(shape)?;
        let entry = EntryName::try_from(entry).map_err(|_| shape())?;
        let path = PropertyPath::new(path).map_err(|_| shape())?;
        Ok(Self { entry, path })
    }
}

impl TryFrom<&str> for Reference {
    type Error = Error;

    fn try_from(text: &str) -> Result<Self, Error> {
        Self::parse(text)
    }
}

impl FromStr for Reference {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        Self::parse(text)
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.entry.as_str(), self.path.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_round_trips_its_spelling() {
        for text in [
            "Characters/Teemo/Skins/Skin0:speed",
            "0x1234abcd:resourceMap",
            "a/b:c.d[0]",
        ] {
            let reference = Reference::parse(text).unwrap();
            assert_eq!(reference.to_string(), text, "{text}");
        }
    }

    #[test]
    fn a_path_keeps_a_colon_the_entry_name_cannot_hold() {
        let reference = Reference::parse("a/b:c{\"d:e\"}").unwrap();
        assert_eq!(reference.entry.as_str(), "a/b");
        assert_eq!(reference.path.as_str(), "c{\"d:e\"}");
    }

    #[test]
    fn a_reference_refuses_what_it_cannot_split_or_parse() {
        for text in ["nocolon", ":path", "entry:", "entry:a[", "links:a"] {
            let error = Reference::parse(text).unwrap_err();
            assert_eq!(error.kind, ErrorKind::ReferenceShape, "{text}");
        }
    }
}
