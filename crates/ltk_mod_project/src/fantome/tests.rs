use super::*;
use crate::{
    Cancellation, ImportError, ImportProgress, ImportStage, ModProject, ModProjectAuthor,
    ModProjectHashtable, ModProjectLayer, ModProjectLicense, PackError, ProjectImporter,
    ProjectPacker, ProjectPath, ProjectPaths,
};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{FantomeInfo, FantomeLicense, FantomeReader};
use ltk_hashtable::Category;
use std::io::Cursor;
use std::sync::atomic::AtomicBool;
use tempfile::TempDir;

/// The temp directory's path, which packing takes as UTF-8.
fn utf8_dir(dir: &TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap()
}

fn test_project(license: Option<ModProjectLicense>) -> ModProject {
    ModProject {
        name: "test-mod".to_string(),
        display_name: "Test Mod".to_string(),
        version: "1.0.0".to_string(),
        description: "A test mod".to_string(),
        authors: vec![ModProjectAuthor::Name("Alice".to_string())],
        license,
        layers: ModProjectLayer::default_table(),
        ..Default::default()
    }
}

/// Write a minimal project tree with one base-layer WAD file.
fn write_project_tree(root: &Utf8Path) {
    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::create_dir_all(&wad_dir).unwrap();
    std::fs::write(wad_dir.join("data.bin"), b"content").unwrap();
}

fn try_pack(
    project: &ModProject,
    root: &Utf8Path,
) -> Result<Cursor<Vec<u8>>, PackError<FantomePackError>> {
    let mut buffer = Cursor::new(Vec::new());
    ProjectPacker::new(project.clone(), root.to_owned()).pack(FantomeFormat::new(&mut buffer))?;
    buffer.set_position(0);
    Ok(buffer)
}

fn pack(project: &ModProject, root: &Utf8Path) -> Cursor<Vec<u8>> {
    try_pack(project, root).unwrap()
}

/// Whether the archive's packed WAD `wad_name` holds the chunk `rel_path`
/// addresses.
///
/// A packed WAD keys its chunks by hash, so asking what a pack wrote means
/// mounting the WAD rather than looking an entry name up: the file names the
/// author used are not in the archive at all.
fn holds_chunk(archive: Cursor<Vec<u8>>, wad_name: &str, rel_path: &str) -> bool {
    let mut reader = FantomeReader::new(archive).unwrap();
    let Some(wad) = reader.mount_packed_wad(wad_name).unwrap() else {
        return false;
    };
    wad.chunks()
        .contains(ltk_wad::chunk_hash_of(Utf8Path::new(rel_path)))
}

/// The bytes of the chunk `rel_path` addresses, decoded.
fn read_chunk(archive: Cursor<Vec<u8>>, wad_name: &str, rel_path: &str) -> Vec<u8> {
    let mut reader = FantomeReader::new(archive).unwrap();
    let mut wad = reader
        .mount_packed_wad(wad_name)
        .unwrap()
        .expect("a packed WAD");
    let chunk = *wad
        .chunks()
        .get(ltk_wad::chunk_hash_of(Utf8Path::new(rel_path)))
        .expect("the chunk");
    wad.load_chunk_decompressed(&chunk).unwrap().into_vec()
}

// -- packing tests ----------------------------------------------------------

#[test]
fn pack_writes_license_file_and_field() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);
    std::fs::write(root.join("LICENSE.md"), "The terms.").unwrap();

    let project = test_project(Some(ModProjectLicense::Spdx("MIT".to_string())));
    let buffer = pack(&project, &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();

    // The source file's name is preserved in the entry name.
    let mut license = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("META/LICENSE.md").unwrap(),
        &mut license,
    )
    .unwrap();
    assert_eq!(license, "The terms.");

    let mut info_content = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("META/info.json").unwrap(),
        &mut info_content,
    )
    .unwrap();
    let info: FantomeInfo = serde_json::from_str(&info_content).unwrap();
    assert_eq!(info.license, Some(FantomeLicense::Spdx("MIT".to_string())));
}

#[test]
fn pack_omits_license_entry_when_project_has_none() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();
    assert!(archive.by_name("META/LICENSE").is_err());
}

#[test]
fn license_survives_project_fantome_project_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp).join("project");
    write_project_tree(&root);
    std::fs::write(root.join("LICENSE.txt"), "Round trip terms.").unwrap();

    let project = test_project(Some(ModProjectLicense::Custom {
        name: "My License".to_string(),
        url: None,
    }));

    let buffer = pack(&project, &root);

    let extracted = utf8_dir(&tmp).join("extracted");
    let imported = ProjectImporter::new(&extracted)
        .import(FantomeImporter::new(buffer))
        .unwrap();

    assert_eq!(
        imported.license,
        Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: None,
        })
    );

    // The file comes back under the name it went in with.
    assert_eq!(
        std::fs::read_to_string(extracted.join("LICENSE.txt")).unwrap(),
        "Round trip terms."
    );
}

#[test]
fn pack_canonicalizes_license_entry_name() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);
    std::fs::write(root.join("license.txt"), "The terms.").unwrap();

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();

    // A lowercase source name is written under its canonical spelling, so
    // repacking an extracted project is stable rather than case-drifting.
    assert!(archive.by_name("META/LICENSE.txt").is_ok());
}

#[test]
fn pack_skips_modignored_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::write(wad_dir.join("source.psd"), b"working file").unwrap();
    std::fs::write(root.join(".modignore"), "*.psd\n").unwrap();

    let buffer = pack(&test_project(None), &root);

    // The rest of the WAD directory is packed as before.
    assert!(holds_chunk(buffer.clone(), "Test.wad.client", "data.bin"));
    assert!(!holds_chunk(buffer, "Test.wad.client", "source.psd"));
}

#[test]
fn pack_detects_wad_directories_case_insensitively() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let wad_dir = root.join("content").join("base").join("Upper.WAD.Client");
    std::fs::create_dir_all(&wad_dir).unwrap();
    std::fs::write(wad_dir.join("data.bin"), b"data").unwrap();

    let buffer = pack(&test_project(None), &root);

    // The entry keeps the author's spelling; only detection is folded.
    let mut archive = zip::ZipArchive::new(buffer.clone()).unwrap();
    assert!(archive.by_name("WAD/Upper.WAD.Client").is_ok());
    assert!(holds_chunk(buffer, "Upper.WAD.Client", "data.bin"));
}

#[test]
fn pack_applies_nested_modignore_and_never_archives_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::write(wad_dir.join("source.psd"), b"working file").unwrap();
    std::fs::write(wad_dir.join(".modignore"), "*.psd\n").unwrap();

    let buffer = pack(&test_project(None), &root);

    assert!(holds_chunk(buffer.clone(), "Test.wad.client", "data.bin"));
    assert!(!holds_chunk(
        buffer.clone(),
        "Test.wad.client",
        "source.psd"
    ));
    assert!(
        !holds_chunk(buffer, "Test.wad.client", ".modignore"),
        "filter metadata leaked into the archive"
    );
}

/// Each WAD directory becomes one built WAD, stored as a single archive entry.
///
/// That is the shape distributed mods overwhelmingly have, the shape a reader
/// can seek a chunk out of without inflating anything, and the shape a repair
/// can rewrite the tail of instead of repacking the mod.
#[test]
fn pack_writes_each_wad_directory_as_one_stored_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let zed = root.join("content").join("base").join("Zed.wad.client");
    std::fs::create_dir_all(&zed).unwrap();
    std::fs::write(zed.join("data.bin"), b"zed content").unwrap();

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer.clone()).unwrap();
    for name in ["WAD/Test.wad.client", "WAD/Zed.wad.client"] {
        let entry = archive.by_name(name).unwrap_or_else(|_| panic!("{name}"));
        assert_eq!(
            entry.compression(),
            zip::CompressionMethod::Stored,
            "{name} must be stored so a reader can seek into it"
        );
    }
    assert!(
        archive
            .file_names()
            .all(|name| !name.starts_with("WAD/Test.wad.client/")),
        "the WAD's files leaked in as loose entries as well"
    );

    // And the content is in there, addressed by the hash of its path.
    assert_eq!(
        read_chunk(buffer.clone(), "Test.wad.client", "data.bin"),
        b"content"
    );
    assert_eq!(
        read_chunk(buffer, "Zed.wad.client", "data.bin"),
        b"zed content"
    );
}

