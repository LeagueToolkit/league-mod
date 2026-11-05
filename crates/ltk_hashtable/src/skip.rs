//! [`HashtableSkipReason`]: the reason a container read skips a table declaration.

use std::fmt;

use crate::HashtableReadError;

/// The reason a read skipped a hashtable declaration.
///
/// Each container lists its skipped declarations with this reason beside its own manifest
/// entry.
#[derive(Debug)]
#[non_exhaustive]
pub enum HashtableSkipReason {
    /// The container holds no file at the declared path.
    Absent,
    /// The file at the declared path does not fit the table grammar.
    Unreadable(HashtableReadError),
}

impl fmt::Display for HashtableSkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => f.write_str("the container holds no file at that path"),
            Self::Unreadable(source) => write!(f, "{source}"),
        }
    }
}
