//! The chunk index: archives, rows, lookups and the build.

mod common;

use camino::Utf8PathBuf;
use common::Installation;
use ltk_game_index::{
    ArchiveLookupError, ArchiveReadError, BuildError, GameIndex, WadHash, chunk_hash,
};
use ltk_hash::Hash as _;

#[test]
fn archives_sort_by_name_in_byte_order_and_ids_are_dense() {
    let install = Installation::new();
    // A case-only twin shares one file on a case-insensitive filesystem, so the twin
    // lives in another directory. Byte order puts `Zed` before `aatrox`.
    install.write_archive("Maps/Map11.wad.client", &[("a", b"1")]);
    install.write_archive("Champions/Aatrox.wad.client", &[("b", b"2")]);
    install.write_archive("Legacy/aatrox.wad.client", &[("c", b"3")]);
    install.write_archive("Champions/Zed.wad.client", &[("d", b"4")]);
    install.write_archive("Global.wad.client", &[("e", b"5")]);

    let index = GameIndex::build(install.game_dir()).unwrap();

    let names: Vec<&str> = index.archives().iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Champions/Aatrox.wad.client",
            "Champions/Zed.wad.client",
            "Global.wad.client",
            "Legacy/aatrox.wad.client",
            "Maps/Map11.wad.client",
        ]
    );
    for (position, archive) in index.archives().iter().enumerate() {
        let id = index.archive_by_file_name(archive.file_name());
        if archive
            .file_name()
            .eq_ignore_ascii_case("aatrox.wad.client")
        {
            assert!(matches!(id, Err(ArchiveLookupError::Ambiguous { .. })));
        } else {
            assert_eq!(id.unwrap().index(), position);
        }
        assert_eq!(archive.path, install.archive_path(&archive.name));
    }
    let global = index.archive_by_file_name("Global.wad.client").unwrap();
    assert_eq!(global.index(), 2);
    assert_eq!(index.archive(global).name, "Global.wad.client");
}

#[test]
fn a_chunk_in_two_archives_has_two_holders_in_id_order() {
    let install = Installation::new();
    install.write_archive("B.wad.client", &[("shared.bin", b"same"), ("only_b", b"b")]);
    install.write_archive("A.wad.client", &[("shared.bin", b"same"), ("only_a", b"a")]);

    let index = GameIndex::build(install.game_dir()).unwrap();
    let a = index.archive_by_file_name("A.wad.client").unwrap();
    let b = index.archive_by_file_name("B.wad.client").unwrap();

    let row = index.row_by_path("shared.bin").unwrap();
    assert_eq!(row.holders().collect::<Vec<_>>(), [a, b]);
    assert_eq!(row.first_holder(), a);
    assert_eq!(row.size(), 4);
    assert!(row.is_consistent());
    assert_eq!(row.copies()[0].checksum, row.copies()[1].checksum);

    assert_eq!(index.holders(chunk_hash("only_b")).collect::<Vec<_>>(), [b]);
    assert_eq!(index.len(), 3);
    assert!(!index.is_empty());
    assert!(index.contains(chunk_hash("only_a")));
    assert!(!index.contains(chunk_hash("absent")));
    assert_eq!(index.holders(chunk_hash("absent")).count(), 0);
    assert!(index.row(chunk_hash("absent")).is_none());
}

#[test]
fn copies_with_different_bytes_are_inconsistent() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("shared.bin", b"one")]);
    install.write_archive("B.wad.client", &[("shared.bin", b"two")]);

    let index = GameIndex::build(install.game_dir()).unwrap();
    let row = index.row_by_path("shared.bin").unwrap();
    assert_eq!(row.copies().len(), 2);
    assert!(!row.is_consistent());
}

#[test]
fn chunks_iterate_in_ascending_hash_order() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("x", b"1"), ("y", b"2"), ("z", b"3")]);

    let index = GameIndex::build(install.game_dir()).unwrap();
    let hashes: Vec<WadHash> = index.chunks().map(|(hash, _)| hash).collect();
    let mut sorted = hashes.clone();
    sorted.sort();
    assert_eq!(hashes, sorted);
    assert_eq!(index.chunks().len(), 3);
}

#[test]
fn file_name_lookup_is_case_insensitive_and_reports_absent_and_ambiguous() {
    let install = Installation::new();
    install.write_archive("Champions/Aatrox.wad.client", &[("a", b"1")]);
    install.write_archive("Maps/Shipping/Map11.wad.client", &[("b", b"2")]);
    install.write_archive("Maps/Map11.wad.client", &[("c", b"3")]);

    let index = GameIndex::build(install.game_dir()).unwrap();

    let aatrox = index.archive_by_file_name("aatrox.WAD.client").unwrap();
    assert_eq!(index.archive(aatrox).name, "Champions/Aatrox.wad.client");
    assert_eq!(index.archive(aatrox).file_name(), "Aatrox.wad.client");

    let map11 = index.archive_by_file_name("Map11.wad.client");
    let Err(ArchiveLookupError::Ambiguous {
        file_name,
        candidates,
    }) = map11
    else {
        panic!("expected an ambiguous lookup, got {map11:?}");
    };
    assert_eq!(file_name, "Map11.wad.client");
    let names: Vec<&str> = candidates
        .iter()
        .map(|&id| index.archive(id).name.as_str())
        .collect();
    assert_eq!(
        names,
        ["Maps/Map11.wad.client", "Maps/Shipping/Map11.wad.client"]
    );
    assert!(candidates.windows(2).all(|pair| pair[0] < pair[1]));

    assert_eq!(
        index.archive_by_file_name("Nothing.wad.client"),
        Err(ArchiveLookupError::Absent {
            file_name: "Nothing.wad.client".to_owned()
        })
    );
}

