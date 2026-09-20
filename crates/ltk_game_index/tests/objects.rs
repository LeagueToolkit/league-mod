//! The object index: classification, declarations order, lookups, cancellation, cache.

#![cfg(feature = "objects")]

mod common;

use std::collections::HashMap;
use std::io::Cursor;
use std::num::NonZeroUsize;

use common::Installation;
use ltk_game_index::{
    BinHash, BuildOptions, CacheError, Declaration, GameIndex, ObjectBuildError, ObjectIndex,
    ResolveWadPath, WadHash, chunk_hash, for_each_declaration,
};
use ltk_meta::{Bin, BinObject, BinOverride};

/// A `PROP` declaring `objects` as `(object, class)` pairs.
fn prop(objects: &[(u32, u32)]) -> Vec<u8> {
    let bin = Bin::new(
        objects
            .iter()
            .map(|&(object, class)| BinObject::new(object, class)),
        Vec::<String>::new(),
    );
    let mut cursor = Cursor::new(Vec::new());
    bin.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

/// A `PTCH` declaring `objects` as `(object, class)` pairs.
fn ptch(objects: &[(u32, u32)]) -> Vec<u8> {
    let mut patch = BinOverride::new();
    for &(object, class) in objects {
        patch
            .objects
            .insert(BinHash(object), BinObject::new(object, class));
    }
    let mut cursor = Cursor::new(Vec::new());
    patch.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

/// A resolver over a fixed name table, implementing the trait directly.
struct Names(HashMap<WadHash, String>);

impl Names {
    fn of(paths: &[&str]) -> Self {
        Self(
            paths
                .iter()
                .map(|path| (chunk_hash(path), (*path).to_owned()))
                .collect(),
        )
    }
}

impl ResolveWadPath for Names {
    fn for_each_named(&self, hashes: &[WadHash], visit: &mut dyn FnMut(usize, &str)) {
        for (index, hash) in hashes.iter().enumerate() {
            if let Some(path) = self.0.get(hash) {
                visit(index, path);
            }
        }
    }
}

/// A resolver through `ltk_wad::PathResolver`, which the blanket impl covers.
struct WadNames(HashMap<WadHash, String>);

impl ltk_wad::PathResolver for WadNames {
    fn resolve(&self, path_hash: WadHash) -> Option<String> {
        self.0.get(&path_hash).cloned()
    }
}

/// Build options with the given resolver, worker count and cancellation.
fn options<'a>(
    resolver: Option<&'a dyn ResolveWadPath>,
    workers: Option<usize>,
    called_off: Option<&'a (dyn Fn() -> bool + Sync)>,
) -> BuildOptions<'a> {
    let mut options = BuildOptions::default();
    options.resolver = resolver;
    options.workers = workers.and_then(NonZeroUsize::new);
    options.called_off = called_off;
    options
}

fn pairs(declarations: &[Declaration]) -> Vec<(u32, u32)> {
    declarations
        .iter()
        .map(|d| (d.object.0, d.class.0))
        .collect()
}

#[test]
fn for_each_declaration_reads_a_prop_and_a_ptch() {
    let mut seen = Vec::new();
    for_each_declaration(Cursor::new(prop(&[(1, 10), (2, 20)])), |o, c| {
        seen.push((o.0, c.0));
    })
    .unwrap();
    assert_eq!(seen, [(1, 10), (2, 20)]);

    let mut seen = Vec::new();
    for_each_declaration(Cursor::new(ptch(&[(3, 30)])), |o, c| seen.push((o.0, c.0))).unwrap();
    assert_eq!(seen, [(3, 30)]);

    assert!(for_each_declaration(Cursor::new(b"not a bin at all".to_vec()), |_, _| {}).is_err());
}

#[test]
fn named_bins_and_sniffed_chunks_contribute_and_other_named_chunks_are_never_read() {
    let install = Installation::new();
    install.write_archive(
        "A.wad.client",
        &[
            ("data/x.bin", &prop(&[(1, 10)])),
            ("data/trap.dds", &prop(&[(2, 20)])),
            ("data/scene", &prop(&[(3, 30)])),
            ("data/nothing.txt", b"plain text"),
            ("unnamed/patch", &ptch(&[(4, 40)])),
            ("unnamed/texture", b"DDS \x00\x00\x00\x00 not a bin"),
        ],
    );
    let game = GameIndex::build(install.game_dir()).unwrap();
    let names = Names::of(&[
        "data/x.bin",
        "data/trap.dds",
        "data/scene",
        "data/nothing.txt",
    ]);

    let index = ObjectIndex::build_with(&game, &options(Some(&names), None, None)).unwrap();

    assert!(index.declares(BinHash(1)));
    assert!(!index.declares(BinHash(2)));
    assert!(index.declares(BinHash(3)));
    assert!(index.declares(BinHash(4)));
    assert_eq!(index.len(), 3);
    assert_eq!(
        index.objects().collect::<Vec<_>>(),
        [BinHash(1), BinHash(3), BinHash(4)]
    );

    let stats = index.stats();
    assert_eq!(stats.archives, 1);
    // x.bin named, scene and patch sniffed as bins.
    assert_eq!(stats.bins, 3);
    // scene (bare) and the two unnamed chunks.
    assert_eq!(stats.sniffed, 3);
    assert_eq!(stats.sniffed_bins, 2);
    assert_eq!(stats.declarations, 3);
    assert_eq!(stats.skipped_chunks, 0);
    assert!(stats.bytes > 0);
    assert!(index.skipped().is_empty());
}

