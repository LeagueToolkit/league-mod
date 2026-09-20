//! The fingerprint and the cache: stability, round trip, rejection, atomic save.

mod common;

use std::io::Write as _;

use camino::Utf8PathBuf;
use common::Installation;
use ltk_game_index::{CACHE_FORMAT_VERSION, CacheError, GameIndex};

fn cache_path(install: &Installation) -> Utf8PathBuf {
    install.game_dir().join("state").join("game_index.bin")
}

/// Moves the modification time of `path` one full second forward.
fn touch(path: &camino::Utf8Path) {
    let file = std::fs::File::options()
        .write(true)
        .open(path.as_std_path())
        .unwrap();
    let modified = file.metadata().unwrap().modified().unwrap();
    file.set_modified(modified + std::time::Duration::from_secs(1))
        .unwrap();
}

#[test]
fn a_rebuild_of_an_untouched_tree_has_the_same_fingerprint() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("a", b"1")]);
    install.write_garbage("B.wad.client", b"broken");

    let first = GameIndex::build(install.game_dir()).unwrap();
    let second = GameIndex::build(install.game_dir()).unwrap();
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(
        GameIndex::fingerprint_of(install.game_dir()).unwrap(),
        first.fingerprint()
    );
    assert_eq!(first, second);
    assert_eq!(format!("{}", first.fingerprint()).len(), 16);
    assert_eq!(
        format!("{:x}", first.fingerprint()),
        format!("{:x}", first.fingerprint().as_u64())
    );
}

#[test]
fn touching_an_archive_changes_the_fingerprint_even_a_skipped_one() {
    let install = Installation::new();
    let a = install.write_archive("A.wad.client", &[("a", b"1")]);
    let b = install.write_garbage("B.wad.client", b"broken");

    let before = GameIndex::fingerprint_of(install.game_dir()).unwrap();
    touch(&a);
    let after_a = GameIndex::fingerprint_of(install.game_dir()).unwrap();
    assert_ne!(before, after_a);
    touch(&b);
    let after_b = GameIndex::fingerprint_of(install.game_dir()).unwrap();
    assert_ne!(after_a, after_b);
}

#[test]
fn save_then_load_yields_an_equal_index() {
    let install = Installation::new();
    install.write_archive("Champions/A.wad.client", &[("shared", b"1"), ("a", b"2")]);
    install.write_archive("B.wad.client", &[("shared", b"1")]);
    install.write_garbage("C.wad.client", b"broken");
    let path = cache_path(&install);

    let built = GameIndex::build(install.game_dir()).unwrap();
    built.save(&path).unwrap();

    let loaded = GameIndex::load(&path).unwrap();
    assert_eq!(loaded, built);
    assert_eq!(loaded.skipped(), built.skipped());
    assert_eq!(
        loaded.archive_by_file_name("a.wad.client").unwrap(),
        built.archive_by_file_name("a.wad.client").unwrap()
    );

    let checked = GameIndex::load_for(&path, install.game_dir()).unwrap();
    assert_eq!(checked, built);
}

#[test]
fn a_cache_with_a_foreign_format_version_is_rejected() {
    let install = Installation::new();
    let path = cache_path(&install);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = std::fs::File::create(path.as_std_path()).unwrap();
    rmp_serde::encode::write(&mut file, &(CACHE_FORMAT_VERSION + 1)).unwrap();
    file.write_all(b"whatever follows").unwrap();

    let error = GameIndex::load(&path).unwrap_err();
    assert!(matches!(
        error,
        CacheError::Version { found, expected, .. }
            if found == CACHE_FORMAT_VERSION + 1 && expected == CACHE_FORMAT_VERSION
    ));
}

#[test]
fn a_cache_that_is_not_an_index_is_a_decode_error_and_a_missing_one_a_read_error() {
    let install = Installation::new();
    let path = cache_path(&install);

    let error = GameIndex::load(&path).unwrap_err();
    assert!(error.is_missing_file());
    assert!(matches!(error, CacheError::Read { .. }));

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path.as_std_path(), b"\xc1\xc1\xc1").unwrap();
    let error = GameIndex::load(&path).unwrap_err();
    assert!(matches!(error, CacheError::Decode { .. }));
    assert!(!error.is_missing_file());
}

#[test]
fn load_for_against_a_changed_tree_is_stale_with_both_fingerprints() {
    let install = Installation::new();
    let a = install.write_archive("A.wad.client", &[("a", b"1")]);
    let path = cache_path(&install);

    let built = GameIndex::build(install.game_dir()).unwrap();
    built.save(&path).unwrap();
    touch(&a);
    let current = GameIndex::fingerprint_of(install.game_dir()).unwrap();

    let error = GameIndex::load_for(&path, install.game_dir()).unwrap_err();
    let CacheError::Stale {
        cached,
        current: reported,
        ..
    } = error
    else {
        panic!("expected a stale cache, got {error:?}");
    };
    assert_eq!(cached, built.fingerprint());
    assert_eq!(reported, current);
    assert_ne!(cached, reported);
}

#[test]
fn a_failed_save_leaves_no_partial_file() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("a", b"1")]);
    let built = GameIndex::build(install.game_dir()).unwrap();

    // The parent of the cache path is a regular file, so nothing under it can be created.
    let blocker = install.game_dir().join("state");
    std::fs::write(blocker.as_std_path(), b"in the way").unwrap();
    let path = blocker.join("game_index.bin");

    let error = built.save(&path).unwrap_err();
    assert!(matches!(error, CacheError::Write { .. }));
    assert!(!path.exists());
    assert_eq!(std::fs::read(blocker.as_std_path()).unwrap(), b"in the way");
}

#[test]
fn save_leaves_no_temporary_sibling_behind() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("a", b"1")]);
    let path = cache_path(&install);

    GameIndex::build(install.game_dir())
        .unwrap()
        .save(&path)
        .unwrap();

    let siblings: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(siblings, ["game_index.bin"]);
}

#[test]
fn load_or_build_returns_a_built_index_for_an_unreadable_cache_and_saves_it() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("a", b"1")]);
    let path = cache_path(&install);

    let first = GameIndex::load_or_build(install.game_dir(), &path).unwrap();
    assert_eq!(first.len(), 1);
    assert!(path.exists());

    std::fs::write(path.as_std_path(), b"not a cache").unwrap();
    let second = GameIndex::load_or_build(install.game_dir(), &path).unwrap();
    assert_eq!(second, first);
    assert_eq!(GameIndex::load(&path).unwrap(), first);

    let third = GameIndex::load_or_build(install.game_dir(), &path).unwrap();
    assert_eq!(third, first);
}

#[test]
fn load_or_build_rebuilds_a_stale_cache() {
    let install = Installation::new();
    let a = install.write_archive("A.wad.client", &[("a", b"1")]);
    let path = cache_path(&install);

    let first = GameIndex::load_or_build(install.game_dir(), &path).unwrap();
    common::write_archive(&a, &[("a", b"1"), ("b", b"2")]);
    touch(&a);

    let second = GameIndex::load_or_build(install.game_dir(), &path).unwrap();
    assert_ne!(second.fingerprint(), first.fingerprint());
    assert_eq!(second.len(), 2);
    assert_eq!(GameIndex::load(&path).unwrap(), second);
}

#[test]
fn load_or_build_without_data_final_fails_with_the_build_error() {
    let root = tempfile::tempdir().unwrap();
    let game_dir = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let error = GameIndex::load_or_build(&game_dir, &game_dir.join("cache.bin")).unwrap_err();
    assert!(matches!(
        error,
        ltk_game_index::BuildError::MissingDataFinal { .. }
    ));
}
