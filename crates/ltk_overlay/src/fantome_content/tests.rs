use super::*;
use crate::test_support::hash;
use ltk_wad::WadChunkCompression;
use std::io::{Cursor, Write};

/// The single chunk every packed-WAD fixture holds.
const PACKED_CHUNK_PATH: &str = "packed/file.bin";

fn make_fantome_zip(entries: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
    let buffer = Vec::new();
    let cursor = Cursor::new(buffer);
    let mut zip = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    let mut cursor = zip.finish().unwrap();
    cursor.set_position(0);
    cursor
}

/// Build a ZIP (entries are Deflated) and overwrite every CRC32 field with
/// `0xDEADBEEF`, simulating Fantome creators that emit incorrect CRCs.
///
/// The blind signature scan should hit exactly one local and one central
/// header per entry; asserting the count catches a signature that spuriously
/// matched inside compressed data (which would clobber unrelated bytes).
fn make_fantome_zip_corrupt_crc(entries: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
    let cursor = make_fantome_zip(entries);
    let mut bytes = cursor.into_inner();

    let mut local_patched = 0usize;
    let mut central_patched = 0usize;
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        let sig = u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        match sig {
            // Local file header: CRC32 is at +14
            0x0403_4b50 => {
                if i + 18 <= bytes.len() {
                    bytes[i + 14..i + 18].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
                    local_patched += 1;
                }
                i += 4;
            }
            // Central directory header: CRC32 is at +16
            0x0201_4b50 => {
                if i + 20 <= bytes.len() {
                    bytes[i + 16..i + 20].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
                    central_patched += 1;
                }
                i += 4;
            }
            _ => i += 1,
        }
    }

    assert_eq!(
        local_patched,
        entries.len(),
        "expected exactly one local-header CRC per entry (spurious/missing signature match)"
    );
    assert_eq!(
        central_patched,
        entries.len(),
        "expected exactly one central-header CRC per entry (spurious/missing signature match)"
    );

    Cursor::new(bytes)
}

/// Build a minimal in-memory packed WAD containing a single uncompressed
/// chunk, for exercising the packed-WAD code paths.
fn make_packed_wad_bytes(payload: &[u8]) -> Vec<u8> {
    make_packed_wad_bytes_with(payload, WadChunkCompression::None)
}

/// The same fixture stored under an explicit codec, for the paths that care
/// which one a chunk arrives in.
fn make_packed_wad_bytes_with(payload: &[u8], compression: WadChunkCompression) -> Vec<u8> {
    use ltk_wad::{WadBuilder, WadChunkBuilder};

    let payload = payload.to_vec();
    let mut cursor = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(
            WadChunkBuilder::default()
                .with_path(PACKED_CHUNK_PATH)
                .with_force_compression(compression),
        )
        .build_to_writer(&mut cursor, move |_hash, c| {
            c.write_all(&payload)?;
            Ok(())
        })
        .expect("build packed WAD");
    cursor.into_inner()
}

fn make_info_json(name: &str) -> Vec<u8> {
    make_info_json_with_license(name, None)
}

fn make_info_json_with_license(
    name: &str,
    license: Option<ltk_fantome::FantomeLicense>,
) -> Vec<u8> {
    serde_json::to_vec(&ltk_fantome::FantomeInfo {
        name: name.to_string(),
        author: "Author".to_string(),
        version: "1.0.0".to_string(),
        description: "Desc".to_string(),
        license,
        tags: Vec::new(),
        champions: Vec::new(),
        maps: Vec::new(),
        layers: std::collections::HashMap::new(),
        hashtables: Vec::new(),
        extra: Default::default(),
    })
    .unwrap()
}

#[test]
fn new_with_valid_zip() {
    let cursor = make_fantome_zip(&[("META/info.json", &make_info_json("Test"))]);
    assert!(FantomeContent::new(cursor).is_ok());
}

#[test]
fn new_with_invalid_data() {
    let cursor = Cursor::new(b"not a zip".to_vec());
    assert!(FantomeContent::new(cursor).is_err());
}

