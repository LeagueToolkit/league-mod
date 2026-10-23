//! The search feature: term index, name sheets, matches and pages, the object tree.

#![cfg(feature = "search")]

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU32, Ordering};

use common::Installation;
use ltk_game_index::{
    BinHash, BuildOptions, CacheError, ChunkKind, DeclarationId, Dir, GameIndex, GameSearch,
    ListOptions, MatchedTerm, NameSheets, NameSource, NameTable, ObjectBuildError, ObjectIndex,
    Query, ResolveWadPath, TERM_CACHE_FORMAT_VERSION, TermIndex, TermKind, TermKinds, TreeEntry,
    WadHash, chunk_hash,
};
use ltk_meta::property::values;
use ltk_meta::{Bin, BinObject, BinOverride, PropertyValueEnum};

const AATROX: u32 = 0x0000_0100;
const AHRI: u32 = 0x0000_0200;
const PATCHED: u32 = 0x0000_0300;
const SKIN_CLASS: u32 = 0x0000_C001;
const DATA_CLASS: u32 = 0x0000_C002;
const VFX_CLASS: u32 = 0x0000_E001;
const MESH_FIELD: u32 = 0x0000_F001;
const LINK_FIELD: u32 = 0x0000_F002;
const VFX_FIELD: u32 = 0x0000_F003;
const COUNT_FIELD: u32 = 0x0000_F004;
const EMBED_FIELD: u32 = 0x0000_F005;
const SOUND_HASH: u32 = 0x0000_4A5E;

fn write_prop(objects: Vec<BinObject>) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    Bin::new(objects, Vec::<String>::new())
        .to_writer(&mut cursor)
        .unwrap();
    cursor.into_inner()
}

fn write_ptch(objects: Vec<BinObject>) -> Vec<u8> {
    let mut patch = BinOverride::new();
    for object in objects {
        patch.objects.insert(object.path_hash, object);
    }
    let mut cursor = Cursor::new(Vec::new());
    patch.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

fn string(value: &str) -> values::String {
    values::String {
        value: value.into(),
    }
}

/// Aatrox: a mesh string, a link to Ahri, a sound hash, a chunk link and an embedded VFX
/// class. Ahri: a number and a string spelling Ahri's own hash.
fn skins() -> Vec<u8> {
    let vfx = values::Embedded(values::Struct {
        class_hash: BinHash(VFX_CLASS),
        properties: [(
            BinHash(COUNT_FIELD),
            PropertyValueEnum::U32(values::U32::new(3)),
        )]
        .into_iter()
        .collect(),
    });
    let aatrox = BinObject::builder(AATROX, SKIN_CLASS)
        .property(
            MESH_FIELD,
            string("ASSETS/Characters/Aatrox/Skins/Base/Aatrox.skn"),
        )
        .property(
            LINK_FIELD,
            values::ObjectLink {
                value: BinHash(AHRI),
            },
        )
        .property(
            COUNT_FIELD,
            values::Hash {
                value: ltk_hash::HashValue::narrow(SOUND_HASH),
            },
        )
        .property(
            VFX_FIELD,
            values::WadChunkLink {
                value: chunk_hash("data/patch.bin"),
            },
        )
        .property(EMBED_FIELD, vfx)
        .build();
    let ahri = BinObject::builder(AHRI, SKIN_CLASS)
        .property(COUNT_FIELD, values::U32::new(9))
        .property(MESH_FIELD, string("see 00000200"))
        .build();
    write_prop(vec![aatrox, ahri])
}

fn patch() -> Vec<u8> {
    write_ptch(vec![
        BinObject::builder(PATCHED, DATA_CLASS)
            .property(MESH_FIELD, string("Patched"))
            .build(),
    ])
}

/// A consumer's tables, counting each cut.
#[derive(Default)]
struct Tables {
    names: HashMap<NameTable, Vec<(u64, String)>>,
    versions: HashMap<NameTable, Option<String>>,
    cuts: RefCell<HashMap<NameTable, u32>>,
}

impl Tables {
    fn standard() -> Self {
        let mut tables = Self::default();
        tables.set(
            NameTable::Entries,
            &[
                (u64::from(AATROX), "Characters/Aatrox/Skins/Skin0"),
                (u64::from(AHRI), "Characters/Ahri/Skins/Skin0"),
                (u64::from(PATCHED), "Items/Patched"),
            ],
        );
        tables.set(
            NameTable::Types,
            &[
                (u64::from(SKIN_CLASS), "SkinCharacterDataProperties"),
                (u64::from(VFX_CLASS), "VfxSystemDefinitionData"),
            ],
        );
        tables.set(
            NameTable::Fields,
            &[
                (u64::from(MESH_FIELD), "simpleSkin"),
                (u64::from(LINK_FIELD), "linked"),
            ],
        );
        tables.set(NameTable::Hashes, &[(u64::from(SOUND_HASH), "Play_Aatrox")]);
        tables.set(
            NameTable::Chunks,
            &[
                (chunk_hash("data/skin2.bin").0, "data/skin2.bin"),
                (chunk_hash("data/skin10.bin").0, "data/skin10.bin"),
                (chunk_hash("data/patch.bin").0, "data/patch.bin"),
                (chunk_hash("data/absent.bin").0, "data/absent.bin"),
                (
                    chunk_hash("assets/characters/aatrox/aatrox.dds").0,
                    "assets/characters/aatrox/aatrox.dds",
                ),
            ],
        );
        for table in NameTable::ALL {
            tables.versions.insert(table, Some(format!("{table}-1")));
        }
        tables
    }

    fn set(&mut self, table: NameTable, names: &[(u64, &str)]) {
        self.names.insert(
            table,
            names
                .iter()
                .map(|&(hash, name)| (hash, name.to_owned()))
                .collect(),
        );
    }

    fn cuts(&self, table: NameTable) -> u32 {
        self.cuts.borrow().get(&table).copied().unwrap_or(0)
    }
}

impl NameSource for Tables {
    fn version(&self, table: NameTable) -> Option<String> {
        self.versions.get(&table).cloned().flatten()
    }

    fn for_each_name(&self, table: NameTable, visit: &mut dyn FnMut(u64, &str)) {
        *self.cuts.borrow_mut().entry(table).or_default() += 1;
        for (hash, name) in self.names.get(&table).into_iter().flatten() {
            visit(*hash, name);
        }
    }
}

impl ResolveWadPath for Tables {
    fn for_each_named(&self, hashes: &[WadHash], visit: &mut dyn FnMut(usize, &str)) {
        let chunks: HashMap<u64, &str> = self.names[&NameTable::Chunks]
            .iter()
            .map(|(hash, name)| (*hash, name.as_str()))
            .collect();
        for (index, hash) in hashes.iter().enumerate() {
            if let Some(name) = chunks.get(&hash.0) {
                visit(index, name);
            }
        }
    }
}

/// Archive `A` holds the skins under two names and an unnamed copy; `B` holds the patch.
fn installation() -> (Installation, GameIndex) {
    let install = Installation::new();
    install.write_archive(
        "A.wad.client",
        &[
            ("data/skin10.bin", &skins()),
            ("data/skin2.bin", &skins()),
            ("data/unnamed", &skins()),
        ],
    );
    install.write_archive("B.wad.client", &[("data/patch.bin", &patch())]);
    let game = GameIndex::build(install.game_dir()).unwrap();
    (install, game)
}

fn options(resolver: Option<&dyn ResolveWadPath>) -> BuildOptions<'_> {
    let mut options = BuildOptions::default();
    options.resolver = resolver;
    options.workers = NonZeroUsize::new(2);
    options
}

