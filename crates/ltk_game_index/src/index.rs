//! The chunk index: every chunk of an installation with every holder.

use std::collections::HashMap;

use camino::{Utf8Path, Utf8PathBuf};
use ltk_wad::WadHash;

use serde::{Deserialize, Serialize};

use crate::{
    Archive, ArchiveId, ArchiveLookupError, BuildError, CacheError, ChunkRow, Fingerprint,
    SkippedArchive, build, cache, chunk_hash, fingerprint,
};

/// The cache format version of the chunk index.
///
/// Bumped on any change to [`Archive`], [`ChunkRow`], [`SkippedArchive`], or the container
/// layout.
pub const CACHE_FORMAT_VERSION: u32 = 1;

/// The cached body of a chunk index. The file-name index is rebuilt on load.
#[derive(Serialize, Deserialize)]
struct Body {
    archives: Vec<Archive>,
    hashes: Vec<WadHash>,
    rows: Vec<ChunkRow>,
    skipped: Vec<SkippedArchive>,
}

/// Every chunk of an installation, keyed by chunk hash, with every holder.
///
/// Archives are numbered by [`ArchiveId`] in name order. Rows are keyed by chunk hash and
/// carry no display names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameIndex {
    archives: Vec<Archive>,
    /// ASCII-lowercased file name to every archive with it, in id order.
    by_file_name: HashMap<String, Vec<ArchiveId>>,
    /// Ascending chunk hashes, parallel to `rows`.
    hashes: Vec<WadHash>,
    rows: Vec<ChunkRow>,
    skipped: Vec<SkippedArchive>,
    fingerprint: Fingerprint,
}

impl GameIndex {
    pub(crate) fn from_parts(
        archives: Vec<Archive>,
        hashes: Vec<WadHash>,
        rows: Vec<ChunkRow>,
        skipped: Vec<SkippedArchive>,
        fingerprint: Fingerprint,
    ) -> Self {
        debug_assert_eq!(hashes.len(), rows.len());
        let mut by_file_name: HashMap<String, Vec<ArchiveId>> = HashMap::new();
        for (index, archive) in archives.iter().enumerate() {
            by_file_name
                .entry(archive.file_name().to_ascii_lowercase())
                .or_default()
                .push(ArchiveId(index as u32));
        }
        Self {
            archives,
            by_file_name,
            hashes,
            rows,
            skipped,
            fingerprint,
        }
    }

    /// Indexes every archive under `game_dir/DATA/FINAL`.
    ///
    /// An archive that does not open or mount is recorded in [`skipped`](Self::skipped) and
    /// the rest index.
    ///
    /// # Errors
    ///
    /// [`BuildError::MissingDataFinal`] when `game_dir` has no `DATA/FINAL`.
    /// [`BuildError::Enumerate`] when the directory walk fails.
    pub fn build(game_dir: &Utf8Path) -> Result<Self, BuildError> {
        build::build(game_dir)
    }

    /// Indexes the given archive files, each named relative to `root`.
    ///
    /// Archives sort by name in byte order and index in that order.
    ///
    /// # Errors
    ///
    /// [`BuildError::ArchiveOutsideRoot`] when an archive does not lie under `root`.
    pub fn build_from_archives(
        root: &Utf8Path,
        archives: &[Utf8PathBuf],
    ) -> Result<Self, BuildError> {
        build::build_from_archives(root, archives)
    }

    /// The fingerprint of the installation at `game_dir`, without mounting an archive.
    ///
    /// # Errors
    ///
    /// [`BuildError::MissingDataFinal`], [`BuildError::Enumerate`] and
    /// [`BuildError::Metadata`].
    pub fn fingerprint_of(game_dir: &Utf8Path) -> Result<Fingerprint, BuildError> {
        fingerprint::of_game_dir(game_dir)
    }

    /// Reads the cache at `cache_path`, whatever fingerprint it carries.
    ///
    /// # Errors
    ///
    /// [`CacheError::Read`], [`CacheError::Decode`], and [`CacheError::Version`] for a
    /// format version other than [`CACHE_FORMAT_VERSION`].
    pub fn load(cache_path: &Utf8Path) -> Result<Self, CacheError> {
        let cache::Document { fingerprint, body } =
            cache::read::<Body>(cache_path, CACHE_FORMAT_VERSION)?;
        Ok(Self::from_parts(
            body.archives,
            body.hashes,
            body.rows,
            body.skipped,
            fingerprint,
        ))
    }

    /// Reads the cache at `cache_path` for the installation at `game_dir`.
    ///
    /// # Errors
    ///
    /// Everything [`load`](Self::load) reports, [`CacheError::Build`] when the installation's
    /// fingerprint does not compute, and [`CacheError::Stale`] when it differs from the
    /// cached one.
    pub fn load_for(cache_path: &Utf8Path, game_dir: &Utf8Path) -> Result<Self, CacheError> {
        let index = Self::load(cache_path)?;
        let current = Self::fingerprint_of(game_dir)?;
        if index.fingerprint != current {
            return Err(CacheError::Stale {
                path: cache_path.to_path_buf(),
                cached: index.fingerprint,
                current,
            });
        }
        Ok(index)
    }

