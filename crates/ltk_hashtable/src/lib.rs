//! Primitives for the embedded hashtables a mod carries inside itself.
//!
//! This crate owns the table file grammar, name canonicalization, keys and
//! their truncation, per-category merging, and collision detection. It also
//! owns the parts every container shares: the file name of a new table
//! ([`free_table_name`]) and the reason a read skips a declaration
//! ([`HashtableSkipReason`]). The directory a table file lives in, how it is
//! compressed and how a manifest is spelled belong to the container crates.

mod entry;
mod key;
mod naming;
mod registry;
mod set;
mod skip;
mod table;
#[cfg(feature = "wad")]
mod wad;

pub use entry::HashtableEntry;
pub use key::{Algorithm, Key, KeyWidth};
pub use naming::free_table_name;
pub use registry::Category;
pub use set::{Collision, HashtableSet};
pub use skip::HashtableSkipReason;
pub use table::{Hashtable, HashtableReadError, InvalidNameError};
#[cfg(feature = "wad")]
pub use wad::GameResolver;

#[cfg(test)]
mod tests;
