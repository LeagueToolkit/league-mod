//! The installation the overlay patches: its directory, the overlay's state directory, and
//! the overlay's view of the game index.
//!
//! [`ltk_game_index::GameIndex`] holds every chunk of the installation with every archive
//! holding it. The overlay routes overrides by the game-relative path of an archive,
//! `DATA/FINAL/<name>`. `GameIndexExt` carries the overlay's rules over the index: which
//! archives are locale archives, which chunk hashes are SubChunkTOC entries, and how a game
//! chunk's content hash is computed.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::File;

use camino::{Utf8Path, Utf8PathBuf};
use ltk_game_index::{ArchiveId, GameIndex, WadHash};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh64::xxh64;

use crate::error::{Error, GameDirError, Result};
use crate::utils::ContentHash;

/// The directory under the game directory that holds every archive.
pub const ARCHIVE_ROOT: &str = "DATA/FINAL";

/// The League `Game` directory: the installation the overlay patches.
///
/// A game directory contains `DATA/FINAL`, under which every `.wad.client` lives. Every path
/// the overlay records for a game archive is relative to this directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameDir(Utf8PathBuf);

impl GameDir {
    /// The game directory at `path`. The layout is checked by [`data_final`](Self::data_final).
    pub fn new(path: impl Into<Utf8PathBuf>) -> Self {
        Self(path.into())
    }

    /// The directory as a path.
    #[must_use]
    pub fn as_path(&self) -> &Utf8Path {
        &self.0
    }

    /// The `DATA/FINAL` directory.
    ///
    /// # Errors
    ///
    /// [`GameDirError::MissingDataFinal`] when the directory does not exist.
    pub fn data_final(&self) -> std::result::Result<Utf8PathBuf, GameDirError> {
        let data_final = self.0.join(ARCHIVE_ROOT);
        if !data_final.is_dir() {
            return Err(GameDirError::MissingDataFinal {
                path: self.0.clone(),
            });
        }
        Ok(data_final)
    }

    /// The absolute path of the game-relative `rel_path`.
    #[must_use]
    pub fn join(&self, rel_path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
        self.0.join(rel_path)
    }

    /// Loads the chunk index cached under `state_dir`, or builds it and writes the cache.
    ///
    /// Every skipped archive is logged at warn. A build never fails on one.
    ///
    /// # Errors
    ///
    /// [`GameDirError::MissingDataFinal`] when the layout is wrong, and
    /// [`Error::GameIndex`] when the archives cannot be enumerated.
    pub fn load_or_build_index(&self, state_dir: &StateDir) -> Result<GameIndex> {
        let index = GameIndex::load_or_build(&self.0, &state_dir.game_index_cache())?;
        for skipped in index.skipped() {
            let archive = index.archive(skipped.archive);
            tracing::warn!(
                "Game archive {} is not indexed: {}",
                archive.name,
                skipped.error
            );
        }
        Ok(index)
    }

    /// Reads and decompresses one chunk of the archive at the game-relative `wad_rel_path`.
    ///
    /// # Errors
    ///
    /// [`Error::Read`] when the archive does not open, [`Error::Wad`] when it does not mount or
    /// the chunk does not decompress, and [`GameDirError::StringtableChunkMissing`] when the
    /// archive does not hold `chunk_hash`.
    pub(crate) fn read_chunk(
        &self,
        wad_rel_path: &Utf8Path,
        chunk_hash: WadHash,
    ) -> Result<Vec<u8>> {
        let abs_path = self.join(wad_rel_path);
        let file =
            File::open(abs_path.as_std_path()).map_err(|source| Error::read(&abs_path, source))?;
        let mut wad = ltk_wad::Wad::mount(file)?;

        let chunk =
            *wad.chunks()
                .get(chunk_hash)
                .ok_or_else(|| GameDirError::StringtableChunkMissing {
                    wad: wad_rel_path.to_path_buf(),
                    chunk_hash,
                })?;

        Ok(wad.load_chunk_decompressed(&chunk)?.to_vec())
    }
}

impl fmt::Display for GameDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl AsRef<Utf8Path> for GameDir {
    fn as_ref(&self) -> &Utf8Path {
        &self.0
    }
}

/// The overlay's state directory: `overlay.json` and the caches of one overlay.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateDir(Utf8PathBuf);

