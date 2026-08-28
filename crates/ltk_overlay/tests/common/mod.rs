//! Helpers shared by the overlay integration tests.
//!
//! The centrepiece is [`chunk_facts`]: everything a reader of a WAD can observe
//! about its chunks, with the file layout deliberately left out. Two WADs whose
//! facts are equal hold the same archive - the game cannot tell them apart -
//! even when their bytes sit at different offsets. That is the equivalence the
//! tail layout and the incremental rebuild are allowed to preserve, and
//! byte-for-byte file comparison is not.

// Each test binary uses a different subset of this module; `expect` would fire
// in the ones that leave a helper unused.
#![allow(dead_code)]

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_wad::{Wad, WadBuilder, WadChunkBuilder, WadChunkCompression};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Write};

/// Everything about one chunk that does not depend on where it sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkFacts {
    pub compressed: Vec<u8>,
    pub uncompressed_size: usize,
    pub compression_type: WadChunkCompression,
    pub frame_count: u8,
    pub start_frame: u32,
    pub checksum: u64,
}

/// Read every chunk of `wad_path` into its layout-independent facts.
///
/// # Panics
///
/// Panics when the WAD cannot be opened, mounted, or read - a test fixture that
/// does not parse is a broken test, not a failed assertion.
pub fn chunk_facts(wad_path: &Utf8Path) -> BTreeMap<u64, ChunkFacts> {
    let file = fs::File::open(wad_path.as_std_path()).expect("overlay WAD opens");
    let mut wad = Wad::mount(file).expect("overlay WAD mounts");
    let chunks: Vec<_> = wad.chunks().iter().copied().collect();

    chunks
        .iter()
        .map(|chunk| {
            let compressed = wad
                .load_chunk_raw(chunk)
                .expect("chunk data is inside the file")
                .to_vec();
            (
                chunk.path_hash.0,
                ChunkFacts {
                    compressed,
                    uncompressed_size: chunk.uncompressed_size,
                    compression_type: chunk.compression_type,
                    frame_count: chunk.frame_count,
                    start_frame: chunk.start_frame,
                    checksum: chunk.checksum,
                },
            )
        })
        .collect()
}

/// Assert that two WADs hold the same chunks with the same bytes, whatever
/// their layouts.
///
/// # Panics
///
/// Panics with the differing chunk hashes when the two archives are not
/// equivalent.
pub fn assert_chunks_equivalent(left: &Utf8Path, right: &Utf8Path) {
    let left_facts = chunk_facts(left);
    let right_facts = chunk_facts(right);

    let left_hashes: Vec<u64> = left_facts.keys().copied().collect();
    let right_hashes: Vec<u64> = right_facts.keys().copied().collect();
    assert_eq!(
        left_hashes, right_hashes,
        "chunk sets differ between {left} and {right}"
    );

    for (hash, expected) in &left_facts {
        assert_eq!(
            right_facts.get(hash),
            Some(expected),
            "chunk {hash:016x} differs between {left} and {right}"
        );
    }
}

/// Assert the archive-level invariants every patched WAD must satisfy.
///
/// # Panics
///
/// Panics when the TOC is not strictly ascending by path hash, when the chunk
/// count disagrees with the entries, or when a chunk's data range falls outside
/// the file.
pub fn assert_wad_is_well_formed(wad_path: &Utf8Path) {
    let len = fs::metadata(wad_path.as_std_path())
        .expect("overlay WAD exists")
        .len();
    let file = fs::File::open(wad_path.as_std_path()).expect("overlay WAD opens");
    let wad = Wad::mount(file).expect("overlay WAD mounts");

    let mut previous: Option<u64> = None;
    for chunk in wad.chunks() {
        if let Some(previous) = previous {
            assert!(
                chunk.path_hash.0 > previous,
                "{wad_path}: TOC must be strictly ascending by path hash, \
                 saw {previous:016x} then {:016x}",
                chunk.path_hash.0
            );
        }
        previous = Some(chunk.path_hash.0);

        let end = (chunk.data_offset + chunk.compressed_size) as u64;
        assert!(
            end <= len,
            "{wad_path}: chunk {:016x} ends at {end}, past the {len}-byte file",
            chunk.path_hash.0
        );
    }
}

/// Write a game WAD holding `chunks` as `(path, bytes)` pairs, uncompressed.
///
/// # Panics
///
/// Panics when the fixture cannot be written.
pub fn write_game_wad(wad_path: &Utf8Path, chunks: &[(&str, &[u8])]) {
    fs::create_dir_all(
        wad_path
            .parent()
            .expect("WAD path has a parent")
            .as_std_path(),
    )
    .expect("fixture directory is creatable");

    let mut builder = WadBuilder::default();
    for (path, _) in chunks {
        builder = builder.with_chunk(
            WadChunkBuilder::default()
                .with_path(*path)
                .with_force_compression(WadChunkCompression::None),
        );
    }

    let by_hash: BTreeMap<u64, Vec<u8>> = chunks
        .iter()
        .map(|(path, bytes)| {
            (
                ltk_overlay::utils::resolve_chunk_hash(Utf8Path::new(path), b"")
                    .expect("chunk path hashes"),
                bytes.to_vec(),
            )
        })
        .collect();

    let mut cursor = Cursor::new(Vec::new());
    builder
        .build_to_writer(&mut cursor, move |hash, writer| {
            writer.write_all(&by_hash[&hash.0])?;
            Ok(())
        })
        .expect("fixture WAD builds");

    fs::write(wad_path.as_std_path(), cursor.into_inner()).expect("fixture WAD is writable");
}

/// Write a mod project directory whose base layer overrides `chunks` inside
/// `wad_name`, and return its path.
///
/// # Panics
///
/// Panics when the fixture cannot be written.
pub fn write_mod_dir(
    root: &Utf8Path,
    name: &str,
    wad_name: &str,
    chunks: &[(&str, &[u8])],
) -> Utf8PathBuf {
    let mod_dir = root.join(name);
    let wad_dir = mod_dir.join("content").join("base").join(wad_name);
    for (path, bytes) in chunks {
        let file = wad_dir.join(path);
        fs::create_dir_all(file.parent().expect("override has a parent").as_std_path())
            .expect("override directory is creatable");
        fs::write(file.as_std_path(), bytes).expect("override is writable");
    }

    let project = ModProject {
        name: name.to_string(),
        display_name: name.to_string(),
        version: "1.0.0".to_string(),
        description: String::new(),
        authors: vec![],
        license: None,
        tags: vec![],
        champions: vec![],
        maps: vec![],
        transformers: vec![],
        layers: vec![ModProjectLayer {
            name: "base".to_string(),
            display_name: None,
            priority: 0,
            description: None,
            string_overrides: Default::default(),
        }],
        thumbnail: None,
        hashtables: vec![],
    };
    fs::write(
        mod_dir.join("mod.config.json").as_std_path(),
        serde_json::to_string_pretty(&project).expect("mod project serializes"),
    )
    .expect("mod config is writable");

    mod_dir
}