#[test]
fn mod_project_reads_info_json() {
    let cursor = make_fantome_zip(&[("META/info.json", &make_info_json("My Mod"))]);
    let mut content = FantomeContent::new(cursor).unwrap();
    let project = content.mod_project().unwrap();
    assert_eq!(project.display_name, "My Mod");
    assert_eq!(project.version, "1.0.0");
}

#[test]
fn mod_project_surfaces_license() {
    let cursor = make_fantome_zip(&[(
        "META/info.json",
        &make_info_json_with_license(
            "Licensed Mod",
            Some(ltk_fantome::FantomeLicense::Spdx("MIT".to_string())),
        ),
    )]);
    let mut content = FantomeContent::new(cursor).unwrap();

    assert_eq!(
        content.mod_project().unwrap().license,
        Some(ModProjectLicense::Spdx("MIT".to_string()))
    );
}

#[test]
fn mod_project_missing_info_json() {
    let cursor = make_fantome_zip(&[("WAD/test.wad.client/file", b"data")]);
    let mut content = FantomeContent::new(cursor).unwrap();
    assert!(content.mod_project().is_err());
}

#[test]
fn mod_project_handles_bom() {
    let info_str = format!(
        "\u{feff}{}",
        serde_json::to_string(&ltk_fantome::FantomeInfo {
            name: "BOM Mod".to_string(),
            author: "Author".to_string(),
            version: "1.0.0".to_string(),
            description: "Desc".to_string(),
            license: None,
            tags: Vec::new(),
            champions: Vec::new(),
            maps: Vec::new(),
            layers: std::collections::HashMap::new(),
            hashtables: Vec::new(),
            extra: Default::default(),
        })
        .unwrap()
    );
    let cursor = make_fantome_zip(&[("META/info.json", info_str.as_bytes())]);
    let mut content = FantomeContent::new(cursor).unwrap();
    let project = content.mod_project().unwrap();
    assert_eq!(project.display_name, "BOM Mod");
}

#[test]
fn list_layer_wads_finds_directory_wads() {
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Test")),
        ("WAD/Aatrox.wad.client/file1", b"data1"),
        ("WAD/Aatrox.wad.client/file2", b"data2"),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();
    let wads = content.list_layer_wads("base").unwrap();
    assert_eq!(wads.len(), 1);
    // WAD names are canonicalized to lowercase for case-insensitive matching.
    assert!(wads.contains(&"aatrox.wad.client".to_string()));
}

#[test]
fn read_wad_overrides_lowercase_wad_folder() {
    // Some creators package content under a lowercase `wad/` folder. The
    // archive scanner must recognize it case-insensitively, otherwise the
    // mod's entire WAD content is silently dropped and it never loads.
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Lowercase")),
        ("wad/Aatrox.wad.client/file1.bin", b"data1"),
        ("wad/Aatrox.wad.client/sub/file2.bin", b"data2"),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();

    let wads = content.list_layer_wads("base").unwrap();
    assert_eq!(wads, vec!["aatrox.wad.client"]);

    let overrides = content
        .read_wad_overrides("base", "Aatrox.wad.client")
        .unwrap();
    assert_eq!(overrides.len(), 2);
    let paths: Vec<&str> = overrides.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"file1.bin"));
    assert!(paths.contains(&"sub/file2.bin"));

    // Pass-2 single-file read must also resolve via the lowercase folder.
    let bytes = content
        .read_wad_override_file("base", "aatrox.wad.client", Utf8Path::new("file1.bin"))
        .unwrap();
    assert_eq!(bytes, b"data1");
}

#[test]
fn read_raw_overrides_lowercase_raw_folder() {
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Lowercase")),
        ("raw/assets/characters/aatrox/skin0.bin", b"raw_data"),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();

    let overrides = content.read_raw_overrides().unwrap();
    assert_eq!(overrides.len(), 1);
    assert_eq!(
        overrides[0].0.as_str(),
        "assets/characters/aatrox/skin0.bin"
    );
    assert_eq!(overrides[0].1, b"raw_data");

    let bytes = content
        .read_raw_override_file(Utf8Path::new("assets/characters/aatrox/skin0.bin"))
        .unwrap();
    assert_eq!(bytes, b"raw_data");
}

