//! Fixtures shared by the crate's own unit tests.
//!
//! `tests/common` carries the same two fixtures for the integration tests. The
//! duplication is the crate boundary, not an oversight: a `#[cfg(test)]` module
//! is compiled only into the lib's own test binary, and a `tests/` module is
//! compiled only into the integration ones, so neither can see the other.

use crate::utils::resolve_chunk_hash;
use camino::Utf8Path;
use ltk_wad::{WadBuilder, WadChunkBuilder, WadChunkCompression, WadHash};
use std::collections::BTreeMap;
use std::io::{Cursor, Write};

/// The path hash a chunk at `path` would have.
///
/// # Panics
///
/// Panics when the path cannot be hashed, which means the fixture is wrong.
pub(crate) fn hash(path: &str) -> WadHash {
    resolve_chunk_hash(Utf8Path::new(path), b"").expect("chunk path hashes")
}

/// Write an uncompressed game WAD holding `chunks` as `(path, bytes)` pairs.
///
/// Uncompressed on purpose: a test that stamps or inspects the copied region
/// needs to find the bytes it wrote.
///
/// # Panics
///
/// Panics when the fixture cannot be built or written.
pub(crate) fn write_game_wad(wad_path: &Utf8Path, chunks: &[(&str, &[u8])]) {
    std::fs::create_dir_all(wad_path.parent().expect("WAD has a parent").as_std_path())
        .expect("fixture directory is creatable");

    let mut builder = WadBuilder::default();
    for (path, _) in chunks {
        builder = builder.with_chunk(
            WadChunkBuilder::default()
                .with_path(*path)
                .with_force_compression(WadChunkCompression::None),
        );
    }

    let by_hash: BTreeMap<WadHash, Vec<u8>> = chunks
        .iter()
        .map(|(path, bytes)| (hash(path), bytes.to_vec()))
        .collect();

    let mut cursor = Cursor::new(Vec::new());
    builder
        .build_to_writer(&mut cursor, move |chunk_hash, writer| {
            writer.write_all(&by_hash[&chunk_hash])?;
            Ok(())
        })
        .expect("fixture WAD builds");
    std::fs::write(wad_path.as_std_path(), cursor.into_inner()).expect("fixture WAD writes");
}

/// A temporary game directory with a `DATA/FINAL` for fixture archives.
pub(crate) struct GameFixture {
    _dir: tempfile::TempDir,
    pub(crate) game_dir: camino::Utf8PathBuf,
}

impl GameFixture {
    /// A game directory with an empty `DATA/FINAL`.
    ///
    /// # Panics
    ///
    /// Panics when the directory cannot be created.
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().expect("temporary directory");
        let game_dir =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("Game")).expect("UTF-8 path");
        std::fs::create_dir_all(game_dir.join("DATA").join("FINAL")).expect("DATA/FINAL");
        Self {
            _dir: dir,
            game_dir,
        }
    }

    /// The directory as the overlay's game directory.
    pub(crate) fn game(&self) -> crate::game::GameDir {
        crate::game::GameDir::new(self.game_dir.clone())
    }

    /// Writes an uncompressed archive at the game-relative `rel_path` holding `chunks`.
    pub(crate) fn write_wad(&self, rel_path: &str, chunks: &[(&str, &[u8])]) {
        write_game_wad(&self.game_dir.join(rel_path), chunks);
    }

    /// Writes an uncompressed archive at the game-relative `rel_path` holding one tiny chunk
    /// per hash in `hashes`.
    pub(crate) fn write_wad_with_hashes(&self, rel_path: &str, hashes: &[WadHash]) {
        let wad_path = self.game_dir.join(rel_path);
        std::fs::create_dir_all(wad_path.parent().expect("WAD has a parent").as_std_path())
            .expect("fixture directory is creatable");
        let mut builder = WadBuilder::default();
        for hash in hashes {
            builder = builder.with_chunk(
                WadChunkBuilder::default()
                    .with_hash(*hash)
                    .with_force_compression(WadChunkCompression::None),
            );
        }
        let mut cursor = Cursor::new(Vec::new());
        builder
            .build_to_writer(&mut cursor, |hash, writer| {
                writer.write_all(&hash.0.to_le_bytes())?;
                Ok(())
            })
            .expect("fixture WAD builds");
        std::fs::write(wad_path.as_std_path(), cursor.into_inner()).expect("fixture WAD writes");
    }
}

/// A game index over archives at the given game-relative paths, each holding the given
/// chunk hashes.
///
/// The fixture must outlive the index for any test that reads chunk bytes.
pub(crate) fn game_index_with_hashes(
    wads: &[(&str, &[WadHash])],
) -> (GameFixture, ltk_game_index::GameIndex) {
    let fixture = GameFixture::new();
    for (rel_path, hashes) in wads {
        fixture.write_wad_with_hashes(rel_path, hashes);
    }
    let index = ltk_game_index::GameIndex::build(&fixture.game_dir).expect("fixture index builds");
    (fixture, index)
}