/// The WADs are the last entries, in name order.
///
/// A WAD that is one entry at the end can later be grown in place, with only
/// the central directory behind it to move - the same shape `ltk_fantome`'s
/// normalize and rewrite put an archive into.
#[test]
fn pack_writes_the_wads_last_in_name_order() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    for name in ["Zed.wad.client", "Ahri.wad.client"] {
        let dir = root.join("content").join("base").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("data.bin"), b"content").unwrap();
    }

    let buffer = pack(&test_project(None), &root);

    let archive = zip::ZipArchive::new(buffer).unwrap();
    let names: Vec<&str> = archive.file_names().collect();
    let wads: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| name.starts_with("WAD/"))
        .collect();
    assert_eq!(
        wads,
        [
            "WAD/Ahri.wad.client",
            "WAD/Test.wad.client",
            "WAD/Zed.wad.client"
        ]
    );
    assert_eq!(
        &names[names.len() - 3..],
        wads.as_slice(),
        "the WADs must be the last entries: {names:?}"
    );
}

/// A chunk keeps the codec its content asks for: audio uncompressed, because it
/// is already compressed, and everything else Zstd.
///
/// The same policy `ltk_wad` holds and the overlay builder applies, so a mod
/// packed here and the same content built into an overlay agree.
#[test]
fn pack_stores_audio_uncompressed_and_compresses_the_rest() {
    use ltk_wad::WadChunkCompression;

    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::create_dir_all(&wad_dir).unwrap();

    // `BKHD` is the Wwise bank magic; `PROP` a property bin's.
    std::fs::write(wad_dir.join("sound.bnk"), b"BKHD and then some audio").unwrap();
    std::fs::write(wad_dir.join("data.bin"), b"PROP and then some properties").unwrap();

    let buffer = pack(&test_project(None), &root);

    let mut reader = FantomeReader::new(buffer).unwrap();
    let wad = reader.mount_packed_wad("Test.wad.client").unwrap().unwrap();
    let codec = |rel: &str| {
        wad.chunks()
            .get(ltk_wad::chunk_hash_of(Utf8Path::new(rel)))
            .unwrap()
            .compression_type
    };
    assert_eq!(codec("sound.bnk"), WadChunkCompression::None);
    assert_eq!(codec("data.bin"), WadChunkCompression::Zstd);
}

/// A file too short to carry any magic packs like anything else.
///
/// `ltk_file` 0.2.11 panics on a buffer of exactly three bytes, and a mod may
/// hold such a file, so the packer bounds what the identification is handed.
#[test]
fn pack_handles_a_file_too_short_to_identify() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::create_dir_all(&wad_dir).unwrap();
    std::fs::write(wad_dir.join("tiny.bin"), b"one").unwrap();
    std::fs::write(wad_dir.join("empty.bin"), b"").unwrap();

    let buffer = pack(&test_project(None), &root);

    assert_eq!(
        read_chunk(buffer.clone(), "Test.wad.client", "tiny.bin"),
        b"one"
    );
    assert_eq!(read_chunk(buffer, "Test.wad.client", "empty.bin"), b"");
}

/// Two files of one WAD that address the same chunk fail the pack.
///
/// The `.ltk` suffix a lossless extraction adds to a path two chunks claimed
/// hashes back to the path without it, so extracting and repacking is exactly
/// where this arises. Refused rather than dropping one silently.
#[test]
fn pack_refuses_two_files_that_are_the_same_chunk() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::create_dir_all(&wad_dir).unwrap();
    std::fs::write(wad_dir.join("data.bin"), b"the chunk that claimed the path").unwrap();
    std::fs::write(wad_dir.join("data.bin.ltk"), b"the chunk that was renamed").unwrap();

    let error = try_pack(&test_project(None), &root).unwrap_err();

    assert!(
        matches!(
            &error,
            PackError::Format(FantomePackError::ChunkCollision { wad, .. })
                if wad == "Test.wad.client"
        ),
        "expected a chunk collision, got {error:?}"
    );
}

/// The round trip a repack is: pack, import, and the files come back under the
/// names the author gave them.
///
/// A packed WAD keys its chunks by hash and carries no paths, so this only
/// holds because the pack harvests the project's own chunk paths into a table
/// the import then resolves through. Without it every file here would come back
/// as sixteen hex digits.
#[test]
fn a_packed_wad_imports_back_to_the_files_it_was_built_from() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::create_dir_all(wad_dir.join("assets")).unwrap();
    std::fs::write(wad_dir.join("assets/thing.bin"), b"PROP the asset").unwrap();
    std::fs::write(wad_dir.join("data.bin"), b"PROP the data").unwrap();

    let packed = pack(&test_project(None), &root).into_inner();

    let out = utf8_dir(&tmp).join("reimported");
    import(packed, &out).unwrap();

    let base = out.join("content").join("base").join("Test.wad.client");
    let mut landed = Vec::new();
    collect_files(&base, &base, &mut landed);
    landed.sort();
    assert_eq!(landed, ["assets/thing.bin", "data.bin"]);

    assert_eq!(
        std::fs::read(base.join("assets/thing.bin").as_std_path()).unwrap(),
        b"PROP the asset"
    );
    assert_eq!(
        std::fs::read(base.join("data.bin").as_std_path()).unwrap(),
        b"PROP the data"
    );
}

/// The chain the packed shape exists for: pack, repair a chunk without
/// repacking, and read the mod back.
///
/// A repair changes a handful of files in an archive that may be hundreds of
/// megabytes. `ltk_fantome`'s delta rewrites that chunk into its WAD's tail
/// and copies the rest, which only works because the pack put a WAD there to
/// rebase - a mod packed as loose entries has nothing to rewrite and falls
/// back to a full repack forever.
#[test]
fn a_packed_mod_is_repaired_in_place_and_still_reads_back() {
    use ltk_fantome::{apply_delta, ArchiveDelta};

    const REPAIRED: &[u8] = b"PROP the repaired data";

    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::create_dir_all(&wad_dir).unwrap();
    std::fs::write(wad_dir.join("data.bin"), b"PROP the stale data").unwrap();
    std::fs::write(wad_dir.join("other.bin"), b"PROP the untouched data").unwrap();

    let archive_path = root.join("mod.fantome");
    std::fs::write(
        archive_path.as_std_path(),
        pack(&test_project(None), &root).into_inner(),
    )
    .unwrap();
    let before = std::fs::metadata(archive_path.as_std_path()).unwrap().len();

    // What a repair holds is the file it fixed, named by the path the chunk
    // was extracted under; `chunk_hash_of` turns that back into the chunk.
    let mut delta = ArchiveDelta::new();
    delta.chunk(
        "Test.wad.client",
        ltk_wad::chunk_hash_of(Utf8Path::new("data.bin")),
        REPAIRED,
    );
    let report = apply_delta(&archive_path, &archive_path, &delta, None).unwrap();
    assert_eq!(report.wads_rebased, 1);
    assert_eq!(report.chunks_replaced, 1);

    // Still one packed WAD a reader can seek into, not a directory of loose
    // files - which is what a repack would have left behind.
    let repaired = Cursor::new(std::fs::read(archive_path.as_std_path()).unwrap());
    let mut reader = FantomeReader::new(repaired.clone()).unwrap();
    assert!(reader
        .packed_wad_source("Test.wad.client")
        .unwrap()
        .unwrap()
        .is_in_place());

    assert_eq!(
        read_chunk(repaired.clone(), "Test.wad.client", "data.bin"),
        REPAIRED
    );
    assert_eq!(
        read_chunk(repaired, "Test.wad.client", "other.bin"),
        b"PROP the untouched data"
    );

    // The repair cost the changed chunk, not a rebuild: the archive grew by the
    // tail it appended rather than doubling as a loose repack would.
    let after = std::fs::metadata(archive_path.as_std_path()).unwrap().len();
    assert!(
        after < before * 2,
        "the repaired archive grew from {before} to {after} bytes"
    );

    // And it imports back to a project the way any archive does.
    let out = utf8_dir(&tmp).join("reimported");
    import(std::fs::read(archive_path.as_std_path()).unwrap(), &out).unwrap();
    let base = out.join("content").join("base").join("Test.wad.client");
    let mut bodies: Vec<Vec<u8>> = std::fs::read_dir(base.as_std_path())
        .unwrap()
        .map(|entry| std::fs::read(entry.unwrap().path()).unwrap())
        .collect();
    bodies.sort();
    assert_eq!(
        bodies,
        vec![REPAIRED.to_vec(), b"PROP the untouched data".to_vec()]
    );
}