#[test]
fn read_wad_overrides_lowercase_packed_wad_folder() {
    // Packed WAD directly under a lowercase `wad/` folder.
    let wad_bytes = make_packed_wad_bytes(b"packed");
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Lowercase Packed")),
        ("wad/Packed.wad.client", &wad_bytes),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();

    let wads = content.list_layer_wads("base").unwrap();
    assert_eq!(wads, vec!["packed.wad.client"]);

    let overrides = content
        .read_wad_overrides("base", "Packed.wad.client")
        .unwrap();
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].1, b"packed");
}

#[test]
fn strip_prefix_ci_matches_case_insensitively() {
    assert_eq!(strip_prefix_ci("WAD/foo", "WAD/"), Some("foo"));
    assert_eq!(strip_prefix_ci("wad/foo", "WAD/"), Some("foo"));
    assert_eq!(strip_prefix_ci("Wad/foo", "WAD/"), Some("foo"));
    assert_eq!(strip_prefix_ci("RAW/foo", "RAW/"), Some("foo"));
    assert_eq!(strip_prefix_ci("raw/foo", "RAW/"), Some("foo"));
    assert_eq!(strip_prefix_ci("META/foo", "WAD/"), None);
    assert_eq!(strip_prefix_ci("wa", "WAD/"), None);
}

#[test]
fn list_layer_wads_non_base_returns_empty() {
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Test")),
        ("WAD/Aatrox.wad.client/file1", b"data1"),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();
    let wads = content.list_layer_wads("chroma").unwrap();
    assert!(wads.is_empty());
}

#[test]
fn read_wad_overrides_directory_style() {
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Test")),
        ("WAD/Aatrox.wad.client/file1.bin", b"data1"),
        ("WAD/Aatrox.wad.client/sub/file2.bin", b"data2"),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();
    let overrides = content
        .read_wad_overrides("base", "Aatrox.wad.client")
        .unwrap();
    assert_eq!(overrides.len(), 2);
    let paths: Vec<&str> = overrides.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"file1.bin"));
    assert!(paths.contains(&"sub/file2.bin"));
}

#[test]
fn read_wad_overrides_non_base_returns_empty() {
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Test")),
        ("WAD/Aatrox.wad.client/file1.bin", b"data1"),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();
    let overrides = content
        .read_wad_overrides("chroma", "Aatrox.wad.client")
        .unwrap();
    assert!(overrides.is_empty());
}

#[test]
fn read_raw_overrides_from_raw_dir() {
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Test")),
        ("RAW/assets/characters/aatrox/skin0.bin", b"raw_data"),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();
    let overrides = content.read_raw_overrides().unwrap();
    assert_eq!(overrides.len(), 1);
    assert_eq!(
        overrides[0].0.as_str(),
        "assets/characters/aatrox/skin0.bin"
    );
    assert_eq!(overrides[0].1, b"raw_data");
}

#[test]
fn read_raw_override_file_single() {
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Test")),
        ("RAW/assets/characters/aatrox/skin0.bin", b"raw_data"),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();
    let bytes = content
        .read_raw_override_file(Utf8Path::new("assets/characters/aatrox/skin0.bin"))
        .unwrap();
    assert_eq!(bytes, b"raw_data");
}

#[test]
fn read_wad_override_file_directory_style() {
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Test")),
        ("WAD/Aatrox.wad.client/file1.bin", b"data1"),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();
    let bytes = content
        .read_wad_override_file("base", "Aatrox.wad.client", Utf8Path::new("file1.bin"))
        .unwrap();
    assert_eq!(bytes, b"data1");
}

#[test]
fn loads_archive_with_bad_crc32() {
    // Some Fantome creators emit incorrect CRC32 values in the ZIP central
    // directory. The zip crate's CRC check would otherwise reject these
    // archives with "Invalid checksum" - verify we tolerate that and read
    // the underlying data correctly.
    let cursor = make_fantome_zip_corrupt_crc(&[
        ("META/info.json", &make_info_json("Bad CRC Mod")),
        ("WAD/Aatrox.wad.client/file1.bin", b"data1"),
        ("RAW/assets/raw1.bin", b"raw_data"),
    ]);
    let mut content = FantomeContent::new(cursor).expect("FantomeContent::new");

    let project = content.mod_project().expect("mod_project");
    assert_eq!(project.display_name, "Bad CRC Mod");

    let overrides = content
        .read_wad_overrides("base", "Aatrox.wad.client")
        .expect("read_wad_overrides");
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].1, b"data1");

    let raw = content.read_raw_overrides().expect("read_raw_overrides");
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0].1, b"raw_data");
}

