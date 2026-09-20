use std::fs::File;
use std::io::Cursor;

use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{FantomeInfo, FantomeReader, FantomeWriter};
use ltk_hashtable::Category;

use super::{preserve_archive_names, PreserveOutcome};

fn write_source(dir: &Utf8Path, entries: &[(&str, &str, &[u8])]) -> Utf8PathBuf {
    let path = dir.join("mod.fantome");
    let mut writer = FantomeWriter::new(File::create(path.as_std_path()).unwrap());
    writer
        .write_info(&FantomeInfo {
            name: "Mod".to_owned(),
            ..Default::default()
        })
        .unwrap();
    for (wad, rel, content) in entries {
        writer.write_wad_entry(wad, rel, &mut &content[..]).unwrap();
    }
    writer.finish().unwrap();
    path
}

fn temp_dir() -> (tempfile::TempDir, Utf8PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8Path::from_path(dir.path()).unwrap().to_owned();
    (dir, path)
}

#[test]
fn chunk_paths_on_disk_are_harvested_and_a_hex_name_is_not_guessed_at() {
    let (_guard, dir) = temp_dir();
    let source = write_source(
        &dir,
        &[
            ("Aatrox.wad.client", "ASSETS/Custom/Trail.tex", b"a"),
            ("Aatrox.wad.client", "0123456789abcdef.tex", b"b"),
        ],
    );
    let dest = dir.join("library.fantome");

    let report = preserve_archive_names(&source, &dest, None).unwrap();

    assert_eq!(
        report.outcome,
        PreserveOutcome::Rewritten { names_added: 1 }
    );
    assert_eq!(report.unharvestable, 1);

    let mut reader = FantomeReader::new(File::open(dest.as_std_path()).unwrap()).unwrap();
    let tables = reader.read_hashtables().unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(*tables[0].0.category(), Category::Game);
    assert_eq!(
        tables[0].1.names().collect::<Vec<_>>(),
        ["ASSETS/Custom/Trail.tex"]
    );
    // The source the user gave is untouched.
    let mut source_reader = FantomeReader::new(File::open(source.as_std_path()).unwrap()).unwrap();
    assert!(source_reader.read_hashtables().unwrap().is_empty());
    let _ = Cursor::new(());
}

/// A string as a bin writes it: a little-endian u16 length and the bytes.
fn bin_string(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u16).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

/// Enough of a bin to carry the magic and a few strings.
fn bin_with(paths: &[&str]) -> Vec<u8> {
    let mut out = b"PROP".to_vec();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(paths.len() as u32).to_le_bytes());
    for path in paths {
        bin_string(&mut out, path);
    }
    out
}