/// Every file of every WAD is still reported exactly once.
///
/// The reports moved inside the chunk-data provider so each one lands when its
/// file is actually read rather than all of them landing before any work; this
/// is the guard that the move did not drop or double any.
#[test]
fn pack_reports_every_file_of_every_wad_once() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let zed = root.join("content").join("base").join("Zed.wad.client");
    std::fs::create_dir_all(&zed).unwrap();
    std::fs::write(zed.join("one.bin"), b"PROP one").unwrap();
    std::fs::write(zed.join("two.bin"), b"PROP two").unwrap();

    let mut written: Vec<String> = Vec::new();
    let mut totals: Vec<(u32, u32)> = Vec::new();
    ProjectPacker::new(test_project(None), root.clone())
        .pack_with_progress(
            FantomeFormat::new(&mut Cursor::new(Vec::new())),
            &mut |progress: crate::PackProgress<'_>| {
                if progress.stage == crate::PackStage::Writing {
                    written.push(progress.current_item.unwrap().to_owned());
                    totals.push((progress.current, progress.total));
                }
            },
        )
        .unwrap();

    written.sort();
    assert_eq!(written, ["data.bin", "one.bin", "two.bin"]);
    assert_eq!(
        totals
            .iter()
            .map(|(current, _)| *current)
            .collect::<Vec<_>>(),
        [0, 1, 2],
        "the counter must run once through the plan's files"
    );
    assert!(totals.iter().all(|(_, total)| *total == 3), "{totals:?}");
}

/// A name the project's own declared table already resolves is left to it.
///
/// The harvest adds and never repeats, so a project that declares a table
/// covering its chunks packs exactly as it did before harvesting existed.
#[test]
fn pack_harvests_nothing_a_declared_table_already_names() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    // `write_project_tree` puts one file, `data.bin`, in the WAD directory.
    std::fs::create_dir_all(root.join(crate::HASHES_DIR_NAME)).unwrap();
    std::fs::write(
        root.join(crate::HASHES_DIR_NAME).join("game.hashes.txt"),
        "data.bin\n",
    )
    .unwrap();
    let project = ModProject {
        hashtables: vec![ModProjectHashtable {
            path: "hashes/game.hashes.txt".to_owned(),
            category: Category::Game,
            algorithm: ltk_hashtable::Algorithm::Xxh64,
            bits: 64,
        }],
        ..test_project(None)
    };

    let buffer = pack(&project, &root);

    let mut reader = FantomeReader::new(buffer).unwrap();
    let declared: Vec<String> = reader
        .read_info()
        .unwrap()
        .hashtables
        .iter()
        .map(|manifest| manifest.path.clone())
        .collect();
    assert_eq!(declared, ["META/hashes/game.hashes.txt"]);
}

/// A file with nothing but a hash for a name contributes nothing to harvest.
///
/// It came out of an extraction that could not name it either, so a table
/// recording it would map a hash to the same hash.
#[test]
fn pack_harvests_no_table_for_a_project_of_bare_hashes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    let wad_dir = root.join("content").join("base").join("Test.wad.client");
    std::fs::create_dir_all(&wad_dir).unwrap();
    std::fs::write(wad_dir.join("0123456789abcdef"), b"PROP nameless").unwrap();

    let buffer = pack(&test_project(None), &root);

    let mut reader = FantomeReader::new(buffer).unwrap();
    assert!(
        reader.read_info().unwrap().hashtables.is_empty(),
        "a project with no names to record must declare no table"
    );
}

/// The harvested table never takes the conventional `game.hashes.txt` name.
///
/// That name belongs to a table the author declared, and an archive where it
/// sometimes means one and sometimes the other cannot be read confidently.
#[test]
fn the_harvested_table_never_masquerades_as_a_declared_one() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer.clone()).unwrap();
    assert!(archive.by_name("META/hashes/game.hashes.txt").is_err());
    assert!(archive
        .by_name("META/hashes/game.harvested.hashes.txt")
        .is_ok());

    let mut reader = FantomeReader::new(buffer).unwrap();
    let tables = reader.read_hashtables().unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].1.names().collect::<Vec<_>>(), ["data.bin"]);
}

#[test]
fn pack_skips_content_outside_wad_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    // Loose files and plain directories are packable to modpkg but have no
    // place in a Fantome archive.
    std::fs::write(root.join("content/base/loose.bin"), b"loose").unwrap();
    let plain_dir = root.join("content/base/some_dir");
    std::fs::create_dir_all(&plain_dir).unwrap();
    std::fs::write(plain_dir.join("file.bin"), b"plain").unwrap();

    let buffer = pack(&test_project(None), &root);

    let archive = zip::ZipArchive::new(buffer).unwrap();
    let names: Vec<&str> = archive.file_names().collect();
    assert!(
        names
            .iter()
            .all(|name| !name.contains("loose.bin") && !name.contains("some_dir")),
        "non-WAD content leaked into the archive: {names:?}"
    );
}

#[test]
fn pack_drops_non_base_layers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    let hires_wad = root
        .join("content")
        .join("high-res")
        .join("Test.wad.client");
    std::fs::create_dir_all(&hires_wad).unwrap();
    std::fs::write(hires_wad.join("extra.bin"), b"extra").unwrap();

    let mut project = test_project(None);
    project.layers.push(crate::ModProjectLayer {
        name: "high-res".to_string(),
        priority: 1,
        ..Default::default()
    });

    let buffer = pack(&project, &root);

    assert!(holds_chunk(buffer.clone(), "Test.wad.client", "data.bin"));
    assert!(!holds_chunk(buffer, "Test.wad.client", "extra.bin"));
}

#[test]
fn pack_embeds_an_unconfigured_default_thumbnail() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    // A 1x1 image, saved as the default thumbnail.webp with no config entry.
    let img = image::DynamicImage::new_rgb8(1, 1);
    img.save(root.join("thumbnail.webp").as_std_path()).unwrap();

    let buffer = pack(&test_project(None), &root);

    let mut archive = zip::ZipArchive::new(buffer).unwrap();
    assert!(
        archive.by_name("META/image.png").is_ok(),
        "the default thumbnail.webp must be embedded, as it is for modpkg"
    );
}

#[test]
fn pack_reports_an_unreadable_thumbnail_with_its_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);

    // Present, so packing reaches it, but not an image.
    std::fs::write(root.join("thumbnail.webp"), b"not an image").unwrap();

    let project = ModProject {
        thumbnail: Some("thumbnail.webp".to_string()),
        ..test_project(None)
    };

    let error = try_pack(&project, &root).unwrap_err();

    match error {
        PackError::Format(FantomePackError::Thumbnail { path, .. }) => {
            assert_eq!(path, root.join("thumbnail.webp"));
        }
        other => panic!("expected Thumbnail, got {other:?}"),
    }
}

/// An error's own message must not repeat what its source says, or an error
/// chain prints the same sentence twice.
#[test]
fn pack_error_display_does_not_embed_its_source() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_dir(&tmp);
    write_project_tree(&root);
    std::fs::write(root.join("thumbnail.webp"), b"not an image").unwrap();

    let project = ModProject {
        thumbnail: Some("thumbnail.webp".to_string()),
        ..test_project(None)
    };

    let error = try_pack(&project, &root).unwrap_err();

    let source = std::error::Error::source(&error).unwrap().to_string();
    assert!(
        !error.to_string().contains(&source),
        "`{error}` already contains its source `{source}`"
    );
}

// -- import tests -----------------------------------------------------------

