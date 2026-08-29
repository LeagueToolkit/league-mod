use super::*;

fn info_json(license: Option<FantomeLicense>) -> serde_json::Value {
    let info = FantomeInfo {
        name: "Test Mod".to_string(),
        author: "Alice".to_string(),
        version: "1.0.0".to_string(),
        description: "A test mod".to_string(),
        license,
        tags: vec![],
        champions: vec![],
        maps: vec![],
        layers: HashMap::new(),
        hashtables: vec![],
        extra: Default::default(),
    };
    serde_json::to_value(&info).unwrap()
}

#[test]
fn info_json_emits_spdx_license() {
    let json = info_json(Some(FantomeLicense::Spdx("MIT".to_string())));
    assert_eq!(json["License"], serde_json::json!("MIT"));
}

#[test]
fn info_json_emits_custom_license() {
    let json = info_json(Some(FantomeLicense::Custom {
        name: "My License".to_string(),
        url: Some("https://example.com/terms".to_string()),
    }));
    assert_eq!(
        json["License"],
        serde_json::json!({ "Name": "My License", "Url": "https://example.com/terms" })
    );

    let json = info_json(Some(FantomeLicense::Custom {
        name: "My License".to_string(),
        url: None,
    }));
    assert_eq!(json["License"], serde_json::json!({ "Name": "My License" }));
}

#[test]
fn info_json_omits_absent_license() {
    let json = info_json(None);
    assert!(
        json.get("License").is_none(),
        "License key must be omitted entirely, got: {json}"
    );
}

#[test]
fn legacy_info_json_without_license_still_parses() {
    let legacy = r#"{
            "Name": "Old Mod",
            "Author": "Someone",
            "Version": "1.0.0",
            "Description": "Packed before licenses existed"
        }"#;

    let info: FantomeInfo = serde_json::from_str(legacy).unwrap();

    assert_eq!(info.name, "Old Mod");
    assert_eq!(info.license, None);
}

#[test]
fn custom_license_rejects_unknown_field() {
    let typoed = r#"{
            "Name": "Test",
            "Author": "Test",
            "Version": "1.0.0",
            "Description": "Test",
            "License": { "Name": "My License", "Ur1": "https://example.com/terms" }
        }"#;

    assert!(
        serde_json::from_str::<FantomeInfo>(typoed).is_err(),
        "a misspelled license key must not parse as a URL-less license"
    );
}

#[test]
fn info_json_round_trips_the_hashtables_manifest() {
    let json = serde_json::json!({
        "Name": "Old Summoners Rift",
        "Author": "TheKillerey, Crauzer",
        "Version": "1.0.0",
        "Description": "Brings back the classic Summoners Rift map",
        "Hashtables": [
            {
                "Path": "META/hashes/game.hashes.txt",
                "Category": "game",
                "Algorithm": "xxh64",
                "Bits": 64
            },
            {
                "Path": "META/hashes/binentries.hashes.txt",
                "Category": "binentries",
                "Algorithm": "fnv1a_32",
                "Bits": 32
            }
        ]
    });

    let info: FantomeInfo = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(info.hashtables.len(), 2);
    assert_eq!(serde_json::to_value(&info).unwrap(), json);
}

#[test]
fn info_json_without_hashtables_serializes_without_the_field() {
    let info = FantomeInfo {
        name: "A mod".to_owned(),
        ..Default::default()
    };
    let value = serde_json::to_value(&info).unwrap();
    assert!(value.get("Hashtables").is_none());

    // And the archive entry itself carries no trace of the new fields, so an
    // archive without tables is byte-identical to one written before them.
    use std::io::{Cursor, Read as _};

    use crate::FantomeWriter;

    let mut writer = FantomeWriter::new(Cursor::new(Vec::new()));
    writer.write_info(&info).unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entry_text = String::new();
    archive
        .by_name("META/info.json")
        .unwrap()
        .read_to_string(&mut entry_text)
        .unwrap();
    assert!(!entry_text.contains("Hashtables"));
    assert!(!entry_text.contains("extra"));
}

