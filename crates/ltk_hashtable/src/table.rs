//! [`Hashtable`]: one table's names, in file order.

use std::io::{self, BufRead, BufReader, Read, Write};

/// A name that does not fit the table grammar: printable ASCII, `/`
/// separators.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Not a valid name: {name:?}")]
pub struct InvalidNameError {
    name: String,
}

impl InvalidNameError {
    /// The offending name, verbatim.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Failure to read a hashtable file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HashtableReadError {
    /// The table file could not be read.
    #[error(transparent)]
    Io(#[from] io::Error),

    /// The file starts with a byte order mark, which the grammar refuses.
    #[error("The table file starts with a byte order mark")]
    ByteOrderMark,

    /// A line holds a character outside the grammar.
    #[error("Line {line} is not a valid name")]
    InvalidName {
        /// The 1-based line number of the offending name.
        line: usize,
        /// The name that was refused.
        #[source]
        source: InvalidNameError,
    },
}

/// Whether `name` fits the grammar: printable ASCII, `/` separators only.
fn is_valid_name(name: &str) -> bool {
    name.bytes()
        .all(|byte| (0x20..=0x7e).contains(&byte) && byte != b'\\')
}

/// One hashtable file's names, in file order.
///
/// Holds names exactly as authored - display casing included. Canonicalizing
/// and hashing happen at key computation, never here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hashtable {
    names: Vec<String>,
}

impl Hashtable {
    /// Read a table file: one name per line, blank lines skipped, CRLF
    /// tolerated.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails, the file starts with a BOM, or a
    /// line holds a character outside the grammar.
    pub fn from_reader(reader: impl Read) -> Result<Self, HashtableReadError> {
        let mut names = Vec::new();
        for (index, line) in BufReader::new(reader).lines().enumerate() {
            let line = line?;
            if index == 0 && line.starts_with('\u{feff}') {
                return Err(HashtableReadError::ByteOrderMark);
            }
            if line.is_empty() {
                continue;
            }
            if let Err(source) = Self::validated(&line) {
                return Err(HashtableReadError::InvalidName {
                    line: index + 1,
                    source,
                });
            }
            names.push(line);
        }
        Ok(Self { names })
    }

    /// Build a table from `names`, in iteration order.
    ///
    /// # Errors
    ///
    /// Returns an error if a name does not fit the grammar: printable ASCII
    /// with `/` separators.
    pub fn from_names(
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, InvalidNameError> {
        let mut table = Self::default();
        for name in names {
            table.push(name)?;
        }
        Ok(table)
    }

    /// Check `name` against the grammar.
    fn validated(name: &str) -> Result<(), InvalidNameError> {
        if is_valid_name(name) {
            Ok(())
        } else {
            Err(InvalidNameError {
                name: name.to_owned(),
            })
        }
    }

    /// Append one name, in table order.
    ///
    /// # Errors
    ///
    /// Returns an error if the name does not fit the grammar: printable ASCII
    /// with `/` separators.
    pub fn push(&mut self, name: impl Into<String>) -> Result<(), InvalidNameError> {
        let name = name.into();
        Self::validated(&name)?;
        self.names.push(name);
        Ok(())
    }

    /// Sort the names in `LC_ALL=C` byte order, the order version control
    /// diffs best.
    pub fn sort(&mut self) {
        self.names.sort_unstable();
    }

    /// The table's names, in table order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    /// Write the table: one name per line, LF-terminated, in table order.
    ///
    /// # Errors
    ///
    /// Returns an error if writing fails.
    pub fn write_to(&self, mut writer: impl Write) -> io::Result<()> {
        for name in &self.names {
            writer.write_all(name.as_bytes())?;
            writer.write_all(b"\n")?;
        }
        Ok(())
    }
}
