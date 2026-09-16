//! Index of one League of Legends installation.
//!
//! The chunk index, [`GameIndex`], holds every chunk of every archive under `Game/DATA/FINAL`
//! with every archive holding it. The object index, `ObjectIndex`, behind the `objects`
//! feature, holds every bin object those chunks declare. Both are keyed by hash and carry no
//! display names. Consumers resolve names through their own tables.
//!
//! An archive is addressed by [`ArchiveId`], its ordinal in the index's name-sorted archive
//! list. An archive that does not open or mount is a [`SkippedArchive`]. It keeps its id and
//! the build indexes the rest.
//!
//! # Features
//!
//! - `rayon` (default): archives mount in parallel. The index is identical either way.
//! - `objects`: the bin object index, which adds `ltk_meta` and `ltk_file`.

mod archive;
mod build;
mod chunk;
mod error;
mod index;

pub use archive::{Archive, ArchiveId, ArchiveLookupError, ArchiveReadError, SkippedArchive};
pub use chunk::{ChunkCopy, ChunkRow, chunk_hash};
pub use error::BuildError;
pub use index::GameIndex;
pub use ltk_hash::WadHash;