#[test]
fn loads_packed_wad_with_bad_crc32() {
    // A packed WAD is mounted via Wad::mount during FantomeContent::new - the
    // downstream "WAD mounting" path the fix targets. Verify it and the packed
    // branch of read_wad_override_file tolerate a corrupt CRC.
    const PACKED_PAYLOAD: &[u8] = b"packed_payload_bytes";
    let wad_bytes = make_packed_wad_bytes(PACKED_PAYLOAD);
    let cursor = make_fantome_zip_corrupt_crc(&[
        ("META/info.json", &make_info_json("Packed Bad CRC")),
        ("WAD/Packed.wad.client", &wad_bytes),
    ]);
    let mut content = FantomeContent::new(cursor).expect("FantomeContent::new");

    let overrides = content
        .read_wad_overrides("base", "Packed.wad.client")
        .expect("read_wad_overrides");
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].1, PACKED_PAYLOAD);

    // Packed chunks are exposed as hex-hash filenames; round-trip a lookup.
    let hex_name = overrides[0].0.clone();
    let single = content
        .read_wad_override_file("base", "Packed.wad.client", &hex_name)
        .expect("read_wad_override_file");
    assert_eq!(single, PACKED_PAYLOAD);
}

#[test]
fn streaming_visits_the_same_overrides_as_the_bulk_read() {
    // The streaming visitor exists so the metadata pass holds one chunk at
    // a time instead of a whole WAD; it must surface exactly the entries
    // the bulk read does, for both directory-style and packed WADs.
    let wad_bytes = make_packed_wad_bytes(b"packed_payload");
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Streamed")),
        ("WAD/Aatrox.wad.client/file1.bin", b"data1"),
        ("WAD/Aatrox.wad.client/sub/file2.bin", b"data2"),
        ("WAD/Packed.wad.client", &wad_bytes),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();

    for wad_name in ["Aatrox.wad.client", "Packed.wad.client"] {
        let mut streamed: Vec<(Utf8PathBuf, Vec<u8>)> = Vec::new();
        content
            .visit_wad_override("base", wad_name, &mut |rel_path, bytes| {
                streamed.push((rel_path, bytes));
                Ok(())
            })
            .unwrap();

        let mut bulk = content.read_wad_overrides("base", wad_name).unwrap();
        bulk.sort();
        streamed.sort();
        assert_eq!(streamed, bulk, "streaming and bulk disagree for {wad_name}");
    }
}

#[test]
fn streaming_visits_the_same_raw_overrides_as_the_bulk_read() {
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Streamed Raw")),
        ("RAW/assets/a.bin", b"raw_a"),
        ("RAW/assets/b.bin", b"raw_b"),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();

    let mut streamed: Vec<(Utf8PathBuf, Vec<u8>)> = Vec::new();
    content
        .visit_raw_override(&mut |rel_path, bytes| {
            streamed.push((rel_path, bytes));
            Ok(())
        })
        .unwrap();

    let mut bulk = content.read_raw_overrides().unwrap();
    bulk.sort();
    streamed.sort();
    assert_eq!(streamed, bulk);
}

#[test]
fn packed_wad_is_not_read_before_first_access() {
    // The exact-match skip path only needs META/info.json and a file stat,
    // so construction and mod_project must not touch packed WAD bytes. A
    // deliberately invalid packed WAD makes that observable at the seam:
    // an eager mount would fail in `new`, a lazy one only on first read.
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Lazy")),
        ("WAD/Broken.wad.client", b"not a wad at all"),
    ]);
    let mut content = FantomeContent::new(cursor).expect("new must not mount packed WADs");

    let project = content.mod_project().expect("mod_project");
    assert_eq!(project.display_name, "Lazy");

    // The WAD is still listed - the index alone knows its name.
    let wads = content.list_layer_wads("base").unwrap();
    assert_eq!(wads, vec!["broken.wad.client"]);

    // First real access is where the invalid WAD surfaces.
    assert!(
        content
            .read_wad_overrides("base", "Broken.wad.client")
            .is_err()
    );
    assert!(
        content
            .read_wad_override_file(
                "base",
                "Broken.wad.client",
                Utf8Path::new("0000000000000000.bin"),
            )
            .is_err()
    );
}