/// The progress reports, as owned values a test can compare.
///
/// The match is total on purpose: it is the branching a consumer has to do, and
/// a stage added later fails here rather than being folded into its neighbour.
fn describe(progress: ImportProgress<'_>) -> (String, u32, u32) {
    let stage = match progress.stage {
        ImportStage::Extracting { item } => format!("extracting {item}"),
        ImportStage::WritingMetadata => "writing metadata".to_owned(),
        ImportStage::Complete => "complete".to_owned(),
    };
    (stage, progress.current, progress.total)
}
/// Import an in-memory archive with every driver hook left at its default.
fn import(
    data: Vec<u8>,
    output_dir: &Utf8Path,
) -> Result<ModProject, ImportError<FantomeImportError>> {
    ProjectImporter::new(output_dir).import(FantomeImporter::new(Cursor::new(data)))
}

fn create_test_fantome() -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    let info = r#"{
        "Name": "Test Mod",
        "Author": "Test Author",
        "Version": "1.0.0",
        "Description": "A test mod"
    }"#;
    zip.write_all(info.as_bytes()).unwrap();

    zip.add_directory("WAD/test.wad.client", options).unwrap();
    zip.start_file("WAD/test.wad.client/assets/test.bin", options)
        .unwrap();
    zip.write_all(b"test content").unwrap();

    zip.finish().unwrap().into_inner()
}

/// Build a fantome archive whose license entry is named `license_entry`.
fn create_fantome_with_license(license_entry: &str, info: &str) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(info.as_bytes()).unwrap();

    zip.start_file(license_entry, options).unwrap();
    zip.write_all(b"The terms.").unwrap();

    zip.finish().unwrap().into_inner()
}

#[test]
fn import_materializes_a_project() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = import(create_test_fantome(), &output).unwrap();

    assert_eq!(imported.display_name, "Test Mod");
    assert_eq!(imported.name, "test-mod");
    assert_eq!(imported.version, "1.0.0");

    // Check that mod.config.json was created
    assert!(output.join("mod.config.json").exists());

    // Check that WAD content was extracted
    assert!(output
        .join("content/base/test.wad.client/assets/test.bin")
        .exists());
}

#[test]
fn import_license_entry_case_and_extension_variants() {
    let info = r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;

    for (entry, expected_file) in [
        ("META/LICENSE", "LICENSE"),
        ("META/license.md", "LICENSE.md"),
        ("meta/LICENSE.TXT", "LICENSE.txt"),
    ] {
        let data = create_fantome_with_license(entry, info);

        let temp_dir = tempfile::tempdir().unwrap();
        let output = utf8_dir(&temp_dir);
        import(data, &output).unwrap();

        let extracted = output.join(expected_file);
        assert!(
            extracted.exists(),
            "expected {expected_file} for archive entry {entry}"
        );
        assert_eq!(std::fs::read_to_string(&extracted).unwrap(), "The terms.");
    }
}

#[test]
fn import_reads_the_license_field() {
    let info = r#"{
        "Name": "Test",
        "Author": "Test",
        "Version": "1.0.0",
        "Description": "Test",
        "License": "Apache-2.0"
    }"#;
    let data = create_fantome_with_license("META/LICENSE", info);

    let temp_dir = tempfile::tempdir().unwrap();
    let imported = import(data, &utf8_dir(&temp_dir)).unwrap();

    assert_eq!(
        imported.license,
        Some(ModProjectLicense::Spdx("Apache-2.0".to_string()))
    );
}

#[test]
fn import_reads_a_custom_license_field_without_url() {
    let info = r#"{
        "Name": "Test",
        "Author": "Test",
        "Version": "1.0.0",
        "Description": "Test",
        "License": { "Name": "My License" }
    }"#;
    let data = create_fantome_with_license("META/LICENSE", info);

    let temp_dir = tempfile::tempdir().unwrap();
    let imported = import(data, &utf8_dir(&temp_dir)).unwrap();

    assert_eq!(
        imported.license,
        Some(ModProjectLicense::Custom {
            name: "My License".to_string(),
            url: None,
        })
    );
}

#[test]
fn import_of_a_legacy_fantome_has_no_license() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = import(create_test_fantome(), &output).unwrap();

    assert_eq!(imported.license, None);
    assert!(!output.join("LICENSE").exists());
}

#[test]
fn import_extracts_raw_files() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    let info = r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;
    zip.write_all(info.as_bytes()).unwrap();

    zip.add_directory("RAW", options).unwrap();
    zip.start_file("RAW/assets/characters/aatrox/skin0.bin", options)
        .unwrap();
    zip.write_all(b"aatrox data").unwrap();
    zip.start_file("RAW/assets/maps/map11/scene.bin", options)
        .unwrap();
    zip.write_all(b"map data").unwrap();

    let buffer = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);
    let imported = import(buffer, &output).unwrap();
    assert_eq!(imported.display_name, "Test");

    let raw_file1 = output.join("content/base/raw/assets/characters/aatrox/skin0.bin");
    assert!(raw_file1.exists());
    assert_eq!(std::fs::read(&raw_file1).unwrap(), b"aatrox data");

    let raw_file2 = output.join("content/base/raw/assets/maps/map11/scene.bin");
    assert!(raw_file2.exists());
    assert_eq!(std::fs::read(&raw_file2).unwrap(), b"map data");
}

/// Names every chunk the same, so one chunk lands at a path only the
/// resolver could have chosen.
struct FixedResolver;

impl PathResolver for FixedResolver {
    fn resolve(&self, _path_hash: ltk_wad::WadHash) -> Option<String> {
        Some(String::from("assets/characters/aatrox/skin0.bin"))
    }
}

fn packed_wad_bytes(payload: &[u8]) -> Vec<u8> {
    use ltk_wad::{WadBuilder, WadChunkBuilder, WadChunkCompression};

    let payload = payload.to_vec();
    let mut cursor = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(
            WadChunkBuilder::default()
                .with_path("packed/file.bin")
                .with_force_compression(WadChunkCompression::None),
        )
        .build_to_writer(&mut cursor, move |_hash, writer| {
            std::io::Write::write_all(writer, &payload)?;
            Ok(())
        })
        .unwrap();
    cursor.into_inner()
}

/// The importer hands its resolver to the unpack, so a caller naming chunks
/// from its own tables gets real paths in the project tree.
#[test]
fn import_names_packed_wad_chunks_through_the_resolver() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    let info = r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;
    zip.write_all(info.as_bytes()).unwrap();

    zip.start_file("WAD/Aatrox.wad.client", options).unwrap();
    zip.write_all(&packed_wad_bytes(b"skin bytes")).unwrap();

    let buffer = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);
    ProjectImporter::new(&output)
        .import(FantomeImporter::new(Cursor::new(buffer)).with_path_resolver(&FixedResolver))
        .unwrap();

    let skin = output.join("content/base/Aatrox.wad.client/assets/characters/aatrox/skin0.bin");
    assert_eq!(std::fs::read(&skin).unwrap(), b"skin bytes");
}

/// An archive whose `META/info.json` declares `layers`, each with one string
/// override so a dropped layer is visible as a dropped override.
fn create_fantome_with_layers(layers: &[(&str, i32)]) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let declared: Vec<String> = layers
        .iter()
        .map(|(name, priority)| {
            format!(
                r#""{name}": {{"Name": "{name}", "Priority": {priority}, "StringOverrides": {{"default": {{"key_{name}": "value"}}}}}}"#
            )
        })
        .collect();
    let info = format!(
        r#"{{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test", "Layers": {{{}}}}}"#,
        declared.join(",")
    );

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    zip.start_file("META/info.json", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(info.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

/// An archive whose `WAD/` holds `names`, each a directory of one file.
fn create_fantome_with_wads(names: &[&str]) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    let info = r#"{"Name": "Test", "Author": "Test", "Version": "1.0.0", "Description": "Test"}"#;
    zip.write_all(info.as_bytes()).unwrap();

    for name in names {
        zip.start_file(format!("WAD/{name}/data/file.bin"), options)
            .unwrap();
        zip.write_all(b"content").unwrap();
    }

    zip.finish().unwrap().into_inner()
}

/// Fantome stores content for the base layer alone, but the string overrides
/// on its other layers are metadata nothing downstream can recover.
#[test]
fn import_keeps_the_layers_the_archive_declares() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = import(create_fantome_with_layers(&[("skins", 10)]), &output).unwrap();

    let names: Vec<&str> = imported.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["base", "skins"], "base is added, skins is kept");

    let skins = &imported.layers[1];
    assert_eq!(skins.priority, 10);
    assert_eq!(
        skins.string_overrides["default"]["key_skins"], "value",
        "the overrides came across with the layer"
    );
}