#[test]
fn names_in_a_packed_wads_bins_are_harvested() {
    use std::io::Write;

    use ltk_wad::{WadBuilder, WadChunkBuilder};

    let recovered_path = "assets/custom/recovered.tex";
    let bin = bin_with(&[recovered_path]);

    let mut wad_bytes = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(WadChunkBuilder::default().with_path(recovered_path))
        .with_chunk(WadChunkBuilder::default().with_path("data/anonymous.bin"))
        .build_to_writer(&mut wad_bytes, |hash, cursor| {
            if hash == ltk_wad::WadHash::from(recovered_path) {
                cursor.write_all(b"texture payload")?;
            } else {
                cursor.write_all(&bin)?;
            }
            Ok(())
        })
        .unwrap();

    let (_guard, dir) = temp_dir();
    let source = dir.join("mod.fantome");
    let mut zip = zip::ZipWriter::new(File::create(source.as_std_path()).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("META/info.json", options).unwrap();
    serde_json::to_writer(
        &mut zip,
        &FantomeInfo {
            name: "Packed".to_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    zip.start_file("WAD/Aatrox.wad.client", options).unwrap();
    zip.write_all(&wad_bytes.into_inner()).unwrap();
    zip.finish().unwrap();
    let dest = dir.join("library.fantome");

    let report = preserve_archive_names(&source, &dest, None).unwrap();

    // The bin named the texture chunk; the bin chunk itself is named by
    // nothing and is counted, not guessed at.
    assert_eq!(
        report.outcome,
        PreserveOutcome::Rewritten { names_added: 1 }
    );
    assert_eq!(report.unharvestable, 1);

    let mut reader = FantomeReader::new(File::open(dest.as_std_path()).unwrap()).unwrap();
    let tables = reader.read_hashtables().unwrap();
    assert_eq!(tables[0].1.names().collect::<Vec<_>>(), [recovered_path]);
}

#[test]
fn the_exclusions_remove_names_a_reader_can_recover_elsewhere() {
    use std::collections::HashMap;

    use ltk_wad::WadHash;

    let (_guard, dir) = temp_dir();
    let source = write_source(
        &dir,
        &[
            ("Aatrox.wad.client", "assets/known/by_community.tex", b"a"),
            ("Aatrox.wad.client", "assets/custom/only_ours.tex", b"b"),
        ],
    );
    let dest = dir.join("library.fantome");

    let community: HashMap<WadHash, String> = [(
        WadHash::from("assets/known/by_community.tex"),
        "assets/known/by_community.tex".to_owned(),
    )]
    .into();

    let report = preserve_archive_names(&source, &dest, Some(&community)).unwrap();

    assert_eq!(
        report.outcome,
        PreserveOutcome::Rewritten { names_added: 1 }
    );
    let mut reader = FantomeReader::new(File::open(dest.as_std_path()).unwrap()).unwrap();
    let tables = reader.read_hashtables().unwrap();
    assert_eq!(
        tables[0].1.names().collect::<Vec<_>>(),
        ["assets/custom/only_ours.tex"]
    );
}

#[test]
fn preserving_twice_is_a_no_op() {
    let (_guard, dir) = temp_dir();
    let source = write_source(&dir, &[("Aatrox.wad.client", "assets/custom/a.tex", b"a")]);
    let dest = dir.join("library.fantome");

    let first = preserve_archive_names(&source, &dest, None).unwrap();
    assert_eq!(first.outcome, PreserveOutcome::Rewritten { names_added: 1 });
    let after_first = std::fs::read(dest.as_std_path()).unwrap();

    // A second preserve of the already-preserved archive, in place.
    let second = preserve_archive_names(&dest, &dest, None).unwrap();
    assert_eq!(second.outcome, PreserveOutcome::Unchanged);
    assert_eq!(std::fs::read(dest.as_std_path()).unwrap(), after_first);
}

#[test]
fn a_preserve_that_fails_leaves_the_destination_as_it_was() {
    let (_guard, dir) = temp_dir();
    let source = dir.join("broken.fantome");
    std::fs::write(source.as_std_path(), b"not a zip archive").unwrap();
    let dest = dir.join("library.fantome");
    std::fs::write(dest.as_std_path(), b"existing library archive").unwrap();

    preserve_archive_names(&source, &dest, None).unwrap_err();

    assert_eq!(
        std::fs::read(dest.as_std_path()).unwrap(),
        b"existing library archive"
    );
}

#[test]
fn a_covered_mod_is_copied_to_the_destination_unrewritten() {
    use std::collections::HashMap;

    use ltk_wad::WadHash;

    let (_guard, dir) = temp_dir();
    let source = write_source(&dir, &[("Aatrox.wad.client", "assets/known.tex", b"a")]);
    let dest = dir.join("library.fantome");

    let community: HashMap<WadHash, String> = [(
        WadHash::from("assets/known.tex"),
        "assets/known.tex".to_owned(),
    )]
    .into();

    let report = preserve_archive_names(&source, &dest, Some(&community)).unwrap();

    assert_eq!(report.outcome, PreserveOutcome::Unchanged);
    assert_eq!(
        std::fs::read(dest.as_std_path()).unwrap(),
        std::fs::read(source.as_std_path()).unwrap()
    );
}
