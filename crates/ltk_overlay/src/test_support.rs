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