#[test]
fn a_hashtable_round_trips_through_the_archive() {
    use std::io::Cursor;

    use crate::{FantomeReader, FantomeWriter};

    let manifest = FantomeHashtable {
        path: "META/hashes/game.hashes.txt".to_owned(),
        category: ltk_hashtable::Category::Game,
        algorithm: ltk_hashtable::Algorithm::Xxh64,
        bits: 64,
    };
    let table = ltk_hashtable::Hashtable::from_reader(
        &b"ASSETS/Custom/One.tex\nassets/custom/two.bin\n"[..],
    )
    .unwrap();

    let info = FantomeInfo {
        name: "Tabled".to_owned(),
        hashtables: vec![manifest.clone()],
        ..Default::default()
    };

    let mut writer = FantomeWriter::new(Cursor::new(Vec::new()));
    writer.write_info(&info).unwrap();
    writer.write_hashtable(&manifest, &table).unwrap();
    let archive = writer.finish().unwrap().into_inner();

    let mut reader = FantomeReader::new(Cursor::new(archive)).unwrap();
    let tables = reader.read_hashtables().unwrap();
    assert_eq!(tables.len(), 1);
    let (entry, read_back) = &tables[0];
    assert_eq!(entry.path(), "META/hashes/game.hashes.txt");
    assert_eq!(*entry.category(), ltk_hashtable::Category::Game);
    assert_eq!(read_back, &table);
}

#[test]
fn an_undeclared_hashes_entry_is_not_read() {
    use std::io::Cursor;

    use crate::{FantomeReader, FantomeWriter};

    // The file exists in the archive, but no manifest entry declares it.
    let undeclared = FantomeHashtable {
        path: "META/hashes/game.hashes.txt".to_owned(),
        category: ltk_hashtable::Category::Game,
        algorithm: ltk_hashtable::Algorithm::Xxh64,
        bits: 64,
    };
    let table = ltk_hashtable::Hashtable::from_reader(&b"assets/custom/one.tex\n"[..]).unwrap();

    let mut writer = FantomeWriter::new(Cursor::new(Vec::new()));
    writer
        .write_info(&FantomeInfo {
            name: "Undeclared".to_owned(),
            ..Default::default()
        })
        .unwrap();
    writer.write_hashtable(&undeclared, &table).unwrap();
    let archive = writer.finish().unwrap().into_inner();

    let mut reader = FantomeReader::new(Cursor::new(archive)).unwrap();
    assert!(reader.read_hashtables().unwrap().is_empty());
}

#[test]
fn a_meta_hashes_entry_classifies_as_a_hashtable() {
    use crate::{FantomeEntry, classify_entry};

    assert_eq!(
        classify_entry("META/hashes/game.hashes.txt"),
        Some(FantomeEntry::Hashtable("game.hashes.txt"))
    );
    assert_eq!(
        classify_entry("meta/HASHES/game.imported.hashes.txt"),
        Some(FantomeEntry::Hashtable("game.imported.hashes.txt"))
    );
    // A directory entry is not a file, and the bare directory places nothing.
    assert_eq!(classify_entry("META/hashes/"), None);
}

// The rewrite: raw-copy every entry, merge in harvested names, never touch
// what is already there.

mod rewrite {
    use std::io::Cursor;

    use ltk_hashtable::{Category, Hashtable};

    use crate::{
        FantomeInfo, FantomeReader, FantomeWriter, RewriteOutcome, WadExtractOptions,
        add_hashtables,
    };

    /// An archive holding `META/info.json` (written last) and one WAD file
    /// entry (written first, so its local header sits at offset 0), with the
    /// WAD entry's CRC32 overwritten the way tools in the wild do.
    fn archive_with_wrong_crc(info: &FantomeInfo, payload: &[u8]) -> Vec<u8> {
        let mut writer = FantomeWriter::new(Cursor::new(Vec::new()));
        writer
            .write_wad_entry("Aatrox.wad.client", "data/x.bin", &mut &payload[..])
            .unwrap();
        writer.write_info(info).unwrap();
        let mut bytes = writer.finish().unwrap().into_inner();

        // The first local file header starts at offset 0; its CRC32 field is
        // bytes 14..18. Stamp the same wrong value over every occurrence so
        // the central directory copy changes too.
        let real_crc: [u8; 4] = bytes[14..18].try_into().unwrap();
        let wrong_crc = [0xEF, 0xBE, 0xAD, 0xDE];
        assert_ne!(real_crc, wrong_crc);
        let mut i = 0;
        while i + 4 <= bytes.len() {
            if bytes[i..i + 4] == real_crc {
                bytes[i..i + 4].copy_from_slice(&wrong_crc);
            }
            i += 1;
        }
        bytes
    }