fn search() -> (Installation, GameIndex, GameSearch) {
    let (install, game) = installation();
    let tables = Tables::standard();
    let terms = TermIndex::build_with(&game, &options(Some(&tables))).unwrap();
    let names = NameSheets::build(&tables, &game);
    (install, game, GameSearch::new(terms, names))
}

fn ids(posting: &[DeclarationId]) -> Vec<usize> {
    posting.iter().map(|id| id.index()).collect()
}

#[test]
fn a_prop_and_a_ptch_post_every_kind_of_term() {
    let (_install, game) = installation();
    let terms = TermIndex::build(&game).unwrap();
    let objects = terms.objects();

    let of = |object: u32| -> Vec<usize> {
        objects
            .declarations(BinHash(object))
            .map(|(id, _)| id.index())
            .collect()
    };
    let aatrox = of(AATROX);
    let ahri = of(AHRI);
    let patched = of(PATCHED);
    assert_eq!(aatrox.len(), 3);
    assert_eq!(patched.len(), 1);

    let post = |kind, hash: u64| ids(terms.hash_postings(kind, hash));
    assert_eq!(post(TermKind::Object, u64::from(AATROX)), aatrox);
    let mut skin_class = [aatrox.clone(), ahri.clone()].concat();
    skin_class.sort_unstable();
    assert_eq!(post(TermKind::Class, u64::from(SKIN_CLASS)), skin_class);
    assert!(post(TermKind::Class, u64::from(VFX_CLASS)).is_empty());
    assert_eq!(post(TermKind::Embedded, u64::from(VFX_CLASS)), aatrox);
    assert_eq!(post(TermKind::Field, u64::from(COUNT_FIELD)), skin_class);
    assert_eq!(post(TermKind::Hash, u64::from(SOUND_HASH)), aatrox);
    assert_eq!(post(TermKind::Link, u64::from(AHRI)), aatrox);
    assert_eq!(post(TermKind::Wad, chunk_hash("data/patch.bin").0), aatrox);
    assert!(post(TermKind::String, 0).is_empty());

    let strings = terms.strings();
    let mesh = strings.scan("aatrox.skn");
    assert_eq!(mesh.len(), 1);
    assert_eq!(
        strings.entry(mesh[0]),
        "assets/characters/aatrox/skins/base/aatrox.skn"
    );
    assert_eq!(ids(terms.string_postings(mesh[0])), aatrox);
    let patched_string = strings.scan("patched")[0];
    assert_eq!(ids(terms.string_postings(patched_string)), patched);

    let row = objects.row(DeclarationId::new(patched[0] as u32)).unwrap();
    assert_eq!(row.kind, ChunkKind::Patch);
    assert_eq!(row.class, BinHash(DATA_CLASS));
    assert_eq!(terms.stats().string_terms, 3);
    assert_eq!(terms.stats().failed_chunks, 0);
}

