use crate::Hashtable;

// Grammar: the table file is name-only, LF, forward-slash lines. A table
// round trips its names, their display casing and their order.

#[test]
fn table_file_round_trips_names_casing_and_order() {
    let src = "ASSETS/Characters/Aurora/Skins/Skin0/Aurora_Custom.TEX\n\
               assets/maps/kit/zzz.dds\n\
               DATA/Characters/Aurora/aurora.bin\n";

    let table = Hashtable::from_reader(src.as_bytes()).unwrap();
    let mut out = Vec::new();
    table.write_to(&mut out).unwrap();

    assert_eq!(String::from_utf8(out).unwrap(), src);
}

#[test]
fn crlf_and_blank_lines_read_as_the_same_names() {
    let unix = "a/b.dds\nc/d.bin\n";
    let windows = "a/b.dds\r\n\r\nc/d.bin\r\n\n";

    let from_unix = Hashtable::from_reader(unix.as_bytes()).unwrap();
    let from_windows = Hashtable::from_reader(windows.as_bytes()).unwrap();

    assert_eq!(from_unix, from_windows);
}

#[test]
fn a_bom_is_refused() {
    let src = b"\xEF\xBB\xBFa/b.dds\n";
    Hashtable::from_reader(&src[..]).unwrap_err();
}

#[test]
fn a_backslash_and_a_non_printable_byte_are_refused() {
    Hashtable::from_reader(&b"a\\b.dds\n"[..]).unwrap_err();
    Hashtable::from_reader(&b"a/\x07b.dds\n"[..]).unwrap_err();
    Hashtable::from_reader("a/\u{00e9}.dds\n".as_bytes()).unwrap_err();
}

// Keys: canonicalize, hash, truncate and render in one motion.

use crate::{Algorithm, Key, KeyWidth};

#[test]
fn xxh64_matches_the_known_vector() {
    // xxh64("abc", seed 0) - reference vector from the xxHash test suite.
    let width = KeyWidth::new(64).unwrap();
    let key = Key::of("abc", &Algorithm::Xxh64, width).unwrap();
    assert_eq!(key.to_string(), "44bc2cf5ad770999");
}

#[test]
fn fnv1a32_matches_the_known_vector() {
    // fnv1a_32("abc") - reference vector from the FNV test suite.
    let width = KeyWidth::new(32).unwrap();
    let key = Key::of("abc", &Algorithm::Fnv1a32, width).unwrap();
    assert_eq!(key.to_string(), "1a47e90b");
}

#[test]
fn two_spellings_of_one_name_share_one_key() {
    let width = KeyWidth::new(64).unwrap();
    let upper = Key::of("ASSETS/Foo/Bar.TEX", &Algorithm::Xxh64, width).unwrap();
    let lower = Key::of("assets/foo/bar.tex", &Algorithm::Xxh64, width).unwrap();
    assert_eq!(upper, lower);
}

#[test]
fn a_key_is_the_hash_truncated_to_the_declared_width() {
    // xxh64("abc") = 0x44bc2cf5ad770999; the low 16 bits are 0x0999.
    let width = KeyWidth::new(16).unwrap();
    let key = Key::of("abc", &Algorithm::Xxh64, width).unwrap();
    assert_eq!(key.to_string(), "0999");
}

#[test]
fn hex_zero_pads_a_width_that_is_not_a_multiple_of_four() {
    // The low 10 bits of 0x...0999 are 0x199, rendered at ceil(10/4) digits.
    let width = KeyWidth::new(10).unwrap();
    let key = Key::of("abc", &Algorithm::Xxh64, width).unwrap();
    assert_eq!(key.to_string(), "199");
}

// Registry: Category and Algorithm are open registries whose wire form is a
// bare lowercase string, shared by all three containers.

use crate::Category;

#[test]
fn categories_and_algorithms_round_trip_their_wire_spelling() {
    for (value, wire) in [
        (Category::Game, "\"game\""),
        (Category::BinEntries, "\"binentries\""),
        (Category::BinHashes, "\"binhashes\""),
        (
            Category::Unknown("stringtable".to_owned()),
            "\"stringtable\"",
        ),
    ] {
        assert_eq!(serde_json::to_string(&value).unwrap(), wire);
        assert_eq!(serde_json::from_str::<Category>(wire).unwrap(), value);
    }

    for (value, wire) in [
        (Algorithm::Xxh64, "\"xxh64\""),
        (Algorithm::Fnv1a32, "\"fnv1a_32\""),
        (Algorithm::Unknown("xxh3".to_owned()), "\"xxh3\""),
    ] {
        assert_eq!(serde_json::to_string(&value).unwrap(), wire);
        assert_eq!(serde_json::from_str::<Algorithm>(wire).unwrap(), value);
    }
}

