//! Primitives for the embedded hashtables a mod carries inside itself.
//!
//! This crate owns the table file grammar, name canonicalization, keys and
//! their truncation, per-category merging, and collision detection. It has no
//! notion of a container: where a table file lives, how it is compressed and
//! how a manifest is spelled belong to the container crates.

mod entry;
mod key;
mod registry;
mod set;
mod table;
#[cfg(feature = "wad")]
mod wad;

pub use entry::HashtableEntry;
pub use key::{Algorithm, Key, KeyWidth};
pub use registry::Category;
pub use set::{Collision, HashtableSet};
pub use table::{Hashtable, HashtableReadError, InvalidNameError};
#[cfg(feature = "wad")]
pub use wad::GameResolver;

#[cfg(test)]
mod tests;