#[test]
fn postings_in_unions_the_kinds_asked_for() {
    let (_install, game) = installation();
    let terms = TermIndex::build(&game).unwrap();
    let linked = terms.postings_in(
        TermKinds::of([TermKind::Object, TermKind::Link]),
        u64::from(AHRI),
    );
    assert_eq!(linked.len(), 6);
    assert!(linked.is_sorted());
    assert_eq!(
        terms
            .postings_in(TermKinds::from(TermKind::Object), u64::from(AHRI))
            .len(),
        3
    );
    assert!(
        terms
            .postings_in(TermKinds::NONE, u64::from(AHRI))
            .is_empty()
    );
}

#[test]
fn the_term_index_holds_the_object_index_of_the_same_build_ids_included() {
    let (_install, game) = installation();
    let tables = Tables::standard();
    for resolver in [None, Some(&tables as &dyn ResolveWadPath)] {
        let terms = TermIndex::build_with(&game, &options(resolver)).unwrap();
        let objects = ObjectIndex::build_with(&game, &options(resolver)).unwrap();
        assert_eq!(terms.objects().rows(), objects.rows());
        assert_eq!(terms.objects().skipped(), objects.skipped());
        assert_eq!(terms.objects().stats().bins, objects.stats().bins);
        assert_eq!(
            terms.objects().stats().skipped_chunks,
            objects.stats().skipped_chunks
        );
    }
}

/// A `PROP` whose object table streams and whose first value does not decode.
fn prop_with_a_bad_value() -> Vec<u8> {
    let mut bytes = write_prop(vec![
        BinObject::builder(AATROX, SKIN_CLASS)
            .property(COUNT_FIELD, values::U32::new(1))
            .build(),
    ]);
    let field = COUNT_FIELD.to_le_bytes();
    let at = bytes
        .windows(4)
        .position(|window| window == field)
        .expect("the field is in the body");
    bytes[at + 4] = 0xEE;
    bytes
}

#[test]
fn a_chunk_whose_values_do_not_decode_keeps_its_declarations_without_terms() {
    let install = Installation::new();
    install.write_archive(
        "A.wad.client",
        &[
            ("bad.bin", &prop_with_a_bad_value()),
            ("fine.bin", &patch()),
        ],
    );
    let game = GameIndex::build(install.game_dir()).unwrap();

    let terms = TermIndex::build(&game).unwrap();
    let objects = ObjectIndex::build(&game).unwrap();

    assert_eq!(terms.stats().failed_chunks, 1);
    assert_eq!(terms.objects().rows(), objects.rows());
    assert!(terms.objects().declares(BinHash(AATROX)));
    assert!(
        terms
            .hash_postings(TermKind::Object, u64::from(AATROX))
            .is_empty()
    );
    assert_eq!(
        terms
            .hash_postings(TermKind::Object, u64::from(PATCHED))
            .len(),
        1
    );
}

#[test]
fn a_chunk_that_does_not_read_is_skipped_and_not_counted_as_failed() {
    let install = Installation::new();
    let mut broken = patch();
    broken.truncate(12);
    install.write_archive(
        "A.wad.client",
        &[("broken.bin", &broken), ("fine.bin", &skins())],
    );
    let game = GameIndex::build(install.game_dir()).unwrap();

    let terms = TermIndex::build(&game).unwrap();

    assert_eq!(terms.objects().stats().skipped_chunks, 1);
    assert_eq!(terms.stats().failed_chunks, 0);
    assert!(terms.objects().declares(BinHash(AATROX)));
}

#[test]
fn a_page_and_the_declaration_lists_report_the_same_matches() {
    let (_install, _game, search) = search();
    for text in ["skin", "aatrox", "00000200"] {
        let query = Query::new(text);
        let matches = search.matches(&query);
        let page = search.page(&matches, &query);
        assert_eq!(page.hits.len(), matches.len(), "{text}");
        for hit in &page.hits {
            let listed: Vec<_> = matches.get(hit.id).unwrap().collect();
            assert_eq!(listed, hit.matches, "{text}");
        }
        let again = search.page(&matches, &query);
        assert_eq!(again, page, "{text}");
        assert_eq!(
            matches.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            matches.declarations().map(|(id, _)| id).collect::<Vec<_>>()
        );
    }
}