/// A packed WAD already holds its chunks in the form an overlay WAD wants
/// them, so the provider offers those bytes together with the TOC facts
/// describing them and the build never decodes them.
#[test]
fn a_packed_wad_offers_its_chunks_already_compressed() {
    const PAYLOAD: &[u8] = b"a packed chunk with enough text in it to compress";

    let wad_bytes = make_packed_wad_bytes_with(PAYLOAD, WadChunkCompression::Zstd);
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Packed")),
        ("WAD/Packed.wad.client", &wad_bytes),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();

    let hex_name = Utf8PathBuf::from(format!("{:016x}.bin", hash(PACKED_CHUNK_PATH)));
    let offered = content
        .read_wad_override_compressed("base", "Packed.wad.client", &hex_name)
        .expect("the packed WAD is readable")
        .expect("a packed chunk has a stored form to offer");

    assert_eq!(offered.compression, WadChunkCompression::Zstd);
    assert_eq!(offered.uncompressed_size, PAYLOAD.len());
    assert_eq!(
        offered.claimed_checksum,
        xxhash_rust::xxh3::xxh3_64(&offered.compressed),
        "the claim must be the packed WAD's own TOC checksum for these bytes"
    );
    assert_eq!(
        zstd::decode_all(offered.compressed.as_slice()).unwrap(),
        PAYLOAD,
        "the offered bytes must be the chunk's stored form, not a re-encoding"
    );
}

/// Everything a fantome holds outside a packed WAD is a loose file with no
/// stored form, so the provider declines and the build reads it as usual.
/// Declining is never an error: it is the answer for a chunk this archive
/// cannot pass through, including one it does not hold at all.
#[test]
fn loose_entries_have_no_stored_form_to_offer() {
    let wad_bytes = make_packed_wad_bytes(b"packed");
    let cursor = make_fantome_zip(&[
        ("META/info.json", &make_info_json("Mixed")),
        ("WAD/Aatrox.wad.client/file1.bin", b"data1"),
        ("WAD/Packed.wad.client", &wad_bytes),
    ]);
    let mut content = FantomeContent::new(cursor).unwrap();

    let packed_hex = Utf8PathBuf::from(format!("{:016x}.bin", hash(PACKED_CHUNK_PATH)));
    let declined = [
        // A directory-style override: a loose file in the ZIP.
        ("base", "Aatrox.wad.client", Utf8PathBuf::from("file1.bin")),
        // A WAD the archive has no packed copy of.
        ("base", "Ahri.wad.client", packed_hex.clone()),
        // A chunk the packed WAD does not hold.
        (
            "base",
            "Packed.wad.client",
            Utf8PathBuf::from("0000000000000000.bin"),
        ),
        // Fantome WAD content is base-layer only.
        ("chroma", "Packed.wad.client", packed_hex),
    ];

    for (layer, wad_name, rel_path) in declined {
        assert_eq!(
            content
                .read_wad_override_compressed(layer, wad_name, &rel_path)
                .expect("declining is not an error"),
            None,
            "'{layer}/{wad_name}/{rel_path}' has no stored form to offer"
        );
    }
}

#[test]
fn is_wad_file_name_variants() {
    assert!(is_wad_file_name("test.wad.client"));
    assert!(is_wad_file_name("test.wad"));
    assert!(is_wad_file_name("test.wad.mobile"));
    assert!(!is_wad_file_name("test.txt"));
    assert!(!is_wad_file_name(""));
}

#[test]
fn hex_chunk_names_are_zero_padded() {
    // WadHash's Display does not zero-pad, but its LowerHex forwards the
    // formatter, so `{:016x}` still yields the 16 digits that the file stem
    // check in read_wad_override_file accepts.
    let name = format!("{:016x}.bin", WadHash(0xff));

    assert_eq!(name, "00000000000000ff.bin");
    assert_eq!(Utf8Path::new(&name).file_stem().unwrap().len(), 16);
}