#[test]
fn declarations_store_in_archive_order_then_named_bare_unnamed() {
    let install = Installation::new();
    install.write_archive(
        "B.wad.client",
        &[
            ("b/one.bin", &prop(&[(1, 10)])),
            ("b/two.bin", &prop(&[(2, 20)])),
        ],
    );
    install.write_archive(
        "A.wad.client",
        &[
            ("a/named.bin", &prop(&[(3, 30)])),
            ("a/bare", &prop(&[(4, 40)])),
            ("a/unnamed", &prop(&[(5, 50)])),
        ],
    );
    let game = GameIndex::build(install.game_dir()).unwrap();
    let names = Names::of(&["b/one.bin", "b/two.bin", "a/named.bin", "a/bare"]);

    let index = ObjectIndex::build_with(&game, &options(Some(&names), Some(1), None)).unwrap();

    let a = game.archive_by_file_name("A.wad.client").unwrap();
    let b = game.archive_by_file_name("B.wad.client").unwrap();
    let all: Vec<&Declaration> = index
        .objects()
        .flat_map(|object| index.declarations(object))
        .collect();
    assert_eq!(all.len(), 5);

    // Archive A first, its named chunk, then bare, then unnamed.
    let by_chunk = |chunk: &str| {
        index
            .chunk_declarations(chunk_hash(chunk))
            .map(|d| (d.object.0, d.archive))
            .collect::<Vec<_>>()
    };
    assert_eq!(by_chunk("a/named.bin"), [(3, a)]);
    assert_eq!(by_chunk("a/bare"), [(4, a)]);
    assert_eq!(by_chunk("a/unnamed"), [(5, a)]);
    assert_eq!(by_chunk("b/one.bin"), [(1, b)]);
    assert!(by_chunk("absent").is_empty());

    let mut b_named = [chunk_hash("b/one.bin"), chunk_hash("b/two.bin")];
    b_named.sort();
    let b_order: Vec<WadHash> = [1u32, 2]
        .iter()
        .map(|&o| index.declarations(BinHash(o))[0].chunk)
        .collect();
    let mut expected: Vec<(WadHash, u32)> =
        vec![(chunk_hash("b/one.bin"), 1), (chunk_hash("b/two.bin"), 2)];
    expected.sort();
    // The two named chunks of B read in ascending hash order.
    assert_eq!(
        b_order
            .iter()
            .map(|h| expected.iter().position(|(e, _)| e == h).unwrap())
            .collect::<Vec<_>>(),
        if b_named[0] == chunk_hash("b/one.bin") {
            vec![0, 1]
        } else {
            vec![1, 0]
        }
    );
    let _ = expected;
}

#[test]
fn an_object_in_two_chunks_lists_both_in_archive_id_order_and_chunk_lookup_inverts() {
    let install = Installation::new();
    install.write_archive("B.wad.client", &[("b.bin", &prop(&[(7, 70), (8, 80)]))]);
    install.write_archive("A.wad.client", &[("a.bin", &prop(&[(7, 71)]))]);
    let game = GameIndex::build(install.game_dir()).unwrap();
    let a = game.archive_by_file_name("A.wad.client").unwrap();
    let b = game.archive_by_file_name("B.wad.client").unwrap();

    let index = ObjectIndex::build(&game).unwrap();

    let seven = index.declarations(BinHash(7));
    assert_eq!(seven.len(), 2);
    assert_eq!(seven[0].archive, a);
    assert_eq!(seven[0].chunk, chunk_hash("a.bin"));
    assert_eq!(seven[0].class, BinHash(71));
    assert_eq!(seven[1].archive, b);
    assert_eq!(seven[1].chunk, chunk_hash("b.bin"));
    assert_eq!(seven[1].class, BinHash(70));
    assert!(index.declarations(BinHash(9)).is_empty());

    let in_b: Vec<u32> = index
        .chunk_declarations(chunk_hash("b.bin"))
        .map(|d| d.object.0)
        .collect();
    assert_eq!(in_b, [7, 8]);
    assert_eq!(index.len(), 2);
    assert!(!index.is_empty());
}