/// `META/info.json` stores layers as a map, so only a sort makes two imports of
/// one archive agree.
#[test]
fn import_orders_layers_base_first_then_by_priority_then_by_name() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let archive = create_fantome_with_layers(&[("zed", 5), ("aatrox", 5), ("late", 20)]);
    let imported = import(archive, &output).unwrap();

    let names: Vec<&str> = imported.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["base", "aatrox", "zed", "late"]);
}

#[test]
fn import_of_an_archive_declaring_no_layers_gets_the_default_base() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = import(create_test_fantome(), &output).unwrap();

    assert_eq!(imported.layers, ModProjectLayer::default_table());
}

#[test]
fn import_reports_a_stage_for_each_wad_then_one_for_each_step_past_them() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let mut reported = Vec::new();
    let archive = create_fantome_with_wads(&["Zed.wad.client", "Aatrox.wad.client"]);
    ProjectImporter::new(&output)
        .import_with_progress(
            FantomeImporter::new(Cursor::new(archive)),
            &mut |progress| reported.push(describe(progress)),
        )
        .unwrap();

    assert_eq!(
        reported,
        [
            ("extracting Zed.wad.client".to_owned(), 0, 2),
            ("extracting Aatrox.wad.client".to_owned(), 1, 2),
            // No `RAW/` entries, so no `RAW/` pass and nothing counted for one.
            ("writing metadata".to_owned(), 2, 2),
            ("complete".to_owned(), 2, 2),
        ]
    );
}

#[test]
fn import_without_a_progress_callback_still_imports() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = ProjectImporter::new(&output)
        .import(FantomeImporter::new(Cursor::new(create_test_fantome())))
        .unwrap();

    assert_eq!(imported.name, "test-mod");
}

/// The config is written once, so what `with_config` sets is what the file on
/// disk says as well as what the call returns.
#[test]
fn with_config_names_the_project_and_the_written_config_agrees() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let imported = ProjectImporter::new(&output)
        .with_config(|project| {
            project.name = "chosen-slug".to_owned();
            project.display_name = "Chosen Name".to_owned();
        })
        .import(FantomeImporter::new(Cursor::new(create_test_fantome())))
        .unwrap();

    assert_eq!(imported.name, "chosen-slug");
    assert_eq!(imported.display_name, "Chosen Name");

    let written = ModProject::load(&output).unwrap();
    assert_eq!(written, imported);
}

#[test]
fn a_cancellation_that_answers_true_fails_the_import() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let cancelled = || true;
    let result = ProjectImporter::new(&output)
        .with_cancellation(Cancellation::predicate(&cancelled))
        .import(FantomeImporter::new(Cursor::new(create_test_fantome())));

    assert!(matches!(result, Err(ImportError::Cancelled)));
    assert!(
        !output.join("mod.config.json").exists(),
        "the config is the last thing written, so a cancelled import has none"
    );
}

#[test]
fn a_cancellation_that_answers_false_imports_as_normal() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let flag = AtomicBool::new(false);
    let imported = ProjectImporter::new(&output)
        .with_cancellation(&flag)
        .import(FantomeImporter::new(Cursor::new(create_test_fantome())))
        .unwrap();

    assert_eq!(imported.name, "test-mod");
}

/// An archive can hold nothing but metadata, and the project it becomes still
/// has to be one the packer accepts.
#[test]
fn import_of_a_metadata_only_archive_still_has_a_base_layer_directory() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    zip.start_file("META/info.json", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(br#"{"Name": "Bare", "Author": "A", "Version": "1.0.0", "Description": "d"}"#)
        .unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);
    import(archive, &output).unwrap();

    assert!(output.join("content/base").is_dir());
    ProjectPacker::from_dir(output)
        .unwrap()
        .pack(FantomeFormat::new(Cursor::new(Vec::new())))
        .unwrap();
}

/// An archive can declare a layer it holds no content for - Fantome stores
/// content for the base layer alone - and the project it becomes still has to be
/// one the packer accepts.
#[test]
fn import_of_an_archive_declaring_a_layer_gives_that_layer_a_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir).join("project");

    import(create_fantome_with_layers(&[("skins", 10)]), &output).unwrap();

    assert!(output.join("content/skins").is_dir());
    ProjectPacker::from_dir(output)
        .unwrap()
        .pack(FantomeFormat::new(Cursor::new(Vec::new())))
        .unwrap();
}

/// The attack this guards against: a `WAD/` entry that climbs out of the
/// output directory and lands beside it. The archive is refused whole, so
/// nothing is written - neither the escaping file nor the mod's own content.
#[test]
fn import_refuses_an_archive_whose_entry_escapes_the_output_directory() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name":"Evil","Author":"A","Version":"1.0.0","Description":""}"#)
        .unwrap();
    zip.start_file("WAD/test.wad.client/assets/test.bin", options)
        .unwrap();
    zip.write_all(b"test content").unwrap();
    zip.start_file("WAD/../../pwned.txt", options).unwrap();
    zip.write_all(b"pwned").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let root = utf8_dir(&temp_dir);
    let output_dir = root.join("nested").join("project");

    let error = import(archive, &output_dir).unwrap_err();

    assert!(
        matches!(
            &error,
            ImportError::Format(FantomeImportError::Extract(
                ltk_fantome::FantomeExtractError::EscapingEntry { .. }
            ))
        ),
        "{error:?}"
    );
    assert!(
        !root.join("pwned.txt").exists(),
        "the escaping entry was written outside the output directory"
    );
    assert!(
        !output_dir
            .join("content")
            .join("base")
            .join("test.wad.client")
            .exists(),
        "the archive's own content was extracted despite the refusal"
    );
}

/// The `RAW/` pass is a unit of the extraction like a WAD is, so it is named,
/// counted, and inside the total. Leaving it out filled the bar before the pass
/// a raw-heavy mod spends most of its import in.
#[test]
fn the_raw_pass_is_a_counted_unit_of_the_extraction() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name": "T", "Author": "A", "Version": "1.0.0", "Description": "d"}"#)
        .unwrap();
    zip.start_file("WAD/Zed.wad.client/data/file.bin", options)
        .unwrap();
    zip.write_all(b"content").unwrap();
    zip.start_file("RAW/assets/loose.bin", options).unwrap();
    zip.write_all(b"loose").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let mut reported = Vec::new();
    ProjectImporter::new(&output)
        .import_with_progress(
            FantomeImporter::new(Cursor::new(archive)),
            &mut |progress| reported.push(describe(progress)),
        )
        .unwrap();

    assert_eq!(
        reported,
        [
            ("extracting Zed.wad.client".to_owned(), 0, 2),
            ("extracting RAW".to_owned(), 1, 2),
            ("writing metadata".to_owned(), 2, 2),
            ("complete".to_owned(), 2, 2),
        ]
    );
    assert!(output.join("content/base/raw/assets/loose.bin").is_file());
}

/// An unpacked `.wad.client` directory under `WAD/`, which is how an archive
/// ships a WAD without packing it. Real tools write an explicit zip directory
/// record for the folder and one for each subdirectory, so the fixture does
/// too: those records name no file, and the tree has to come out of the file
/// entries alone.
#[test]
fn a_wad_shipped_as_a_folder_imports_with_its_tree_intact() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name": "T", "Author": "A", "Version": "1.0.0", "Description": "d"}"#)
        .unwrap();

    zip.add_directory("WAD", options).unwrap();
    zip.add_directory("WAD/Aatrox.wad.client", options).unwrap();
    zip.add_directory("WAD/Aatrox.wad.client/assets", options)
        .unwrap();
    zip.add_directory("WAD/Aatrox.wad.client/assets/characters", options)
        .unwrap();

    let files = [
        ("WAD/Aatrox.wad.client/assets/characters/skin0.bin", "one"),
        ("WAD/Aatrox.wad.client/assets/characters/skin1.bin", "two"),
        ("WAD/Aatrox.wad.client/data/aatrox.bin", "three"),
        ("WAD/Zed.wad.client/data/zed.bin", "four"),
    ];
    for (name, body) in files {
        zip.start_file(name, options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    let archive = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir).join("project");

    let imported = import(archive, &output).unwrap();

    let base = output.join("content").join("base");
    for (name, body) in files {
        let landed = base.join(name.strip_prefix("WAD/").unwrap());
        assert_eq!(
            std::fs::read_to_string(&landed).unwrap(),
            body,
            "{name} did not land at {landed}"
        );
    }

    // Both folder WADs are directories of the project, so the packer reads the
    // WAD each file belongs to back out of the tree.
    assert!(base.join("Aatrox.wad.client").is_dir());
    assert!(base.join("Zed.wad.client").is_dir());

    ProjectPacker::new(imported, output)
        .pack(FantomeFormat::new(&mut Cursor::new(Vec::new())))
        .unwrap();
}

