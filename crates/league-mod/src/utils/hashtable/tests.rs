use super::*;
use std::io::Write;

fn resolved(hashtable: &WadHashtable, hash: u64) -> Option<String> {
    hashtable.resolve(WadHash(hash))
}

#[test]
fn resolves_a_known_hash_and_answers_none_for_the_rest() {
    let mut hashtable = WadHashtable::default();
    hashtable
        .add_from_reader(&b"0123456789abcdef assets/characters/aatrox/skin0.bin\n"[..])
        .unwrap();

    assert_eq!(
        resolved(&hashtable, 0x0123456789abcdef).as_deref(),
        Some("assets/characters/aatrox/skin0.bin")
    );
    assert!(hashtable.is_known(WadHash(0x0123456789abcdef)));

    assert_eq!(resolved(&hashtable, 0x1), None);
    assert!(!hashtable.is_known(WadHash(0x1)));
}

/// Paths with spaces survive: the line is split on the first space only.
#[test]
fn keeps_spaces_in_paths() {
    let mut hashtable = WadHashtable::default();
    hashtable
        .add_from_reader(&b"00000000000000ff some path/with spaces.bin"[..])
        .unwrap();

    assert_eq!(
        resolved(&hashtable, 0xff).as_deref(),
        Some("some path/with spaces.bin")
    );
}

#[test]
fn skips_malformed_lines() {
    let mut hashtable = WadHashtable::default();
    hashtable
        .add_from_reader(&b"\nnot-a-hash some/path\n00000000000000ff kept.bin\nff\n"[..])
        .unwrap();

    assert_eq!(resolved(&hashtable, 0xff).as_deref(), Some("kept.bin"));
    assert_eq!(resolved(&hashtable, 0x1), None);
}

#[test]
fn missing_file_names_the_path() {
    let tmp = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(tmp.path().join("absent.txt")).unwrap();

    match WadHashtable::default().add_from_file(&path) {
        Err(WadHashtableError::Read { path: failed, .. }) => assert_eq!(failed, path),
        other => panic!("expected Read, got {other:?}"),
    }
}

/// A hashtable directory the user has not created is not a failure.
#[test]
fn missing_directory_loads_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    let hashtable = WadHashtable::from_directory(root.join("does-not-exist")).unwrap();

    assert_eq!(resolved(&hashtable, 0xff), None);
}

#[test]
fn loads_every_file_in_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    std::fs::create_dir_all(root.join("nested")).unwrap();
    let mut first = File::create(root.join("one.txt")).unwrap();
    first.write_all(b"0000000000000001 one.bin\n").unwrap();
    let mut second = File::create(root.join("nested/two.txt")).unwrap();
    second.write_all(b"0000000000000002 two.bin\n").unwrap();

    let hashtable = WadHashtable::from_directory(&root).unwrap();

    assert_eq!(resolved(&hashtable, 0x1).as_deref(), Some("one.bin"));
    assert_eq!(resolved(&hashtable, 0x2).as_deref(), Some("two.bin"));
}