#[test]
fn an_unknown_algorithm_computes_no_key() {
    let width = KeyWidth::new(64).unwrap();
    let algorithm = Algorithm::Unknown("xxh3".to_owned());
    assert_eq!(Key::of("abc", &algorithm, width), None);
}

// Merging: tables in manifest order, lines in file order, first key wins.

use crate::{HashtableEntry, HashtableSet};

fn game_entry(path: &str) -> HashtableEntry {
    HashtableEntry::new(
        path,
        Category::Game,
        Algorithm::Xxh64,
        KeyWidth::new(64).unwrap(),
    )
}

fn game_key(name: &str) -> Key {
    Key::of(name, &Algorithm::Xxh64, KeyWidth::new(64).unwrap()).unwrap()
}

#[test]
fn merging_keeps_the_first_key_in_manifest_then_file_order() {
    let first = Hashtable::from_reader(&b"ASSETS/One.tex\n"[..]).unwrap();
    let second = Hashtable::from_reader(&b"assets/one.tex\nassets/two.tex\n"[..]).unwrap();

    let set = HashtableSet::build([
        (game_entry("META/hashes/game.hashes.txt"), first),
        (game_entry("META/hashes/game.imported.hashes.txt"), second),
    ]);

    // The duplicate keeps the first occurrence - display casing included.
    assert_eq!(
        set.resolve(&Category::Game, game_key("assets/one.tex")),
        Some("ASSETS/One.tex")
    );
    assert_eq!(
        set.resolve(&Category::Game, game_key("assets/two.tex")),
        Some("assets/two.tex")
    );
    assert_eq!(
        set.resolve(&Category::Game, game_key("assets/absent.tex")),
        None
    );
}

#[test]
fn a_collision_is_detected_across_two_files_of_one_category() {
    // fnv1a_32("data/strings/name_1") = 0x38b3564a and
    // fnv1a_32("data/strings/name_50") = 0xc446a14a collide on the low 8 bits.
    let width = KeyWidth::new(8).unwrap();
    let entry =
        |path: &str| HashtableEntry::new(path, Category::BinHashes, Algorithm::Fnv1a32, width);
    let first = Hashtable::from_reader(&b"data/strings/name_1\n"[..]).unwrap();
    let second = Hashtable::from_reader(&b"data/strings/name_50\n"[..]).unwrap();

    let set = HashtableSet::build([
        (entry("hashes/binhashes.hashes.txt"), first),
        (entry("hashes/binhashes.imported.hashes.txt"), second),
    ]);

    let collisions = set.collisions();
    assert_eq!(collisions.len(), 1);
    assert_eq!(collisions[0].category, Category::BinHashes);
    assert_eq!(collisions[0].first, "data/strings/name_1");
    assert_eq!(collisions[0].second, "data/strings/name_50");
    assert_eq!(collisions[0].key.to_string(), "4a");

    // A reader that runs into one anyway keeps the first occurrence.
    let key = Key::of("data/strings/name_1", &Algorithm::Fnv1a32, width).unwrap();
    assert_eq!(
        set.resolve(&Category::BinHashes, key),
        Some("data/strings/name_1")
    );
}

#[test]
fn a_duplicate_is_not_a_collision() {
    let first = Hashtable::from_reader(&b"ASSETS/One.tex\nassets/one.tex\n"[..]).unwrap();
    let set = HashtableSet::build([(game_entry("hashes/game.hashes.txt"), first)]);
    assert!(set.collisions().is_empty());
}

#[test]
fn a_table_with_an_unknown_algorithm_is_skipped_for_lookup() {
    let entry = HashtableEntry::new(
        "hashes/game.hashes.txt",
        Category::Game,
        Algorithm::Unknown("xxh3".to_owned()),
        KeyWidth::new(64).unwrap(),
    );
    let table = Hashtable::from_reader(&b"assets/one.tex\n"[..]).unwrap();

    let set = HashtableSet::build([(entry, table)]);

    // The same name keyed by a known algorithm resolves to nothing: the
    // unknown table contributed no keys.
    assert_eq!(
        set.resolve(&Category::Game, game_key("assets/one.tex")),
        None
    );
    assert!(set.collisions().is_empty());
}