#[test]
fn the_term_cache_round_trips_and_refuses_a_foreign_version_and_a_stale_fingerprint() {
    let (install, game) = installation();
    let path = install.game_dir().join("state").join("terms.bin");
    let built = TermIndex::build(&game).unwrap();

    built.save(&path).unwrap();
    let loaded = TermIndex::load_for(&path, &game).unwrap();
    assert_eq!(loaded, built);

    let mut bytes = std::fs::read(&path).unwrap();
    assert_eq!(u32::from(bytes[0]), TERM_CACHE_FORMAT_VERSION);
    bytes[0] = 0x7f;
    let foreign = install.game_dir().join("state").join("foreign.bin");
    std::fs::write(&foreign, &bytes).unwrap();
    assert!(matches!(
        TermIndex::load_for(&foreign, &game),
        Err(CacheError::Version { found: 0x7f, .. })
    ));

    install.write_archive("C.wad.client", &[("data/c.bin", &patch())]);
    let changed = GameIndex::build(install.game_dir()).unwrap();
    assert!(matches!(
        TermIndex::load_for(&path, &changed),
        Err(CacheError::Stale { .. })
    ));
    let rebuilt = TermIndex::load_or_build_with(&changed, &path, &BuildOptions::default()).unwrap();
    assert_eq!(rebuilt.fingerprint(), changed.fingerprint());
    assert_eq!(
        TermIndex::load_for(&path, &changed).unwrap().fingerprint(),
        changed.fingerprint()
    );
}

#[test]
fn a_term_build_called_off_after_the_first_archive_stops() {
    let (_install, game) = installation();
    let polls = AtomicU32::new(0);
    let called_off = || polls.fetch_add(1, Ordering::Relaxed) >= 1;
    let mut options = BuildOptions::default();
    options.workers = NonZeroUsize::new(1);
    options.called_off = Some(&called_off);
    assert!(matches!(
        TermIndex::build_with(&game, &options),
        Err(ObjectBuildError::CalledOff)
    ));
}

#[test]
fn name_sheets_cut_each_table_and_keep_only_held_chunks() {
    let (_install, game) = installation();
    let tables = Tables::standard();
    let names = NameSheets::build(&tables, &game);

    assert_eq!(names.sheet(NameTable::Entries).len(), 3);
    assert_eq!(names.version(NameTable::Types), Some("types-1"));
    let chunks = names.sheet(NameTable::Chunks);
    assert_eq!(chunks.len(), 3);
    assert!(chunks.scan("absent").is_empty());
    assert!(chunks.scan("aatrox.dds").is_empty());
    assert_eq!(
        names.version(NameTable::Chunks),
        Some(format!("chunks-1+{}", game.fingerprint()).as_str())
    );
    assert!(!names.is_empty());
}

#[test]
fn a_changed_version_recuts_its_table_alone_and_a_none_version_recuts_every_load() {
    let (install, game) = installation();
    let path = install.game_dir().join("state").join("names.bin");
    let mut tables = Tables::standard();
    tables.versions.insert(NameTable::Hashes, None);

    let first = NameSheets::load_or_build(&path, &tables, &game);
    for table in NameTable::ALL {
        assert_eq!(tables.cuts(table), 1);
    }
    assert_eq!(first.sheet(NameTable::Hashes).len(), 1);
    let saved = NameSheets::load(&path).unwrap();
    assert!(saved.sheet(NameTable::Hashes).is_empty());
    assert_eq!(saved.version(NameTable::Hashes), None);
    assert_eq!(
        saved.sheet(NameTable::Entries),
        first.sheet(NameTable::Entries)
    );

    tables
        .versions
        .insert(NameTable::Types, Some("types-2".to_owned()));
    let second = NameSheets::load_or_build(&path, &tables, &game);
    assert_eq!(tables.cuts(NameTable::Entries), 1);
    assert_eq!(tables.cuts(NameTable::Types), 2);
    assert_eq!(tables.cuts(NameTable::Hashes), 2);
    assert_eq!(second.version(NameTable::Types), Some("types-2"));
    assert_eq!(second.sheet(NameTable::Hashes).len(), 1);
    assert_eq!(
        second.sheet(NameTable::Entries),
        first.sheet(NameTable::Entries)
    );

    let written = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(written - std::time::Duration::from_secs(60))
        .unwrap();
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let third = NameSheets::load_or_build(&path, &tables, &game);
    assert_eq!(tables.cuts(NameTable::Hashes), 3);
    assert_eq!(tables.cuts(NameTable::Types), 2);
    assert_eq!(third.sheet(NameTable::Hashes).len(), 1);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before,
        "a recut of an unversioned table alone writes nothing"
    );
}

