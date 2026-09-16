//! A temporary installation with fixture archives written through `ltk_wad`'s builder.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_game_index::chunk_hash;
use ltk_wad::{WadBuilder, WadChunkBuilder, WadChunkCompression, WadHash};

/// A `Game` directory under a temporary root, with a `DATA/FINAL`.
pub struct Installation {
    _root: tempfile::TempDir,
    game_dir: Utf8PathBuf,
}

impl Installation {
    /// An installation with an empty `DATA/FINAL`.
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary directory");
        let game_dir = Utf8PathBuf::from_path_buf(root.path().join("Game")).expect("UTF-8 path");
        std::fs::create_dir_all(game_dir.join("DATA").join("FINAL")).expect("DATA/FINAL");
        Self {
            _root: root,
            game_dir,
        }
    }

    pub fn game_dir(&self) -> &Utf8Path {
        &self.game_dir
    }

    pub fn data_final(&self) -> Utf8PathBuf {
        self.game_dir.join("DATA").join("FINAL")
    }

    /// The absolute path of the archive named `name` relative to `DATA/FINAL`.
    pub fn archive_path(&self, name: &str) -> Utf8PathBuf {
        self.data_final().join(name)
    }

    /// Writes an uncompressed archive named `name` holding `chunks` as `(path, bytes)`.
    pub fn write_archive(&self, name: &str, chunks: &[(&str, &[u8])]) -> Utf8PathBuf {
        let path = self.archive_path(name);
        write_archive(&path, chunks);
        path
    }

    /// Writes a file at `name` that is not a WAD.
    pub fn write_garbage(&self, name: &str, bytes: &[u8]) -> Utf8PathBuf {
        let path = self.archive_path(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        std::fs::write(&path, bytes).expect("garbage file");
        path
    }
}

/// Writes an uncompressed archive at `path` holding `chunks` as `(path, bytes)` pairs.
pub fn write_archive(path: &Utf8Path, chunks: &[(&str, &[u8])]) {
    std::fs::create_dir_all(path.parent().expect("archive has a parent")).expect("directory");
    let mut builder = WadBuilder::default();
    for (chunk_path, _) in chunks {
        builder = builder.with_chunk(
            WadChunkBuilder::default()
                .with_path(*chunk_path)
                .with_force_compression(WadChunkCompression::None),
        );
    }
    let by_hash: BTreeMap<WadHash, Vec<u8>> = chunks
        .iter()
        .map(|(chunk_path, bytes)| (chunk_hash(chunk_path), bytes.to_vec()))
        .collect();
    let mut cursor = Cursor::new(Vec::new());
    builder
        .build_to_writer(&mut cursor, move |hash, writer| {
            writer.write_all(&by_hash[&hash])?;
            Ok(())
        })
        .expect("fixture archive builds");
    std::fs::write(path.as_std_path(), cursor.into_inner()).expect("fixture archive writes");
}
