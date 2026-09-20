//! Index of one League of Legends installation.
//!
//! The chunk index, [`GameIndex`], holds every chunk of every archive under
//! `Game/DATA/FINAL` with every archive holding it. The object index, `ObjectIndex`,
//! behind the `objects` feature, holds every bin object those chunks declare. The object
//! build takes an optional [`ResolveWadPath`] to name chunks. Without one it sniffs every
//! chunk. Both are keyed by hash and carry no
//! display names. Consumers resolve names through their own tables.
//!
//! A [`Fingerprint`] identifies an installation's archive set by size and modification time.
//! Either index caches to disk under the fingerprint it was built from, and a cache for
//! another installation state is stale.
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
mod cache;
mod chunk;
mod error;
mod fingerprint;
mod index;
#[cfg(feature = "objects")]
mod objects;
mod resolver;

pub use archive::{Archive, ArchiveId, ArchiveLookupError, ArchiveReadError, SkippedArchive};
pub use cache::CacheError;
pub use chunk::{ChunkCopy, ChunkRow, chunk_hash};
pub use error::BuildError;
pub use fingerprint::Fingerprint;
pub use index::{CACHE_FORMAT_VERSION, GameIndex};
#[cfg(feature = "objects")]
pub use ltk_hash::BinHash;
pub use ltk_hash::WadHash;
#[cfg(feature = "objects")]
pub use objects::{
    BuildOptions, Declaration, OBJECT_CACHE_FORMAT_VERSION, ObjectBuildError, ObjectIndex,
    ObjectStats, for_each_declaration,
};
pub use resolver::ResolveWadPath;