#[test]
fn a_hash_query_hits_by_lookup_in_every_kind_and_by_text() {
    let (_install, _game, search) = search();
    for text in ["0x00000200", "00000200", " 0X00000200 "] {
        let matches = search.matches(&Query::new(text));
        let count = |kind: TermKind| matches.count(kind);
        assert_eq!(count(TermKind::Object), 3, "{text}");
        assert_eq!(count(TermKind::Link), 3, "{text}");
        let ahri = search
            .terms()
            .objects()
            .declarations(BinHash(AHRI))
            .next()
            .unwrap()
            .0;
        let of: Vec<_> = matches.get(ahri).unwrap().collect();
        assert_eq!(of[0].kind, TermKind::Object);
        assert_eq!(
            of[0].term,
            MatchedTerm::Hash {
                hash: u64::from(AHRI),
                name: Some("Characters/Ahri/Skins/Skin0".to_owned())
            }
        );
        assert!(of[0].ranges.is_empty());
        if !text.trim().to_ascii_lowercase().starts_with("0x") {
            assert_eq!(count(TermKind::String), 3, "{text}");
            assert_eq!(
                of.last().unwrap().term,
                MatchedTerm::String("see 00000200".to_owned())
            );
            assert_eq!(of.last().unwrap().ranges, [(4, 12)]);
        }
    }
}

#[test]
fn a_name_query_resolves_through_each_sheet_with_ranges() {
    let (_install, _game, search) = search();
    let matches = search.matches(&Query::new("AATROX"));
    let count = |kind: TermKind| matches.count(kind);
    assert_eq!(count(TermKind::Object), 3);
    assert_eq!(count(TermKind::Hash), 3);
    assert_eq!(count(TermKind::String), 3);
    assert_eq!(count(TermKind::Wad), 0);
    assert_eq!(matches.len(), 3);

    let (_, of) = matches.iter().next().unwrap();
    let of: Vec<_> = of.collect();
    assert_eq!(
        of.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [TermKind::Object, TermKind::Hash, TermKind::String]
    );
    assert_eq!(
        of[0].term,
        MatchedTerm::Hash {
            hash: u64::from(AATROX),
            name: Some("Characters/Aatrox/Skins/Skin0".to_owned())
        }
    );
    assert_eq!(of[0].ranges, [(11, 17)]);
    assert_eq!(of[1].ranges, [(5, 11)]);
    assert_eq!(of[2].ranges, [(18, 24), (36, 42)]);

    let classes = search.matches(&Query::new("vfxsystem"));
    assert_eq!(classes.count(TermKind::Embedded), 3);
    assert_eq!(classes.count(TermKind::Class), 0);
    let fields = search.matches(&Query::new("simpleskin"));
    assert_eq!(fields.count(TermKind::Field), 7);
}

#[test]
fn a_chunk_link_matches_through_the_scan_sheet_of_held_linked_chunks() {
    let (_install, _game, search) = search();
    let matches = search.matches(&Query::new("PATCH.bin"));
    assert_eq!(matches.count(TermKind::Wad), 3);
    let (_, of) = matches.iter().next().unwrap();
    let wad: Vec<_> = of.filter(|m| m.kind == TermKind::Wad).collect();
    assert_eq!(
        wad[0].term,
        MatchedTerm::Hash {
            hash: chunk_hash("data/patch.bin").0,
            name: Some("data/patch.bin".to_owned())
        }
    );
    assert_eq!(wad[0].ranges, [(5, 14)]);
    assert_eq!(matches.count(TermKind::Object), 0);
    assert_eq!(
        search
            .matches(&Query::new("skin2.bin"))
            .count(TermKind::Wad),
        0
    );
}

#[test]
fn a_prepared_search_answers_as_an_unprepared_one() {
    let (_install, _game, cold) = search();
    let (_install, _game, warm) = search();
    warm.prepare();
    for text in ["skin", "aatrox", "0x00000200", "patch"] {
        let query = Query::new(text);
        assert_eq!(warm.search(&query), cold.search(&query), "{text}");
    }
    let options = ListOptions::default();
    assert_eq!(
        warm.list(Dir::Root, &options),
        cold.list(Dir::Root, &options)
    );
}

#[test]
fn a_page_keeps_the_requested_kinds_and_counts_every_kind() {
    let (_install, _game, search) = search();
    let mut query = Query::new("patched");
    query.kinds = TermKinds::from(TermKind::Object);
    let page = search.search(&query);
    assert_eq!(page.total, 1);
    assert_eq!(
        page.counts
            .iter()
            .find(|count| count.kind == TermKind::String)
            .map(|count| count.count),
        Some(1)
    );
    assert_eq!(
        page.hits[0]
            .matches
            .iter()
            .map(|m| m.kind)
            .collect::<Vec<_>>(),
        [TermKind::Object, TermKind::String]
    );

    query.kinds = TermKinds::from(TermKind::Wad);
    assert_eq!(search.search(&query).total, 0);
    assert!(search.search(&Query::new("   ")).hits.is_empty());
}