/// A `PROP` declaring object 1 twice: object 2's hash is rewritten to 1 in the bytes.
fn prop_with_duplicate() -> Vec<u8> {
    let mut bytes = prop(&[(1, 10), (2, 11)]);
    let needle = 2u32.to_le_bytes();
    let at = bytes
        .windows(4)
        .rposition(|window| window == needle)
        .expect("object 2 is in the body");
    bytes[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes
}

#[test]
fn an_object_declared_twice_in_one_chunk_contributes_two_declarations() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("x.bin", &prop_with_duplicate())]);
    let game = GameIndex::build(install.game_dir()).unwrap();
    let index = ObjectIndex::build(&game).unwrap();
    assert_eq!(pairs(index.declarations(BinHash(1))), [(1, 10), (1, 11)]);
    assert_eq!(index.chunk_declarations(chunk_hash("x.bin")).count(), 2);
    assert_eq!(index.len(), 1);
}

#[test]
fn a_chunk_that_does_not_parse_is_counted_and_declares_nothing() {
    let install = Installation::new();
    let mut broken = prop(&[(1, 10)]);
    broken.truncate(12);
    install.write_archive(
        "A.wad.client",
        &[("broken.bin", &broken), ("fine.bin", &prop(&[(2, 20)]))],
    );
    let game = GameIndex::build(install.game_dir()).unwrap();
    let names = Names::of(&["broken.bin", "fine.bin"]);

    let index = ObjectIndex::build_with(&game, &options(Some(&names), None, None)).unwrap();
    assert_eq!(index.stats().skipped_chunks, 1);
    assert_eq!(index.stats().bins, 2);
    assert!(!index.declares(BinHash(1)));
    assert!(index.declares(BinHash(2)));
}

#[test]
fn a_called_off_build_is_called_off() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("a.bin", &prop(&[(1, 10)]))]);
    let game = GameIndex::build(install.game_dir()).unwrap();

    let stop = || true;
    let error = ObjectIndex::build_with(&game, &options(None, None, Some(&stop))).unwrap_err();
    assert!(matches!(error, ObjectBuildError::CalledOff));

    let go = || false;
    let index = ObjectIndex::build_with(&game, &options(None, None, Some(&go))).unwrap();
    assert!(index.declares(BinHash(1)));
}

#[test]
fn without_a_resolver_every_chunk_is_sniffed_and_declarations_match_the_named_build() {
    let install = Installation::new();
    install.write_archive(
        "A.wad.client",
        &[
            ("a/x.bin", &prop(&[(1, 10)])),
            ("a/scene", &prop(&[(2, 20)])),
            ("a/patch.bin", &ptch(&[(3, 30)])),
            ("a/tex.dds", b"DDS \x00\x00\x00\x00"),
        ],
    );
    install.write_archive("B.wad.client", &[("b/y.bin", &prop(&[(4, 40)]))]);
    let game = GameIndex::build(install.game_dir()).unwrap();
    let names = Names::of(&["a/x.bin", "a/scene", "a/patch.bin", "a/tex.dds", "b/y.bin"]);

    let named = ObjectIndex::build_with(&game, &options(Some(&names), None, None)).unwrap();
    let sniffed = ObjectIndex::build(&game).unwrap();

    assert_eq!(sniffed.stats().sniffed, game.len() as u32);
    assert_eq!(sniffed.stats().sniffed_bins, 4);
    assert_eq!(named.stats().sniffed, 1);
    for object in named.objects() {
        assert_eq!(named.declarations(object), sniffed.declarations(object));
    }
    assert_eq!(named.len(), sniffed.len());
}

#[test]
fn a_wad_path_resolver_names_chunks_through_the_blanket_impl() {
    let install = Installation::new();
    install.write_archive(
        "A.wad.client",
        &[
            ("x.bin", &prop(&[(1, 10)])),
            ("trap.dds", &prop(&[(2, 20)])),
        ],
    );
    let game = GameIndex::build(install.game_dir()).unwrap();
    let names = WadNames(
        ["x.bin", "trap.dds"]
            .into_iter()
            .map(|p| (chunk_hash(p), p.to_owned()))
            .collect(),
    );

    let index = ObjectIndex::build_with(&game, &options(Some(&names), None, None)).unwrap();
    assert!(index.declares(BinHash(1)));
    assert!(!index.declares(BinHash(2)));
    assert_eq!(index.stats().sniffed, 0);
}