// Raw hash values: the interop direction, for hashes other crates hold.

#[test]
fn from_value_masks_to_the_width_and_matches_of() {
    let width = KeyWidth::new(8).unwrap();
    let key = Key::of("data/strings/name_1", &Algorithm::Fnv1a32, width).unwrap();

    // The full 32-bit hash truncates to the same key.
    assert_eq!(Key::from_value(0x38b3_564a, width), key);
    // An already-truncated value is left as it is.
    assert_eq!(Key::from_value(0x4a, width), key);
}

#[test]
fn a_raw_hash_value_resolves_through_every_declared_width() {
    let narrow = HashtableEntry::new(
        "hashes/game.narrow.hashes.txt",
        Category::Game,
        Algorithm::Xxh64,
        KeyWidth::new(32).unwrap(),
    );
    let set = HashtableSet::build([
        (
            game_entry("hashes/game.hashes.txt"),
            Hashtable::from_reader(&b"assets/one.tex\n"[..]).unwrap(),
        ),
        (
            narrow,
            Hashtable::from_reader(&b"assets/two.tex\n"[..]).unwrap(),
        ),
    ]);

    let full_hash = |name: &str| game_key(name).value();
    assert_eq!(
        set.resolve_value(&Category::Game, full_hash("assets/one.tex")),
        Some("assets/one.tex")
    );
    // The name in the 32-bit table resolves from the full 64-bit hash: the
    // value is truncated to each declared width until one answers.
    assert_eq!(
        set.resolve_value(&Category::Game, full_hash("assets/two.tex")),
        Some("assets/two.tex")
    );
    assert_eq!(
        set.resolve_value(&Category::Game, full_hash("assets/absent.tex")),
        None
    );
}

// Producer side: building and sorting tables.

#[test]
fn a_table_builds_from_names_and_refuses_an_invalid_one() {
    let table = Hashtable::from_names(["b/B.tex", "a/a.tex"]).unwrap();
    assert_eq!(table.names().collect::<Vec<_>>(), ["b/B.tex", "a/a.tex"]);

    Hashtable::from_names(["ok.tex", "bad\u{7}.tex"]).unwrap_err();
}

#[test]
fn sorting_is_byte_order_so_uppercase_sorts_before_lowercase() {
    let mut table = Hashtable::from_names(["b/b.tex", "B/a.tex", "a/a.tex"]).unwrap();
    table.sort();
    assert_eq!(
        table.names().collect::<Vec<_>>(),
        ["B/a.tex", "a/a.tex", "b/b.tex"]
    );
}

#[test]
fn a_known_category_knows_its_standard_shape() {
    let (algorithm, width) = Category::Game.default_shape().unwrap();
    assert_eq!(algorithm, Algorithm::Xxh64);
    assert_eq!(width.bits(), 64);

    let (algorithm, width) = Category::BinHashes.default_shape().unwrap();
    assert_eq!(algorithm, Algorithm::Fnv1a32);
    assert_eq!(width.bits(), 32);

    assert_eq!(
        Category::BinEntries.default_shape().unwrap().0,
        Algorithm::Fnv1a32
    );
    assert_eq!(
        Category::Unknown("stringtable".to_owned()).default_shape(),
        None
    );
}

#[test]
fn a_key_exposes_its_truncated_value() {
    let width = KeyWidth::new(16).unwrap();
    let key = Key::of("abc", &Algorithm::Xxh64, width).unwrap();
    assert_eq!(key.value(), 0x0999);
}

#[test]
fn a_colliding_pair_is_reported_once_however_often_it_recurs() {
    let width = KeyWidth::new(8).unwrap();
    let entry =
        |path: &str| HashtableEntry::new(path, Category::BinHashes, Algorithm::Fnv1a32, width);
    let first = Hashtable::from_reader(&b"data/strings/name_1\n"[..]).unwrap();
    let second = Hashtable::from_reader(
        &b"data/strings/name_50\nDATA/STRINGS/NAME_50\ndata/strings/name_50\n"[..],
    )
    .unwrap();

    let set = HashtableSet::build([
        (entry("hashes/a.hashes.txt"), first),
        (entry("hashes/b.hashes.txt"), second),
    ]);

    assert_eq!(set.collisions().len(), 1);
}