#[test]
fn a_limit_truncates_and_an_offset_past_the_end_is_empty() {
    let (_install, _game, search) = search();
    let mut query = Query::new("skin");
    let all = search.search(&query);
    assert_eq!(all.total, 7);
    assert!(!all.truncated);

    query.limit = 2;
    let first = search.search(&query);
    assert_eq!(first.total, 7);
    assert_eq!(first.hits.len(), 2);
    assert!(first.truncated);

    query.offset = 6;
    let last = search.search(&query);
    assert_eq!(last.hits.len(), 1);
    assert!(!last.truncated);
    assert_eq!(last.hits[0], all.hits[6]);

    query.offset = 9;
    let past = search.search(&query);
    assert!(past.hits.is_empty());
    assert!(!past.truncated);
}

#[test]
fn hits_order_by_archive_then_natural_chunk_path_then_id() {
    let (_install, game, search) = search();
    let page = search.search(&Query::new("skin"));
    let order: Vec<(String, WadHash)> = page
        .hits
        .iter()
        .map(|hit| {
            (
                game.archive(hit.declaration.archive).file_name().to_owned(),
                hit.declaration.chunk,
            )
        })
        .collect();
    let a = |chunk: &str| ("A.wad.client".to_owned(), chunk_hash(chunk));
    assert_eq!(
        order,
        [
            a("data/skin2.bin"),
            a("data/skin2.bin"),
            a("data/skin10.bin"),
            a("data/skin10.bin"),
            a("data/unnamed"),
            a("data/unnamed"),
            ("B.wad.client".to_owned(), chunk_hash("data/patch.bin")),
        ]
    );
    let in_chunk: Vec<u32> = page.hits[..2].iter().map(|hit| hit.id.into()).collect();
    assert!(in_chunk[0] < in_chunk[1]);
}

#[test]
fn a_query_called_off_on_its_second_poll_keeps_the_first_sheet() {
    let (_install, _game, search) = search();
    let polls = AtomicU32::new(0);
    let called_off = || polls.fetch_add(1, Ordering::Relaxed) >= 1;
    let mut query = Query::new("skin");
    query.called_off = Some(&called_off);

    let matches = search.matches(&query);

    assert!(matches.superseded());
    assert_eq!(matches.count(TermKind::Object), 6);
    assert_eq!(matches.count(TermKind::Class), 0);
    assert!(search.search(&query).superseded);
}

/// One archive whose one bin declares the tree's objects.
fn tree() -> (Installation, GameSearch) {
    let paths = [
        "Characters/Aatrox",
        "Characters/Aatrox/Skins/Skin0",
        "Characters/Aatrox/Skins/Skin10",
        "Characters/Aatrox/Skins/Skin2",
        "Characters/Ahri/Skins/Skin0",
        "Maps/Shipping/Map11/Base",
        "Items/1001",
    ];
    let objects: Vec<BinObject> = (1..=paths.len() as u32)
        .chain([0x999])
        .map(|object| BinObject::new(object, SKIN_CLASS))
        .collect();
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("data/tree.bin", &write_prop(objects))]);
    let game = GameIndex::build(install.game_dir()).unwrap();
    let mut tables = Tables::default();
    let entries: Vec<(u64, &str)> = paths
        .iter()
        .enumerate()
        .map(|(at, path)| (at as u64 + 1, *path))
        .collect();
    tables.set(NameTable::Entries, &entries);
    let terms = TermIndex::build(&game).unwrap();
    let names = NameSheets::build(&tables, &game);
    (install, GameSearch::new(terms, names))
}

fn shape(entries: &[TreeEntry]) -> Vec<(String, Option<u32>, bool)> {
    entries
        .iter()
        .map(|entry| match entry {
            TreeEntry::Dir {
                name, objects, own, ..
            } => (name.clone(), Some(*objects), own.is_some()),
            TreeEntry::Object(node) => (node.name.clone(), None, true),
            TreeEntry::Unnamed { objects } => ("<unnamed>".to_owned(), Some(*objects), false),
            _ => unreachable!("a listing holds directories, objects and the unnamed directory"),
        })
        .collect()
}

#[test]
fn the_root_lists_directories_in_natural_order_with_the_unnamed_directory_last() {
    let (_install, search) = tree();
    let root = search.list(Dir::Root, &ListOptions::default()).unwrap();
    assert_eq!(
        shape(&root.entries),
        [
            ("Characters".to_owned(), Some(5), false),
            ("Items".to_owned(), Some(1), false),
            ("Maps".to_owned(), Some(1), false),
            ("<unnamed>".to_owned(), Some(1), false),
        ]
    );
    assert_eq!(root.total, 4);

    let unnamed = search.list(Dir::Unnamed, &ListOptions::default()).unwrap();
    assert_eq!(
        shape(&unnamed.entries),
        [("0x00000999".to_owned(), None, true)]
    );
}