#[test]
fn one_worker_and_many_workers_produce_equal_declarations() {
    let install = Installation::new();
    for archive in ["A", "B", "C", "D", "E", "F"] {
        let base = archive.as_bytes()[0] as u32 * 100;
        install.write_archive(
            &format!("{archive}.wad.client"),
            &[
                (
                    &format!("{archive}/one.bin"),
                    &prop(&[(base + 1, 1), (base + 2, 2)]),
                ),
                (&format!("{archive}/two.bin"), &prop(&[(base + 3, 3)])),
            ],
        );
    }
    let game = GameIndex::build(install.game_dir()).unwrap();

    let one = ObjectIndex::build_with(&game, &options(None, Some(1), None)).unwrap();
    let many = ObjectIndex::build_with(&game, &options(None, Some(4), None)).unwrap();

    assert_eq!(one.len(), 18);
    for (hash, _) in game.chunks() {
        assert_eq!(
            one.chunk_declarations(hash).collect::<Vec<_>>(),
            many.chunk_declarations(hash).collect::<Vec<_>>()
        );
    }
    assert_eq!(one.stats().workers, 1);
    assert_eq!(many.stats().workers, 4);

    // The same fixed content under every feature set: archive order, then hash order.
    let a = game.archive_by_file_name("A.wad.client").unwrap();
    let first: Vec<(u32, u32)> = many
        .chunk_declarations(chunk_hash("A/one.bin"))
        .map(|d| (d.object.0, d.class.0))
        .collect();
    assert_eq!(first, [(6501, 1), (6502, 2)]);
    assert!(
        many.chunk_declarations(chunk_hash("A/one.bin"))
            .all(|d| d.archive == a)
    );
}

#[test]
fn an_archive_that_does_not_mount_after_the_chunk_index_is_skipped() {
    let install = Installation::new();
    let a = install.write_archive("A.wad.client", &[("a.bin", &prop(&[(1, 10)]))]);
    install.write_archive("B.wad.client", &[("b.bin", &prop(&[(2, 20)]))]);
    let game = GameIndex::build(install.game_dir()).unwrap();
    std::fs::write(a.as_std_path(), b"gone bad").unwrap();

    let index = ObjectIndex::build(&game).unwrap();
    assert_eq!(index.stats().archives, 1);
    assert_eq!(index.skipped().len(), 1);
    assert_eq!(
        index.skipped()[0].archive,
        game.archive_by_file_name("A.wad.client").unwrap()
    );
    assert!(!index.declares(BinHash(1)));
    assert!(index.declares(BinHash(2)));
    assert_eq!(index.stats().skipped_chunks, 1);
}

#[test]
fn the_cache_round_trips_and_follows_the_chunk_index_fingerprint() {
    let install = Installation::new();
    let a = install.write_archive("A.wad.client", &[("a.bin", &prop(&[(1, 10), (2, 20)]))]);
    let game = GameIndex::build(install.game_dir()).unwrap();
    let path = install.game_dir().join("state").join("object_index.bin");

    let built = ObjectIndex::build(&game).unwrap();
    built.save(&path).unwrap();
    let loaded = ObjectIndex::load_for(&path, &game).unwrap();
    assert_eq!(loaded.fingerprint(), game.fingerprint());
    assert_eq!(loaded.stats(), built.stats());
    assert_eq!(loaded.len(), built.len());
    for object in built.objects() {
        assert_eq!(loaded.declarations(object), built.declarations(object));
    }
    assert_eq!(loaded.chunk_declarations(chunk_hash("a.bin")).count(), 2);

    common::write_archive(&a, &[("a.bin", &prop(&[(1, 10)]))]);
    let file = std::fs::File::options()
        .write(true)
        .open(a.as_std_path())
        .unwrap();
    let modified = file.metadata().unwrap().modified().unwrap();
    file.set_modified(modified + std::time::Duration::from_secs(1))
        .unwrap();
    let changed = GameIndex::build(install.game_dir()).unwrap();
    assert_ne!(changed.fingerprint(), game.fingerprint());

    let error = ObjectIndex::load_for(&path, &changed).unwrap_err();
    assert!(matches!(
        error,
        CacheError::Stale { cached, current, .. }
            if cached == game.fingerprint() && current == changed.fingerprint()
    ));

    let rebuilt =
        ObjectIndex::load_or_build_with(&changed, &path, &BuildOptions::default()).unwrap();
    assert_eq!(rebuilt.len(), 1);
    assert_eq!(ObjectIndex::load_for(&path, &changed).unwrap().len(), 1);
}