    /// Writes the index to `cache_path` atomically.
    ///
    /// The bytes go to a sibling temporary file, which is renamed over `cache_path`.
    ///
    /// # Errors
    ///
    /// [`CacheError::Write`] and [`CacheError::Encode`].
    pub fn save(&self, cache_path: &Utf8Path) -> Result<(), CacheError> {
        let body = Body {
            archives: self.archives.clone(),
            hashes: self.hashes.clone(),
            rows: self.rows.clone(),
            skipped: self.skipped.clone(),
        };
        cache::write(cache_path, CACHE_FORMAT_VERSION, self.fingerprint, &body)?;
        tracing::debug!("Game index cache saved to {cache_path}");
        Ok(())
    }

    /// The cached index when it matches the installation, and a fresh build otherwise.
    ///
    /// A missing cache is logged at debug and any other cache error at warn. The built index
    /// is saved best-effort, with a warn on failure.
    ///
    /// # Errors
    ///
    /// Everything [`build`](Self::build) reports.
    pub fn load_or_build(game_dir: &Utf8Path, cache_path: &Utf8Path) -> Result<Self, BuildError> {
        cache::load_or_build(
            "game index",
            cache_path,
            || Self::load_for(cache_path, game_dir),
            || Self::build(game_dir),
            |index| index.save(cache_path),
        )
    }

    /// The fingerprint of the installation the index was built from.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Every archive, in id order.
    #[must_use]
    pub fn archives(&self) -> &[Archive] {
        &self.archives
    }

    /// The archive with `id`.
    ///
    /// # Panics
    ///
    /// Panics on an id from another index.
    #[must_use]
    pub fn archive(&self, id: ArchiveId) -> &Archive {
        self.archives.get(id.index()).unwrap_or_else(|| {
            panic!(
                "archive id {id} is not from this index, which has {} archives",
                self.archives.len()
            )
        })
    }

    /// The one archive whose file name is `file_name`, ASCII case-insensitively.
    ///
    /// # Errors
    ///
    /// [`ArchiveLookupError::Absent`] when no archive matches and
    /// [`ArchiveLookupError::Ambiguous`] when several do.
    pub fn archive_by_file_name(&self, file_name: &str) -> Result<ArchiveId, ArchiveLookupError> {
        match self
            .by_file_name
            .get(&file_name.to_ascii_lowercase())
            .map(Vec::as_slice)
        {
            Some([single]) => Ok(*single),
            Some(candidates) => Err(ArchiveLookupError::Ambiguous {
                file_name: file_name.to_owned(),
                candidates: candidates.to_vec(),
            }),
            None => Err(ArchiveLookupError::Absent {
                file_name: file_name.to_owned(),
            }),
        }
    }

    /// Every archive the build could not read, in id order.
    #[must_use]
    pub fn skipped(&self) -> &[SkippedArchive] {
        &self.skipped
    }

    /// The row of `hash`, or `None` for a chunk no archive holds.
    #[must_use]
    pub fn row(&self, hash: WadHash) -> Option<&ChunkRow> {
        let at = self.hashes.binary_search(&hash).ok()?;
        Some(&self.rows[at])
    }

    /// The row of the chunk at `path`, hashed with [`chunk_hash`].
    #[must_use]
    pub fn row_by_path(&self, path: &str) -> Option<&ChunkRow> {
        self.row(chunk_hash(path))
    }

    /// Every holder of `hash`, in archive id order. Empty for an absent chunk.
    pub fn holders(&self, hash: WadHash) -> impl Iterator<Item = ArchiveId> + '_ {
        self.row(hash).into_iter().flat_map(ChunkRow::holders)
    }

    /// Whether any archive holds `hash`.
    #[must_use]
    pub fn contains(&self, hash: WadHash) -> bool {
        self.hashes.binary_search(&hash).is_ok()
    }

    /// Every chunk with its row, in ascending chunk hash order.
    pub fn chunks(&self) -> impl ExactSizeIterator<Item = (WadHash, &ChunkRow)> {
        self.hashes.iter().copied().zip(self.rows.iter())
    }

    /// The number of distinct chunks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no archive holds a chunk.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The archive holding the most of `hashes`.
    ///
    /// A tie goes to the lower id. `None` when no hash is present.
    #[must_use]
    pub fn dominant_holder(&self, hashes: &[WadHash]) -> Option<ArchiveId> {
        let mut hits = vec![0u32; self.archives.len()];
        for &hash in hashes {
            for holder in self.holders(hash) {
                hits[holder.index()] += 1;
            }
        }
        let (best, count) =
            hits.iter()
                .enumerate()
                .fold((0, 0), |(best, most), (index, &count)| {
                    if count > most {
                        (index, count)
                    } else {
                        (best, most)
                    }
                });
        (count > 0).then_some(ArchiveId(best as u32))
    }
}