#[test]
fn a_directory_that_is_also_an_object_carries_it_and_objects_sort_naturally() {
    let (_install, search) = tree();
    let characters = search
        .list(Dir::Path("characters/"), &ListOptions::default())
        .unwrap();
    assert_eq!(
        shape(&characters.entries),
        [
            ("Aatrox".to_owned(), Some(3), true),
            ("Ahri".to_owned(), Some(1), false),
        ]
    );
    let TreeEntry::Dir { path, own, .. } = &characters.entries[0] else {
        panic!("Aatrox is a directory");
    };
    assert_eq!(path, "Characters/Aatrox");
    let own = own.as_ref().unwrap();
    assert_eq!(own.path, "Characters/Aatrox");
    assert_eq!(own.declarations.len(), 1);

    let skins = search
        .list(
            Dir::Path("Characters/Aatrox/Skins"),
            &ListOptions::default(),
        )
        .unwrap();
    assert_eq!(
        shape(&skins.entries)
            .into_iter()
            .map(|(name, ..)| name)
            .collect::<Vec<_>>(),
        ["Skin0", "Skin2", "Skin10"]
    );
    assert!(
        search
            .list(Dir::Path("Characters/Nobody"), &ListOptions::default())
            .is_none()
    );
}

#[test]
fn folding_runs_through_single_directories_and_stops_at_an_object() {
    let (_install, search) = tree();
    let mut options = ListOptions::default();
    options.fold_single = true;

    let root = search.list(Dir::Root, &options).unwrap();
    let TreeEntry::Dir { path, name, .. } = &root.entries[2] else {
        panic!("Maps is a directory");
    };
    assert_eq!(name, "Maps/Shipping/Map11");
    assert_eq!(path, "Maps/Shipping/Map11");
    assert_eq!(
        search
            .list(Dir::Path(path), &options)
            .unwrap()
            .entries
            .len(),
        1
    );

    let characters = search.list(Dir::Path("Characters"), &options).unwrap();
    assert_eq!(
        shape(&characters.entries),
        [
            ("Aatrox".to_owned(), Some(3), true),
            ("Ahri/Skins".to_owned(), Some(1), false),
        ]
    );
}

#[test]
fn a_listing_over_its_limit_reports_the_total() {
    let (_install, search) = tree();
    let mut options = ListOptions::default();
    options.limit = 2;
    let root = search.list(Dir::Root, &options).unwrap();
    assert_eq!(root.entries.len(), 2);
    assert_eq!(root.total, 4);
    assert!(root.truncated);

    options.limit = 3;
    let root = search.list(Dir::Root, &options).unwrap();
    assert_eq!(
        shape(&root.entries)
            .into_iter()
            .map(|(name, ..)| name)
            .collect::<Vec<_>>(),
        ["Characters", "Items", "Maps"]
    );

    options.limit = 0;
    let unnamed = search.list(Dir::Unnamed, &options).unwrap();
    assert!(unnamed.entries.is_empty());
    assert_eq!(unnamed.total, 1);
    assert!(unnamed.truncated);
}

#[test]
fn under_yields_a_prefix_in_path_order_and_node_finds_a_path() {
    let (_install, search) = tree();
    let under: Vec<&str> = search
        .under("Characters/Aatrox")
        .map(|(path, _)| path)
        .collect();
    assert_eq!(
        under,
        [
            "characters/aatrox",
            "characters/aatrox/skins/skin0",
            "characters/aatrox/skins/skin10",
            "characters/aatrox/skins/skin2",
        ]
    );
    assert_eq!(search.under("").count(), 7);
    assert_eq!(search.under("Characters/Aat").count(), 0);

    let node = search.node("CHARACTERS/AHRI/skins/skin0").unwrap();
    assert_eq!(node.path, "Characters/Ahri/Skins/Skin0");
    assert_eq!(node.name, "Skin0");
    assert_eq!(node.object, BinHash(5));
    assert_eq!(node.declarations.len(), 1);
    assert!(search.node("Characters/Ahri").is_none());
}

#[test]
fn set_names_recuts_the_tree() {
    let (_install, mut search) = tree();
    assert_eq!(search.under("").count(), 7);
    search.set_names(NameSheets::default());
    assert_eq!(search.under("").count(), 0);
    let root = search.list(Dir::Root, &ListOptions::default()).unwrap();
    assert_eq!(
        shape(&root.entries),
        [("<unnamed>".to_owned(), Some(8), false)]
    );
}

/// CDTB-format tables, `<hex hash> <name>` per line, read from one directory.
struct CdtbTables {
    dir: camino::Utf8PathBuf,
}

impl CdtbTables {
    fn file(&self, table: NameTable) -> camino::Utf8PathBuf {
        let name = match table {
            NameTable::Entries => "binentries",
            NameTable::Types => "bintypes",
            NameTable::Fields => "binfields",
            NameTable::Hashes => "binhashes",
            _ => "game",
        };
        self.dir.join(format!("hashes.{name}.txt"))
    }
}

impl NameSource for CdtbTables {
    fn version(&self, table: NameTable) -> Option<String> {
        let modified = std::fs::metadata(self.file(table)).ok()?.modified().ok()?;
        Some(format!("{modified:?}"))
    }