/// Both spellings of a WAD arrive the same way, which is what makes a folder a
/// drop-in for a packed file.
#[test]
fn a_folder_wad_and_a_packed_wad_are_listed_and_reported_alike() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name": "T", "Author": "A", "Version": "1.0.0", "Description": "d"}"#)
        .unwrap();

    // A folder WAD, directory record and all.
    zip.add_directory("WAD/Folder.wad.client", options).unwrap();
    zip.start_file("WAD/Folder.wad.client/data/x.bin", options)
        .unwrap();
    zip.write_all(b"folder").unwrap();

    // A packed WAD, stored as one entry.
    zip.start_file("WAD/Packed.wad.client", options).unwrap();
    zip.write_all(&packed_wad_bytes(b"packed")).unwrap();

    let archive = zip.finish().unwrap().into_inner();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);

    let mut reported = Vec::new();
    ProjectImporter::new(&output)
        .import_with_progress(
            FantomeImporter::new(Cursor::new(archive)),
            &mut |progress| reported.push(describe(progress)),
        )
        .unwrap();

    assert_eq!(
        reported,
        [
            ("extracting Folder.wad.client".to_owned(), 0, 2),
            ("extracting Packed.wad.client".to_owned(), 1, 2),
            ("writing metadata".to_owned(), 2, 2),
            ("complete".to_owned(), 2, 2),
        ],
        "a folder WAD counts as one unit, exactly as a packed one does"
    );

    let base = output.join("content").join("base");
    assert!(base.join("Folder.wad.client/data/x.bin").is_file());
    assert!(base.join("Packed.wad.client").is_dir());
}

// -- where an import puts things -------------------------------------------

/// An archive holding every kind of entry an import places, so the preflight is
/// checked against a tree with a `RAW/` file and root files in it as well as
/// WAD content.
fn create_fantome_with_every_kind_of_entry() -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();

    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name":"Test Mod","Author":"A","Version":"1.0.0","Description":"d"}"#)
        .unwrap();

    zip.start_file("META/README.md", options).unwrap();
    zip.write_all(b"# Test Mod\n").unwrap();

    zip.start_file("META/LICENSE.txt", options).unwrap();
    zip.write_all(b"The terms.").unwrap();

    zip.add_directory("WAD/test.wad.client", options).unwrap();
    zip.start_file("WAD/test.wad.client/assets/test.bin", options)
        .unwrap();
    zip.write_all(b"test content").unwrap();

    zip.start_file("RAW/assets/loose.bin", options).unwrap();
    zip.write_all(b"raw content").unwrap();

    zip.finish().unwrap().into_inner()
}

/// A preflight is only worth having if it agrees with the import. The two are
/// separate statements here - the importer writes through `extract_wads` and
/// `extract_raw`, the preflight reads the entry names - so nothing but this
/// holds them together.
#[test]
fn the_predicted_paths_match_what_an_import_writes() {
    let archive = create_fantome_with_every_kind_of_entry();

    let reader = FantomeReader::new(Cursor::new(archive.clone())).unwrap();
    let mut predicted: Vec<Utf8PathBuf> = reader
        .iter_project_paths()
        .map(|path| {
            assert!(
                !path.is_unpacked_wad(),
                "this archive holds no packed WAD, got {path}"
            );
            path.into_path()
        })
        .collect();
    predicted.sort();

    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);
    import(archive, &output).unwrap();

    for path in &predicted {
        assert!(
            output.join(path).is_file(),
            "{path} was predicted but not written"
        );
    }

    // And nothing was written that was not predicted, config aside: the config
    // is the driver's, not the archive's.
    let mut written = Vec::new();
    collect_files(&output, &output, &mut written);
    written.retain(|path| path != "mod.config.json");
    written.sort();

    assert_eq!(written, predicted);
}

/// A packed WAD is a directory the import unpacks into, and the answer says so
/// rather than naming a file that never lands.
#[test]
fn a_packed_wad_is_predicted_as_a_directory_the_import_unpacks_into() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();
    zip.start_file("META/info.json", options).unwrap();
    zip.write_all(br#"{"Name":"M","Author":"A","Version":"1.0.0","Description":"d"}"#)
        .unwrap();
    zip.start_file("WAD/test.wad.client", options).unwrap();
    zip.write_all(&packed_wad_bytes(b"payload")).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let reader = FantomeReader::new(Cursor::new(archive.clone())).unwrap();
    let predicted: Vec<ProjectPath> = reader.iter_project_paths().collect();

    assert_eq!(
        predicted,
        [ProjectPath::unpacked_wad("content/base/test.wad.client")]
    );

    // And the import does unpack into it, rather than writing a file there.
    let temp_dir = tempfile::tempdir().unwrap();
    let output = utf8_dir(&temp_dir);
    import(archive, &output).unwrap();

    assert!(output.join("content/base/test.wad.client").is_dir());
}

fn collect_files(root: &Utf8Path, dir: &Utf8Path, into: &mut Vec<Utf8PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = Utf8PathBuf::from_path_buf(entry.unwrap().path()).unwrap();
        if path.is_dir() {
            collect_files(root, &path, into);
        } else {
            into.push(path.strip_prefix(root).unwrap().to_owned());
        }
    }
}

// -- hashtables --------------------------------------------------------------

mod hashtables {
    use ltk_hashtable::{Algorithm, Category};

    use super::*;
    use crate::{ModProjectHashtable, HASHES_DIR_NAME};

    fn game_manifest() -> ModProjectHashtable {
        ModProjectHashtable {
            path: format!("{HASHES_DIR_NAME}/game.hashes.txt"),
            category: Category::Game,
            algorithm: Algorithm::Xxh64,
            bits: 64,
        }
    }

    /// A project tree declaring one `game` table holding `names`.
    fn project_with_table(root: &Utf8Path, names: &str) -> ModProject {
        write_project_tree(root);
        std::fs::create_dir_all(root.join(HASHES_DIR_NAME)).unwrap();
        std::fs::write(root.join(&game_manifest().path), names).unwrap();

        ModProject {
            hashtables: vec![game_manifest()],
            ..test_project(None)
        }
    }

    #[test]
    fn pack_writes_the_declared_table_under_meta_hashes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        let project = project_with_table(&root, "ASSETS/Custom/One.tex\nassets/custom/two.tex\n");

        let buffer = pack(&project, &root);
        let mut archive = zip::ZipArchive::new(buffer).unwrap();