    #[test]
    fn a_rewrite_adds_its_tables_and_raw_copies_every_entry() {
        let payload = b"wad payload bytes";
        let bytes = archive_with_wrong_crc(
            &FantomeInfo {
                name: "Mod".to_owned(),
                ..Default::default()
            },
            payload,
        );

        let mut reader = FantomeReader::new(Cursor::new(bytes)).unwrap();
        let harvested = Hashtable::from_names(["ASSETS/Custom/New.tex"]).unwrap();
        let mut sink = Cursor::new(Vec::new());
        let outcome =
            add_hashtables(&mut reader, &mut sink, &[(Category::Game, harvested)]).unwrap();
        assert_eq!(outcome, RewriteOutcome::Rewritten { names_added: 1 });

        let out = sink.into_inner();

        // The wrong CRC32 was carried through, not recomputed.
        let mut archive = zip::ZipArchive::new(Cursor::new(out.clone())).unwrap();
        let entry = archive.by_name("WAD/Aatrox.wad.client/data/x.bin").unwrap();
        assert_eq!(entry.crc32(), 0xDEAD_BEEF);
        drop(entry);

        // The content survived: extraction (which bypasses CRC) yields the
        // original bytes.
        let mut out_reader = FantomeReader::new(Cursor::new(out.clone())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dir_path = camino::Utf8Path::from_path(dir.path()).unwrap();
        out_reader
            .extract_wads(dir_path, WadExtractOptions::new())
            .unwrap();
        let extracted =
            std::fs::read(dir_path.join("Aatrox.wad.client/data/x.bin").as_std_path()).unwrap();
        assert_eq!(extracted, payload);

        // The manifest declares the table and the table reads back.
        let info = out_reader.read_info().unwrap();
        assert_eq!(info.hashtables.len(), 1);
        assert_eq!(info.hashtables[0].path, "META/hashes/game.hashes.txt");
        let tables = out_reader.read_hashtables().unwrap();
        assert_eq!(
            tables[0].1.names().collect::<Vec<_>>(),
            ["ASSETS/Custom/New.tex"]
        );
    }

    /// The reason a repair edits rather than repacks: the entry it named
    /// carries its new bytes, and the archive around it was copied.
    #[test]
    fn a_replaced_entry_is_written_and_every_other_is_raw_copied() {
        const CHANGED: &str = "WAD/Aatrox.wad.client/data/x.bin";

        let bytes = archive_with_wrong_crc(
            &FantomeInfo {
                name: "Mod".to_owned(),
                ..Default::default()
            },
            b"wad payload bytes",
        );

        let mut reader = FantomeReader::new(Cursor::new(bytes)).unwrap();
        let mut sink = Cursor::new(Vec::new());
        let outcome = crate::replace_entries(
            &mut reader,
            &mut sink,
            &[(CHANGED, b"repaired bytes".as_slice())],
            &[],
        )
        .unwrap();
        assert_eq!(outcome, RewriteOutcome::Rewritten { names_added: 0 });

        let mut out_reader = FantomeReader::new(Cursor::new(sink.into_inner())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dir_path = camino::Utf8Path::from_path(dir.path()).unwrap();
        out_reader
            .extract_wads(dir_path, WadExtractOptions::new())
            .unwrap();

        let extracted =
            std::fs::read(dir_path.join("Aatrox.wad.client/data/x.bin").as_std_path()).unwrap();
        assert_eq!(extracted, b"repaired bytes");
    }

    /// An entry given but not held is added, so a caller does not have to know
    /// which of the two it is doing.
    #[test]
    fn an_entry_the_archive_does_not_hold_is_added() {
        let bytes = archive_with_wrong_crc(&FantomeInfo::default(), b"payload");

        let mut reader = FantomeReader::new(Cursor::new(bytes)).unwrap();
        let mut sink = Cursor::new(Vec::new());
        crate::replace_entries(
            &mut reader,
            &mut sink,
            &[("WAD/Aatrox.wad.client/data/new.bin", b"fresh".as_slice())],
            &[],
        )
        .unwrap();

        let mut archive = zip::ZipArchive::new(Cursor::new(sink.into_inner())).unwrap();
        assert!(
            archive
                .by_name("WAD/Aatrox.wad.client/data/new.bin")
                .is_ok()
        );
        assert!(archive.by_name("WAD/Aatrox.wad.client/data/x.bin").is_ok());
    }

    /// Nothing to change is nothing written, so a caller can ask without
    /// first deciding whether it needs to.
    #[test]
    fn no_entries_and_no_names_leaves_the_sink_alone() {
        let bytes = archive_with_wrong_crc(&FantomeInfo::default(), b"payload");

        let mut reader = FantomeReader::new(Cursor::new(bytes)).unwrap();
        let mut sink = Cursor::new(Vec::new());
        let outcome = crate::replace_entries(&mut reader, &mut sink, &[], &[]).unwrap();

        assert_eq!(outcome, RewriteOutcome::Unchanged);
        assert!(sink.into_inner().is_empty());
    }
}

mod rewrite_merge {
    use std::io::Cursor;

    use ltk_hashtable::{Algorithm, Category, Hashtable};

    use crate::{
        FantomeHashtable, FantomeInfo, FantomeReader, FantomeWriter, RewriteOutcome, add_hashtables,
    };

    fn game_manifest() -> FantomeHashtable {
        FantomeHashtable {
            path: "META/hashes/game.hashes.txt".to_owned(),
            category: Category::Game,
            algorithm: Algorithm::Xxh64,
            bits: 64,
        }
    }

    fn archive_declaring(names: &[u8]) -> Vec<u8> {
        let manifest = game_manifest();
        let mut writer = FantomeWriter::new(Cursor::new(Vec::new()));
        writer
            .write_info(&FantomeInfo {
                name: "Mod".to_owned(),
                hashtables: vec![manifest.clone()],
                ..Default::default()
            })
            .unwrap();
        writer
            .write_hashtable(&manifest, &Hashtable::from_reader(names).unwrap())
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn a_harvest_that_adds_nothing_performs_no_rewrite() {
        let bytes = archive_declaring(
            b"ASSETS/Custom/Known.tex
",
        );
        let mut reader = FantomeReader::new(Cursor::new(bytes)).unwrap();

        // The same canonical name in another casing is not new.
        let harvested = Hashtable::from_names(["assets/custom/known.tex"]).unwrap();
        let mut sink = Cursor::new(Vec::new());
        let outcome =
            add_hashtables(&mut reader, &mut sink, &[(Category::Game, harvested)]).unwrap();

        assert_eq!(outcome, RewriteOutcome::Unchanged);
        assert!(sink.into_inner().is_empty());
    }

    #[test]
    fn a_merge_adds_to_an_existing_table_and_never_replaces_it() {
        let bytes = archive_declaring(
            b"b/Existing.tex
",
        );
        let mut reader = FantomeReader::new(Cursor::new(bytes)).unwrap();

        let harvested =
            Hashtable::from_names(["B/EXISTING.TEX", "a/new.tex", "A/NEW.TEX"]).unwrap();
        let mut sink = Cursor::new(Vec::new());
        let outcome =
            add_hashtables(&mut reader, &mut sink, &[(Category::Game, harvested)]).unwrap();
        assert_eq!(outcome, RewriteOutcome::Rewritten { names_added: 1 });

        let mut out_reader = FantomeReader::new(Cursor::new(sink.into_inner())).unwrap();
        let info = out_reader.read_info().unwrap();
        assert_eq!(info.hashtables, vec![game_manifest()]);

        let tables = out_reader.read_hashtables().unwrap();
        assert_eq!(tables.len(), 1);
        // The existing name survives in its authored casing; the new name
        // lands beside it, sorted in byte order.
        assert_eq!(
            tables[0].1.names().collect::<Vec<_>>(),
            ["a/new.tex", "b/Existing.tex"]
        );
    }
}

#[test]
fn an_unknown_info_field_survives_a_rewrite() {
    use std::io::{Cursor, Write as _};

    use ltk_hashtable::{Category, Hashtable};
    use zip::write::SimpleFileOptions;

    // An info.json written by a newer or different tool, with a field this
    // crate does not know.
    let info_json = serde_json::json!({
        "Name": "Mod",
        "Author": "A",
        "Version": "1.0.0",
        "Description": "",
        "SomeNewerField": {"nested": true}
    });
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("META/info.json", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(info_json.to_string().as_bytes()).unwrap();
    let bytes = zip.finish().unwrap().into_inner();

    let mut reader = crate::FantomeReader::new(Cursor::new(bytes)).unwrap();
    let harvested = Hashtable::from_names(["assets/custom/one.tex"]).unwrap();
    let mut sink = Cursor::new(Vec::new());
    crate::add_hashtables(&mut reader, &mut sink, &[(Category::Game, harvested)]).unwrap();

    let mut out_reader = crate::FantomeReader::new(Cursor::new(sink.into_inner())).unwrap();
    let info = out_reader.read_info().unwrap();
    assert_eq!(
        serde_json::to_value(&info).unwrap()["SomeNewerField"],
        serde_json::json!({"nested": true})
    );
}

#[test]
fn a_packed_wad_reads_back_whole() {
    use std::io::{Cursor, Write as _};

    use ltk_wad::{WadBuilder, WadChunkBuilder};
    use zip::write::SimpleFileOptions;

    let mut wad_bytes = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(WadChunkBuilder::default().with_path("data/x.bin"))
        .build_to_writer(&mut wad_bytes, |_, cursor| {
            cursor.write_all(b"chunk payload")?;
            Ok(())
        })
        .unwrap();
    let wad_bytes = wad_bytes.into_inner();

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("WAD/Aatrox.wad.client", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&wad_bytes).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let mut reader = crate::FantomeReader::new(Cursor::new(archive)).unwrap();
    assert_eq!(
        reader.read_packed_wad("Aatrox.wad.client").unwrap(),
        Some(wad_bytes)
    );
    assert_eq!(reader.read_packed_wad("Absent.wad.client").unwrap(), None);
}

#[test]
fn a_manifest_entry_the_rewrite_cannot_read_is_never_shadowed() {
    use std::io::{Cursor, Read as _};

    use ltk_hashtable::{Category, Hashtable};

    use crate::{FantomeReader, FantomeWriter, add_hashtables};

    // A manifest entry whose Bits no key can have, sitting at the
    // conventional path. This tool cannot read it - and must not touch it.
    let unreadable = FantomeHashtable {
        path: "META/hashes/game.hashes.txt".to_owned(),
        category: ltk_hashtable::Category::Game,
        algorithm: ltk_hashtable::Algorithm::Xxh64,
        bits: 0,
    };
    let mut writer = FantomeWriter::new(Cursor::new(Vec::new()));
    writer
        .write_info(&FantomeInfo {
            name: "Mod".to_owned(),
            hashtables: vec![unreadable.clone()],
            ..Default::default()
        })
        .unwrap();
    writer
        .write_hashtable(
            &unreadable,
            &Hashtable::from_names(["some/opaque.name"]).unwrap(),
        )
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();

    let mut reader = FantomeReader::new(Cursor::new(bytes)).unwrap();
    let harvested = Hashtable::from_names(["assets/custom/new.tex"]).unwrap();
    let mut sink = Cursor::new(Vec::new());
    add_hashtables(&mut reader, &mut sink, &[(Category::Game, harvested)]).unwrap();

    let out = sink.into_inner();
    let mut out_reader = FantomeReader::new(Cursor::new(out.clone())).unwrap();
    let info = out_reader.read_info().unwrap();

    // Both entries are declared, at two distinct paths.
    assert_eq!(info.hashtables.len(), 2);
    assert_eq!(info.hashtables[0], unreadable);
    assert_ne!(info.hashtables[1].path, unreadable.path);

    // The unreadable entry survived byte-for-byte.
    let mut archive = zip::ZipArchive::new(Cursor::new(out)).unwrap();
    let mut file = archive.by_name("META/hashes/game.hashes.txt").unwrap();
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap();
    assert_eq!(
        content,
        "some/opaque.name
"
    );
}

#[test]
fn a_sink_that_fails_part_way_surfaces_the_error() {
    use std::io::{self, Cursor, Seek, SeekFrom, Write};

    use ltk_hashtable::{Category, Hashtable};

    use crate::{FantomeInfo, FantomeReader, FantomeWriter, add_hashtables};

    /// A sink that refuses every write past a budget.
    struct FailingSink {
        inner: Cursor<Vec<u8>>,
        budget: usize,
    }

    impl Write for FailingSink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.budget < buf.len() {
                return Err(io::Error::other("disk full"));
            }
            self.budget -= buf.len();
            self.inner.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    impl Seek for FailingSink {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    let mut writer = FantomeWriter::new(Cursor::new(Vec::new()));
    writer
        .write_info(&FantomeInfo {
            name: "Mod".to_owned(),
            ..Default::default()
        })
        .unwrap();
    writer
        .write_wad_entry("Aatrox.wad.client", "assets/x.tex", &mut &b"payload"[..])
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();

    let mut reader = FantomeReader::new(Cursor::new(bytes)).unwrap();
    let harvested = Hashtable::from_names(["assets/custom/new.tex"]).unwrap();
    let sink = FailingSink {
        inner: Cursor::new(Vec::new()),
        budget: 64,
    };

    // The failure surfaces as an error rather than a truncated archive being
    // reported written; keeping the original safe is the temp-and-rename
    // above this call.
    add_hashtables(&mut reader, sink, &[(Category::Game, harvested)]).unwrap_err();
}