impl StateDir {
    /// The chunk index cache file name.
    pub const GAME_INDEX_CACHE: &str = "game_index.bin";
    /// The override metadata cache file name.
    pub const OVERRIDE_META_CACHE: &str = "override_meta.bin";
    /// The overlay state file name.
    pub const OVERLAY_STATE: &str = "overlay.json";

    /// The state directory at `path`. Created by [`create`](Self::create).
    pub fn new(path: impl Into<Utf8PathBuf>) -> Self {
        Self(path.into())
    }

    /// The directory as a path.
    #[must_use]
    pub fn as_path(&self) -> &Utf8Path {
        &self.0
    }

    /// Creates the directory and its parents.
    ///
    /// # Errors
    ///
    /// [`Error::Write`] when the directory cannot be created.
    pub fn create(&self) -> Result<()> {
        std::fs::create_dir_all(self.0.as_std_path())
            .map_err(|source| Error::write(&self.0, source))
    }

    /// The chunk index cache, `game_index.bin`.
    #[must_use]
    pub fn game_index_cache(&self) -> Utf8PathBuf {
        self.0.join(Self::GAME_INDEX_CACHE)
    }

    /// The override metadata cache, `override_meta.bin`.
    #[must_use]
    pub fn override_meta_cache(&self) -> Utf8PathBuf {
        self.0.join(Self::OVERRIDE_META_CACHE)
    }

    /// The overlay state, `overlay.json`.
    #[must_use]
    pub fn overlay_state(&self) -> Utf8PathBuf {
        self.0.join(Self::OVERLAY_STATE)
    }
}

impl fmt::Display for StateDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl AsRef<Utf8Path> for StateDir {
    fn as_ref(&self) -> &Utf8Path {
        &self.0
    }
}

/// A game archive the index build could not read.
///
/// Reported on the build result, never fatal. The archive's chunks are unknown to the build.
/// An override the archive holds a copy of is routed to the other holders only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedGameArchive {
    /// `DATA/FINAL`-relative name: `Champions/Aatrox.wad.client`.
    pub name: String,
    /// Absolute path of the file.
    pub path: Utf8PathBuf,
    /// Why the archive did not read.
    pub error: String,
}

/// The overlay's rules over the game index.
///
/// Every method is a function of the index alone. Game-layout policy over archive names lives
/// here and not in `ltk_game_index`.
pub(crate) trait GameIndexExt {
    /// The game-relative path of an archive: `DATA/FINAL/<name>`.
    fn wad_rel_path(&self, id: ArchiveId) -> Utf8PathBuf;

    /// Whether `path` is the game-relative path of the archive `id`.
    fn is_wad_rel_path(&self, id: ArchiveId, path: &Utf8Path) -> bool;

    /// The game-relative path of every holder of `hash`, in archive id order.
    fn holder_paths(&self, hash: WadHash) -> impl Iterator<Item = Utf8PathBuf> + '_;

    /// Every archive the index build skipped, in archive id order.
    fn skipped_archives(&self) -> Vec<SkippedGameArchive>;

    /// Locales with a `Global.{locale}.wad.client` in the game, each with its archive.
    ///
    /// Locales are lowercase (`en_us`) and sorted. The unlocalized `Global.wad.client` is not
    /// a locale. A file name shared by several archives names no locale.
    fn localized_global_wads(&self) -> Vec<(String, ArchiveId)>;

    /// The chunk hashes of every archive's `.wad.SubChunkTOC`.
    ///
    /// A mod override with one of these hashes is stripped from the build. The path is the
    /// game-relative archive path with `.client` replaced by `.SubChunkTOC`, lowercased, and
    /// hashed with XXH64 seed zero.
    fn subchunktoc_blocked(&self) -> HashSet<WadHash>;

    /// The content hash of the game's copy of every chunk in `hashes` the index holds.
    ///
    /// The client requires every copy of a shared chunk to be byte-identical. Each chunk reads
    /// from its first holder. Archives are read in parallel, each once. A chunk that does not
    /// read is left out.
    fn content_hashes(&self, hashes: &HashSet<WadHash>) -> HashMap<WadHash, ContentHash>;
}

impl GameIndexExt for GameIndex {
    fn wad_rel_path(&self, id: ArchiveId) -> Utf8PathBuf {
        Utf8PathBuf::from(format!("{ARCHIVE_ROOT}/{}", self.archive(id).name))
    }

    fn is_wad_rel_path(&self, id: ArchiveId, path: &Utf8Path) -> bool {
        path.strip_prefix(ARCHIVE_ROOT)
            .is_ok_and(|name| name.as_str() == self.archive(id).name)
    }