        let mut table = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("META/hashes/game.hashes.txt").unwrap(),
            &mut table,
        )
        .unwrap();
        assert_eq!(table, "ASSETS/Custom/One.tex\nassets/custom/two.tex\n");

        let mut info_content = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("META/info.json").unwrap(),
            &mut info_content,
        )
        .unwrap();
        let info: FantomeInfo = serde_json::from_str(&info_content).unwrap();
        assert_eq!(info.hashtables[0].path, "META/hashes/game.hashes.txt");
        assert_eq!(info.hashtables[0].category, Category::Game);
        assert_eq!(info.hashtables[0].bits, 64);

        // The pack harvests its own chunk paths beside the declared table, so
        // the WAD it packed can be named again on the way back out.
        assert_eq!(
            info.hashtables[1].path,
            "META/hashes/game.harvested.hashes.txt"
        );
        assert_eq!(info.hashtables.len(), 2);
    }

    /// `hashes/` is outside `content/`, so the table file must never appear
    /// as content - and `.modignore` must never touch it, even a rule that
    /// ignores everything.
    #[test]
    fn the_table_is_not_content_and_never_meets_modignore() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        let project = project_with_table(&root, "assets/custom/one.tex\n");
        std::fs::write(root.join("content/.modignore"), "*\n").unwrap();

        let buffer = pack(&project, &root);
        let mut archive = zip::ZipArchive::new(buffer).unwrap();

        assert!(archive.by_name("META/hashes/game.hashes.txt").is_ok());
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_owned())
            .collect();
        assert!(
            !names.iter().any(|name| name.starts_with("WAD/")),
            "the ignore-everything rule should have dropped all content: {names:?}"
        );
    }

    /// A file under `hashes/` no manifest entry declares does not exist as
    /// far as a pack is concerned.
    #[test]
    fn an_undeclared_table_file_is_not_packed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);
        std::fs::create_dir_all(root.join(HASHES_DIR_NAME)).unwrap();
        std::fs::write(
            root.join(HASHES_DIR_NAME).join("game.hashes.txt"),
            "assets/custom/one.tex\n",
        )
        .unwrap();

        let buffer = pack(&test_project(None), &root);
        let mut archive = zip::ZipArchive::new(buffer).unwrap();
        assert!(archive.by_name("META/hashes/game.hashes.txt").is_err());
    }

    #[test]
    fn pack_fails_on_a_missing_table_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);

        let project = ModProject {
            hashtables: vec![game_manifest()],
            ..test_project(None)
        };

        match try_pack(&project, &root) {
            Err(PackError::Hashtable { path, .. }) => {
                assert_eq!(path, root.join(&game_manifest().path));
            }
            other => panic!("expected Hashtable, got {other:?}"),
        }
    }

    #[test]
    fn pack_fails_on_an_impossible_key_width() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        let mut project = project_with_table(&root, "assets/custom/one.tex\n");
        project.hashtables[0].bits = 0;

        match try_pack(&project, &root) {
            Err(PackError::HashtableWidth { path, bits }) => {
                assert_eq!(path, game_manifest().path);
                assert_eq!(bits, 0);
            }
            other => panic!("expected HashtableWidth, got {other:?}"),
        }
    }

    /// fnv1a_32("data/strings/name_1") and fnv1a_32("data/strings/name_50")
    /// share their low 8 bits, so an 8-bit table holding both names makes one
    /// of them unresolvable forever - which only the author can fix.
    #[test]
    fn pack_fails_on_a_key_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        let mut project = project_with_table(&root, "data/strings/name_1\ndata/strings/name_50\n");
        project.hashtables[0].algorithm = Algorithm::Fnv1a32;
        project.hashtables[0].bits = 8;

        match try_pack(&project, &root) {
            Err(PackError::HashtableCollision(collision)) => {
                assert_eq!(collision.first, "data/strings/name_1");
                assert_eq!(collision.second, "data/strings/name_50");
            }
            other => panic!("expected HashtableCollision, got {other:?}"),
        }
    }

    /// User story 15: a round trip through an archive loses nothing - not
    /// the names, not their casing, not the manifest.
    #[test]
    fn the_table_survives_project_archive_project_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp).join("project");
        let project = project_with_table(&root, "ASSETS/Custom/One.tex\nassets/custom/two.tex\n");

        let buffer = pack(&project, &root);

        let extracted = utf8_dir(&tmp).join("extracted");
        let imported = ProjectImporter::new(&extracted)
            .import(FantomeImporter::new(buffer))
            .unwrap();

        assert_eq!(imported.hashtables[0], game_manifest());
        assert_eq!(
            std::fs::read_to_string(extracted.join(&game_manifest().path)).unwrap(),
            "ASSETS/Custom/One.tex\nassets/custom/two.tex\n"
        );

        // The harvested table comes back too, naming the WAD file the project
        // holds - which is what keeps that file's name across the round trip
        // now that the archive stores it as a hash-keyed chunk.
        assert_eq!(imported.hashtables.len(), 2);
        assert_eq!(
            imported.hashtables[1].path,
            "hashes/game.harvested.hashes.txt"
        );
        assert_eq!(
            std::fs::read_to_string(extracted.join("hashes/game.harvested.hashes.txt")).unwrap(),
            "data.bin\n"
        );
    }

    /// An archive declaring a table, as the preserve writes one.
    fn fantome_with_table(names: &str, packed_wad: Option<&[u8]>) -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        zip.start_file("META/info.json", options).unwrap();
        let info = r#"{
            "Name": "Test Mod",
            "Author": "Test Author",
            "Version": "1.0.0",
            "Description": "A test mod",
            "Hashtables": [
                {
                    "Path": "META/hashes/game.hashes.txt",
                    "Category": "game",
                    "Algorithm": "xxh64",
                    "Bits": 64
                }
            ]
        }"#;
        zip.write_all(info.as_bytes()).unwrap();

        zip.start_file("META/hashes/game.hashes.txt", options)
            .unwrap();
        zip.write_all(names.as_bytes()).unwrap();

        if let Some(wad) = packed_wad {
            zip.start_file("WAD/Aatrox.wad.client", options).unwrap();
            zip.write_all(wad).unwrap();
        }

        zip.finish().unwrap().into_inner()
    }

    /// A config path that climbs out of `hashes/` must not poison the
    /// archive: the entry lands by its file name under `META/hashes/`, so
    /// the result is one `FantomeReader::new` (which refuses any archive
    /// holding an escaping entry name) will accept.
    #[test]
    fn a_climbing_config_path_packs_to_a_contained_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp).join("project");
        write_project_tree(&root);
        std::fs::create_dir_all(root.join(HASHES_DIR_NAME)).unwrap();
        // Resolves to a real file beside the project, as the config says.
        std::fs::write(utf8_dir(&tmp).join("evil.hashes.txt"), "assets/one.tex\n").unwrap();

        let project = ModProject {
            hashtables: vec![ModProjectHashtable {
                path: format!("{HASHES_DIR_NAME}/../../evil.hashes.txt"),
                ..game_manifest()
            }],
            ..test_project(None)
        };

        let buffer = pack(&project, &root);
        let mut reader = ltk_fantome::FantomeReader::new(buffer).unwrap();
        let info = reader.read_info().unwrap();
        assert_eq!(info.hashtables[0].path, "META/hashes/evil.hashes.txt");

        let tables = reader.read_hashtables().unwrap();
        assert_eq!(tables[0].1.names().collect::<Vec<_>>(), ["assets/one.tex"]);
    }

    /// The whole-archive escape refusal covers table entries too: a hostile
    /// manifest cannot steer an import, because the entry it has to point at
    /// is refused at mount.
    #[test]
    fn an_archive_whose_table_entry_escapes_is_refused() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        zip.start_file("META/info.json", options).unwrap();
        zip.write_all(
            br#"{
            "Name": "Test Mod",
            "Author": "Test Author",
            "Version": "1.0.0",
            "Description": "A test mod",
            "Hashtables": [
                {
                    "Path": "META/hashes/../../evil.hashes.txt",
                    "Category": "game",
                    "Algorithm": "xxh64",
                    "Bits": 64
                }
            ]
        }"#,
        )
        .unwrap();
        zip.start_file("META/hashes/../../evil.hashes.txt", options)
            .unwrap();
        zip.write_all(b"assets/custom/one.tex\n").unwrap();
        let archive = zip.finish().unwrap().into_inner();

        let tmp = tempfile::tempdir().unwrap();
        let output = utf8_dir(&tmp).join("project");
        let error = import(archive, &output).unwrap_err();
        assert!(
            matches!(
                &error,
                ImportError::Format(FantomeImportError::Extract(
                    ltk_fantome::FantomeExtractError::EscapingEntry { .. }
                ))
            ),
            "expected the mount-time refusal, got {error:?}"
        );
        assert!(!utf8_dir(&tmp).join("evil.hashes.txt").exists());
    }

    #[test]
    fn import_writes_the_declared_table_into_hashes() {
        let archive = fantome_with_table("assets/custom/one.tex\n", None);

        let tmp = tempfile::tempdir().unwrap();
        let output = utf8_dir(&tmp);
        let imported = import(archive, &output).unwrap();

        assert_eq!(imported.hashtables, vec![game_manifest()]);
        assert_eq!(
            std::fs::read_to_string(output.join(&game_manifest().path)).unwrap(),
            "assets/custom/one.tex\n"
        );
    }

    /// User story 26: the mod's own table names its chunks, ahead of the
    /// caller's resolver - here one that would claim every chunk.
    #[test]
    fn the_mods_own_table_names_its_packed_wad_chunks() {
        let archive =
            fantome_with_table("packed/file.bin\n", Some(&packed_wad_bytes(b"chunk bytes")));

        let tmp = tempfile::tempdir().unwrap();
        let output = utf8_dir(&tmp);
        ProjectImporter::new(&output)
            .import(FantomeImporter::new(Cursor::new(archive)).with_path_resolver(&FixedResolver))
            .unwrap();

        let named = output.join("content/base/Aatrox.wad.client/packed/file.bin");
        assert_eq!(std::fs::read(&named).unwrap(), b"chunk bytes");
        assert!(
            !output
                .join("content/base/Aatrox.wad.client/assets/characters/aatrox/skin0.bin")
                .exists(),
            "the caller's resolver must not outrank the mod's own table"
        );
    }

    /// Tables land flat under `META/hashes/` by file name, so two different
    /// table files can collide on one archive name. Refused rather than
    /// renamed: a silently renamed table would ship under a name nobody
    /// chose.
    #[test]
    fn colliding_table_file_names_fail_the_pack() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8_dir(&tmp);
        write_project_tree(&root);
        std::fs::create_dir_all(root.join("hashes")).unwrap();
        std::fs::create_dir_all(root.join("backup")).unwrap();
        std::fs::write(root.join("hashes/game.hashes.txt"), "a/one.tex\n").unwrap();
        std::fs::write(root.join("backup/game.hashes.txt"), "a/two.tex\n").unwrap();

        let entry = |path: &str| ModProjectHashtable {
            path: path.to_string(),
            ..game_manifest()
        };
        let project = ModProject {
            hashtables: vec![
                entry("hashes/game.hashes.txt"),
                entry("backup/game.hashes.txt"),
            ],
            ..test_project(None)
        };

        let err = try_pack(&project, &root).unwrap_err();
        assert!(
            matches!(
                err,
                PackError::Format(FantomePackError::DuplicateHashtableName(ref e))
                    if e.destination() == "META/hashes/game.hashes.txt"
                        && e.first() == "hashes/game.hashes.txt"
                        && e.second() == "backup/game.hashes.txt"
            ),
            "Expected DuplicateHashtableName, got: {err}"
        );
    }

    /// Two declared tables landing on one `hashes/` file name would clobber
    /// each other on disk. An archive shaped like this is ambiguous, and an
    /// import must refuse it rather than invent names.
    #[test]
    fn an_archive_with_colliding_table_names_fails_the_import() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        zip.start_file("META/info.json", options).unwrap();
        let info = r#"{
            "Name": "Test Mod",
            "Author": "Test Author",
            "Version": "1.0.0",
            "Description": "A test mod",
            "Hashtables": [
                {
                    "Path": "META/hashes/game.hashes.txt",
                    "Category": "game",
                    "Algorithm": "xxh64",
                    "Bits": 64
                },
                {
                    "Path": "OTHER/game.hashes.txt",
                    "Category": "game",
                    "Algorithm": "xxh64",
                    "Bits": 64
                }
            ]
        }"#;
        zip.write_all(info.as_bytes()).unwrap();
        zip.start_file("META/hashes/game.hashes.txt", options)
            .unwrap();
        zip.write_all(b"a/one.tex\n").unwrap();
        zip.start_file("OTHER/game.hashes.txt", options).unwrap();
        zip.write_all(b"a/two.tex\n").unwrap();
        let archive = zip.finish().unwrap().into_inner();

        let tmp = tempfile::tempdir().unwrap();
        let output = utf8_dir(&tmp);
        let err = import(archive, &output).unwrap_err();
        assert!(
            matches!(
                err,
                ImportError::Format(FantomeImportError::DuplicateHashtableName(ref e))
                    if e.destination() == "hashes/game.hashes.txt"
                        && e.first() == "META/hashes/game.hashes.txt"
                        && e.second() == "OTHER/game.hashes.txt"
            ),
            "Expected DuplicateHashtableName, got: {err}"
        );
    }

    /// A planned table declared at `path`, for driving the mapping directly.
    fn planned_table(path: &str) -> crate::pack::PlannedHashtable {
        crate::pack::PlannedHashtable::new(
            Utf8PathBuf::from(path),
            ltk_hashtable::HashtableEntry::new(
                path,
                Category::Game,
                Algorithm::Xxh64,
                ltk_hashtable::KeyWidth::new(64).unwrap(),
            ),
            ltk_hashtable::Hashtable::from_names(["a/one.tex"]).unwrap(),
        )
    }

    /// A table already under `hashes/` keeps its tail; one declared
    /// elsewhere lands by its file name; one file declared twice stays one
    /// archive entry - and each route carries the table it was mapped from.
    #[test]
    fn tables_declared_outside_hashes_land_by_file_name() {
        use super::super::convert::fantome_routes;

        let planned = [
            planned_table("tables/binhashes.hashes.txt"),
            planned_table("hashes/game.hashes.txt"),
            planned_table("hashes/game.hashes.txt"),
        ];

        let routes = fantome_routes(&planned).unwrap();
        let paths: Vec<&str> = routes
            .iter()
            .map(|route| route.manifest.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "META/hashes/binhashes.hashes.txt",
                "META/hashes/game.hashes.txt",
                "META/hashes/game.hashes.txt",
            ]
        );
        for (route, planned) in routes.iter().zip(&planned) {
            assert_eq!(route.planned, planned, "a route carries its own table");
        }
    }

    /// Two different files landing on one archive name are refused, not
    /// renamed - a renamed table would ship under a name nobody chose.
    #[test]
    fn two_files_landing_on_one_archive_name_are_refused() {
        use super::super::convert::fantome_routes;

        let planned = [
            planned_table("tables/game.hashes.txt"),
            planned_table("hashes/game.hashes.txt"),
        ];

        let err = fantome_routes(&planned).unwrap_err();
        assert_eq!(err.destination(), "META/hashes/game.hashes.txt");
        assert_eq!(err.first(), "tables/game.hashes.txt");
        assert_eq!(err.second(), "hashes/game.hashes.txt");
    }

    /// The import-direction mapping: a `META/hashes/` tail keeps its subpath,
    /// a table elsewhere lands by file name, one file declared twice stays
    /// one file, and an entry no key width fits is dropped (its table cannot
    /// be read out of the archive).
    #[test]
    fn archive_manifests_map_to_project_paths() {
        use ltk_fantome::FantomeHashtable;

        use super::super::convert::project_routes;

        let entry = |path: &str, bits| FantomeHashtable {
            path: path.to_owned(),
            category: Category::Game,
            algorithm: Algorithm::Xxh64,
            bits,
        };

        let routes = project_routes(&[
            entry("META/hashes/sub/game.hashes.txt", 64),
            entry("ELSEWHERE/game.hashes.txt", 64),
            entry("META/hashes/sub/game.hashes.txt", 64),
            entry("META/hashes/broken.hashes.txt", 0),
        ])
        .unwrap();

        let paths: Vec<&str> = routes
            .iter()
            .map(|route| route.manifest.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "hashes/sub/game.hashes.txt",
                "hashes/game.hashes.txt",
                "hashes/sub/game.hashes.txt",
            ]
        );
    }

    /// The mapping is where escape-proofing lives, whoever calls it: a tail
    /// that climbs, re-roots, or smuggles a platform separator lands by its
    /// file name, never as declared.
    #[test]
    fn a_traversing_manifest_path_maps_to_a_plain_project_path() {
        use ltk_fantome::FantomeHashtable;

        use super::super::convert::project_routes;

        let entry = |path: &str| FantomeHashtable {
            path: path.to_owned(),
            category: Category::Game,
            algorithm: Algorithm::Xxh64,
            bits: 64,
        };

        let routes = project_routes(&[
            entry("META/hashes/../../evil.hashes.txt"),
            entry("META/hashes/sub/../evil2.hashes.txt"),
            entry(r"META/hashes/sub\evil3.hashes.txt"),
        ])
        .unwrap();

        let paths: Vec<&str> = routes
            .iter()
            .map(|route| route.manifest.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "hashes/evil.hashes.txt",
                "hashes/evil2.hashes.txt",
                // The whole final component: `\` is not a separator here.
                "hashes/unnamed.hashes.txt",
            ]
        );
    }
}