#[test]
fn only_wad_client_files_are_archives() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("a", b"1")]);
    install.write_archive("B.WAD.CLIENT", &[("b", b"2")]);
    install.write_garbage("A.wad.SubChunkTOC", b"toc");
    install.write_garbage("notes.txt", b"text");

    let index = GameIndex::build(install.game_dir()).unwrap();
    let names: Vec<&str> = index.archives().iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["A.wad.client", "B.WAD.CLIENT"]);
    assert!(index.skipped().is_empty());
}

#[test]
fn a_truncated_archive_is_skipped_and_keeps_its_id() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("a", b"1")]);
    install.write_garbage("B.wad.client", b"RW\x03\x04 truncated");
    install.write_archive("C.wad.client", &[("c", b"3")]);

    let index = GameIndex::build(install.game_dir()).unwrap();

    assert_eq!(index.archives().len(), 3);
    let b = index.archive_by_file_name("B.wad.client").unwrap();
    assert_eq!(b.index(), 1);
    assert_eq!(index.skipped().len(), 1);
    assert_eq!(index.skipped()[0].archive, b);
    assert!(matches!(
        index.skipped()[0].error,
        ArchiveReadError::Mount(_)
    ));
    assert_eq!(index.len(), 2);
    assert!(index.contains(chunk_hash("a")));
    assert!(index.contains(chunk_hash("c")));
}

#[test]
fn a_build_with_every_archive_skipped_is_an_empty_index() {
    let install = Installation::new();
    install.write_garbage("A.wad.client", b"nope");

    let index = GameIndex::build(install.game_dir()).unwrap();
    assert!(index.is_empty());
    assert_eq!(index.skipped().len(), 1);
    assert_eq!(index.archives().len(), 1);
}

#[test]
fn a_game_dir_without_data_final_does_not_build() {
    let root = tempfile::tempdir().unwrap();
    let game_dir = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();

    let error = GameIndex::build(&game_dir).unwrap_err();
    assert!(matches!(error, BuildError::MissingDataFinal { path } if path == game_dir));
}

#[test]
fn build_from_archives_names_each_archive_relative_to_root() {
    let install = Installation::new();
    let a = install.write_archive("Champions/A.wad.client", &[("a", b"1")]);
    let b = install.write_archive("B.wad.client", &[("b", b"2")]);

    let index = GameIndex::build_from_archives(&install.data_final(), &[a.clone(), b]).unwrap();
    let names: Vec<&str> = index.archives().iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["B.wad.client", "Champions/A.wad.client"]);
    assert_eq!(index.archives()[1].path, a);

    let relative = GameIndex::build_from_archives(
        &install.data_final(),
        &[Utf8PathBuf::from("Champions/A.wad.client")],
    )
    .unwrap();
    assert_eq!(relative.archives()[0].path, a);
    assert_eq!(relative.len(), 1);
}

#[test]
fn build_from_archives_rejects_a_path_outside_root() {
    let install = Installation::new();
    let other = tempfile::tempdir().unwrap();
    let outside = Utf8PathBuf::from_path_buf(other.path().join("X.wad.client")).unwrap();
    common::write_archive(&outside, &[("x", b"1")]);

    let error =
        GameIndex::build_from_archives(&install.data_final(), std::slice::from_ref(&outside))
            .unwrap_err();
    assert!(matches!(
        error,
        BuildError::ArchiveOutsideRoot { archive, .. } if archive == outside
    ));

    let error = GameIndex::build_from_archives(
        &install.data_final(),
        &[Utf8PathBuf::from("../X.wad.client")],
    )
    .unwrap_err();
    assert!(matches!(error, BuildError::ArchiveOutsideRoot { .. }));
}

#[test]
fn chunk_hash_matches_ltk_wad_for_mixed_case_input() {
    let path = "ASSETS/Characters/Aatrox/Skins/Base/Aatrox.SKN";
    assert_eq!(chunk_hash(path), WadHash::hash_str(path));
    assert_eq!(chunk_hash(path), chunk_hash(&path.to_ascii_lowercase()));
}

#[test]
fn dominant_holder_breaks_ties_toward_the_lower_id_and_is_none_for_absent_hashes() {
    let install = Installation::new();
    install.write_archive("A.wad.client", &[("1", b"x"), ("2", b"x"), ("3", b"x")]);
    install.write_archive(
        "B.wad.client",
        &[("2", b"x"), ("3", b"x"), ("4", b"x"), ("5", b"x")],
    );

    let index = GameIndex::build(install.game_dir()).unwrap();
    let a = index.archive_by_file_name("A.wad.client").unwrap();
    let b = index.archive_by_file_name("B.wad.client").unwrap();
    let hashes = |paths: &[&str]| paths.iter().map(|p| chunk_hash(p)).collect::<Vec<_>>();

    assert_eq!(
        index.dominant_holder(&hashes(&["2", "3", "4", "5"])),
        Some(b)
    );
    assert_eq!(index.dominant_holder(&hashes(&["1", "2", "3"])), Some(a));
    assert_eq!(index.dominant_holder(&hashes(&["2", "3"])), Some(a));
    assert_eq!(index.dominant_holder(&hashes(&["nope", "nada"])), None);
    assert_eq!(index.dominant_holder(&[]), None);
}