    fn for_each_name(&self, table: NameTable, visit: &mut dyn FnMut(u64, &str)) {
        let Ok(text) = std::fs::read_to_string(self.file(table)) else {
            return;
        };
        for line in text.lines() {
            if let Some((hash, name)) = line.split_once(' ')
                && let Ok(hash) = u64::from_str_radix(hash, 16)
            {
                visit(hash, name);
            }
        }
    }
}

/// Builds every layer over the installation at `LTK_GAME_DIR`, with the CDTB tables at
/// `LTK_HASHTABLES_DIR` when set, and prints the figures of the spec's measurement appendix.
#[test]
#[ignore = "reads a real installation named by LTK_GAME_DIR"]
fn measure_a_real_installation() {
    use std::time::Instant;

    let game_dir = camino::Utf8PathBuf::from(std::env::var("LTK_GAME_DIR").unwrap());
    let state = tempfile::tempdir().unwrap();
    let state = camino::Utf8Path::from_path(state.path())
        .unwrap()
        .to_owned();
    let tables = std::env::var("LTK_HASHTABLES_DIR")
        .ok()
        .map(|dir| CdtbTables { dir: dir.into() });

    let started = Instant::now();
    let game = GameIndex::build(&game_dir).unwrap();
    println!(
        "chunk index: {} chunks in {:?}",
        game.len(),
        started.elapsed()
    );

    let started = Instant::now();
    let names = match &tables {
        Some(tables) => NameSheets::build(tables, &game),
        None => NameSheets::default(),
    };
    println!(
        "name sheets: {:?} entries in {:?}",
        NameTable::ALL.map(|table| names.sheet(table).len()),
        started.elapsed()
    );

    let chunks: HashMap<u64, String> = match &tables {
        Some(tables) => {
            let mut chunks = HashMap::new();
            tables.for_each_name(NameTable::Chunks, &mut |hash, name| {
                chunks.insert(hash, name.to_owned());
            });
            chunks
        }
        None => HashMap::new(),
    };
    struct Chunks(HashMap<u64, String>);
    impl ResolveWadPath for Chunks {
        fn for_each_named(&self, hashes: &[WadHash], visit: &mut dyn FnMut(usize, &str)) {
            for (index, hash) in hashes.iter().enumerate() {
                if let Some(name) = self.0.get(&hash.0) {
                    visit(index, name);
                }
            }
        }
    }
    let resolver = Chunks(chunks);
    let mut options = BuildOptions::default();
    options.resolver = (!resolver.0.is_empty()).then_some(&resolver as &dyn ResolveWadPath);

    let started = Instant::now();
    let terms = TermIndex::build_with(&game, &options).unwrap();
    let walk = started.elapsed();
    let objects = terms.objects().stats();
    println!(
        "term walk: {walk:?}, {} archives, {} bins, {} skipped, {} declarations, {} objects, \
         {} bytes, {:?}, workers {}",
        objects.archives,
        objects.bins,
        objects.skipped_chunks,
        objects.declarations,
        terms.objects().len(),
        objects.bytes,
        terms.stats(),
        objects.workers,
    );

    let started = Instant::now();
    let declarations = ObjectIndex::build_with(&game, &options).unwrap();
    println!("object build: {:?}", started.elapsed());
    assert_eq!(declarations.rows(), terms.objects().rows());

    let path = state.join("terms.bin");
    let started = Instant::now();
    terms.save(&path).unwrap();
    let saved = started.elapsed();
    let started = Instant::now();
    let loaded = TermIndex::load_for(&path, &game).unwrap();
    println!(
        "term cache: {} bytes, save {saved:?}, load {:?}",
        std::fs::metadata(&path).unwrap().len(),
        started.elapsed()
    );
    assert_eq!(loaded, terms);

    let search = GameSearch::new(terms, names);
    let started = Instant::now();
    search.prepare();
    println!("prepare: {:?}", started.elapsed());
    for (text, kinds) in [
        ("aatrox", TermKinds::ALL),
        ("aatrox", TermKinds::from(TermKind::Object)),
        ("mname", TermKinds::from(TermKind::Field)),
        ("0x4f2e7d11", TermKinds::ALL),
        ("a", TermKinds::ALL),
    ] {
        let mut query = Query::new(text);
        query.kinds = kinds;
        let started = Instant::now();
        let page = search.search(&query);
        println!(
            "{text} {kinds:?}: {} declarations in {:?}",
            page.total,
            started.elapsed()
        );
    }
    let started = Instant::now();
    let root = search.list(Dir::Root, &ListOptions::default()).unwrap();
    println!(
        "root listing: {} entries in {:?}",
        root.total,
        started.elapsed()
    );
    let started = Instant::now();
    assert!(
        search
            .list(Dir::Path("Characters/Aatrox"), &ListOptions::default())
            .is_some()
    );
    println!("Characters/Aatrox listing: {:?}", started.elapsed());
}