    fn holder_paths(&self, hash: WadHash) -> impl Iterator<Item = Utf8PathBuf> + '_ {
        self.holders(hash).map(move |id| self.wad_rel_path(id))
    }

    fn skipped_archives(&self) -> Vec<SkippedGameArchive> {
        self.skipped()
            .iter()
            .map(|skipped| {
                let archive = self.archive(skipped.archive);
                SkippedGameArchive {
                    name: archive.name.clone(),
                    path: archive.path.clone(),
                    error: skipped.error.to_string(),
                }
            })
            .collect()
    }

    fn localized_global_wads(&self) -> Vec<(String, ArchiveId)> {
        let mut out: Vec<(String, ArchiveId)> = self
            .archives()
            .iter()
            .filter_map(|archive| {
                let file_name = archive.file_name().to_ascii_lowercase();
                let locale = file_name
                    .strip_prefix("global.")?
                    .strip_suffix(".wad.client")?;
                if locale.is_empty() || locale.contains('.') {
                    return None;
                }
                let id = self.archive_by_file_name(&file_name).ok()?;
                Some((locale.to_owned(), id))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn subchunktoc_blocked(&self) -> HashSet<WadHash> {
        self.archives()
            .iter()
            .filter_map(|archive| {
                let name = archive.name.to_ascii_lowercase();
                let stem = name.strip_suffix(".client")?;
                let toc_path = format!("{ARCHIVE_ROOT}/{stem}.subchunktoc").to_lowercase();
                Some(WadHash(xxh64(toc_path.as_bytes(), 0)))
            })
            .collect()
    }

    fn content_hashes(&self, hashes: &HashSet<WadHash>) -> HashMap<WadHash, ContentHash> {
        let mut by_archive: HashMap<ArchiveId, Vec<WadHash>> = HashMap::new();
        for &hash in hashes {
            if let Some(row) = self.row(hash) {
                by_archive.entry(row.first_holder()).or_default().push(hash);
            }
        }

        let archives = by_archive.len();
        let result: HashMap<WadHash, ContentHash> = by_archive
            .into_par_iter()
            .flat_map_iter(|(id, wanted)| content_hashes_in(&self.archive(id).path, &wanted))
            .collect();

        tracing::info!(
            "Computed {} content hashes on demand from {archives} game archives",
            result.len()
        );
        result
    }
}

/// The `(chunk hash, content hash)` of every chunk of `wanted` in the archive at `path`.
///
/// An archive that does not open or mount, or a chunk that does not decompress, is logged and
/// contributes nothing.
fn content_hashes_in(path: &Utf8Path, wanted: &[WadHash]) -> Vec<(WadHash, ContentHash)> {
    let file = match File::open(path.as_std_path()) {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!("Failed to open game archive '{path}': {e}");
            return Vec::new();
        }
    };
    let mut wad = match ltk_wad::Wad::mount(file) {
        Ok(wad) => wad,
        Err(e) => {
            tracing::warn!("Failed to mount game archive '{path}': {e}");
            return Vec::new();
        }
    };

    let mut hashes = Vec::with_capacity(wanted.len());
    for &hash in wanted {
        let Some(chunk) = wad.chunks().get(hash).copied() else {
            continue;
        };
        match wad.load_chunk_decompressed(&chunk) {
            Ok(data) => hashes.push((hash, ContentHash::of(&data))),
            Err(e) => tracing::trace!("Failed to decompress chunk {hash:016x} in '{path}': {e}"),
        }
    }
    hashes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{GameFixture, game_index_with_hashes, hash};

    #[test]
    fn a_game_dir_without_data_final_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap());
        assert!(matches!(
            game.data_final(),
            Err(GameDirError::MissingDataFinal { .. })
        ));

        let fixture = GameFixture::new();
        assert_eq!(
            fixture.game().data_final().unwrap(),
            fixture.game_dir.join("DATA/FINAL")
        );
    }

    #[test]
    fn a_state_dir_names_its_files() {
        let state = StateDir::new("/tmp/state");
        assert_eq!(state.game_index_cache(), "/tmp/state/game_index.bin");
        assert_eq!(state.override_meta_cache(), "/tmp/state/override_meta.bin");
        assert_eq!(state.overlay_state(), "/tmp/state/overlay.json");
        assert_eq!(state.to_string(), "/tmp/state");
    }

    #[test]
    fn subchunktoc_blocked_hashes_every_archive_toc_path() {
        let (_fixture, index) = game_index_with_hashes(&[
            ("DATA/FINAL/Champions/Aatrox.WAD.CLIENT", &[WadHash(1)]),
            ("DATA/FINAL/Maps/Map11.wad.client", &[WadHash(2)]),
        ]);

        let blocked = index.subchunktoc_blocked();
        assert_eq!(blocked.len(), 2);
        let expected = xxh64(b"data/final/champions/aatrox.wad.subchunktoc", 0);
        assert!(blocked.contains(&WadHash(expected)));
    }

    #[test]
    fn localized_global_wads_lists_each_locale_once_and_skips_the_plain_global() {
        let (_fixture, index) = game_index_with_hashes(&[
            (
                "DATA/FINAL/Localized/Global.ko_KR.wad.client",
                &[WadHash(1)],
            ),
            (
                "DATA/FINAL/Localized/Global.en_US.wad.client",
                &[WadHash(2)],
            ),
            ("DATA/FINAL/Global.wad.client", &[WadHash(3)]),
            ("DATA/FINAL/Champions/Aatrox.wad.client", &[WadHash(4)]),
        ]);

        let locales: Vec<(String, String)> = index
            .localized_global_wads()
            .into_iter()
            .map(|(locale, id)| (locale, index.wad_rel_path(id).into_string()))
            .collect();
        assert_eq!(
            locales,
            [
                (
                    "en_us".to_owned(),
                    "DATA/FINAL/Localized/Global.en_US.wad.client".to_owned()
                ),
                (
                    "ko_kr".to_owned(),
                    "DATA/FINAL/Localized/Global.ko_KR.wad.client".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn wad_rel_path_round_trips_through_is_wad_rel_path() {
        let (_fixture, index) =
            game_index_with_hashes(&[("DATA/FINAL/Champions/Aatrox.wad.client", &[WadHash(1)])]);
        let id = index.archive_by_file_name("aatrox.wad.client").unwrap();
        let path = index.wad_rel_path(id);
        assert_eq!(path, "DATA/FINAL/Champions/Aatrox.wad.client");
        assert!(index.is_wad_rel_path(id, &path));
        assert!(!index.is_wad_rel_path(id, Utf8Path::new("DATA/FINAL/Champions/Ahri.wad.client")));
        assert_eq!(index.holder_paths(WadHash(1)).collect::<Vec<_>>(), [path]);
        assert_eq!(index.holder_paths(WadHash(9)).count(), 0);
    }

    #[test]
    fn content_hashes_come_from_the_first_holder() {
        let fixture = GameFixture::new();
        fixture.write_wad(
            "DATA/FINAL/B.wad.client",
            &[
                ("assets/shared.tex", b"same"),
                ("assets/only_b.tex", b"bee"),
            ],
        );
        fixture.write_wad("DATA/FINAL/A.wad.client", &[("assets/shared.tex", b"same")]);
        let index = GameIndex::build(&fixture.game_dir).unwrap();

        let shared = hash("assets/shared.tex");
        let only_b = hash("assets/only_b.tex");
        let hashes = index.content_hashes(&HashSet::from([shared, only_b, WadHash(7)]));
        assert_eq!(hashes.len(), 2);
        assert_eq!(hashes[&shared], ContentHash::of(b"same"));
        assert_eq!(hashes[&only_b], ContentHash::of(b"bee"));

        assert_eq!(
            fixture
                .game()
                .read_chunk(Utf8Path::new("DATA/FINAL/B.wad.client"), only_b)
                .unwrap(),
            b"bee"
        );
    }

    #[test]
    fn a_skipped_archive_is_reported_and_the_index_still_loads() {
        let fixture = GameFixture::new();
        fixture.write_wad("DATA/FINAL/A.wad.client", &[("a", b"1")]);
        std::fs::write(fixture.game_dir.join("DATA/FINAL/B.wad.client"), b"broken").unwrap();
        let state = StateDir::new(fixture.game_dir.join("state"));
        state.create().unwrap();

        let index = fixture.game().load_or_build_index(&state).unwrap();
        let skipped = index.skipped_archives();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "B.wad.client");
        assert!(state.game_index_cache().exists());

        let again = fixture.game().load_or_build_index(&state).unwrap();
        assert_eq!(again.skipped_archives(), skipped);
    }
}
