use super::*;

use std::io::Write;

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::FantomeReader;

const WAD_ENTRY: &str = "WAD/Aatrox.wad.client";
const WAD_NAME: &str = "Aatrox.wad.client";
const PAYLOAD: &[u8] = b"the chunk payload, stored uncompressed";

/// A packed WAD holding one stored chunk under `packed/file.bin`.
fn packed_wad_bytes() -> Vec<u8> {
    use ltk_wad::{WadBuilder, WadChunkBuilder, WadChunkCompression};

    let mut cursor = Cursor::new(Vec::new());
    WadBuilder::default()
        .with_chunk(
            WadChunkBuilder::default()
                .with_path("packed/file.bin")
                .with_force_compression(WadChunkCompression::None),
        )
        .build_to_writer(&mut cursor, |_hash, writer| {
            writer.write_all(PAYLOAD)?;
            Ok(())
        })
        .unwrap();
    cursor.into_inner()
}

/// An archive whose packed WAD is held under `wads`, behind a deflated entry.
///
/// The leading entry is what makes these tests worth running: it pushes the WAD
/// off offset zero, so a source reporting the outer archive's offsets and one
/// reporting the WAD's own no longer agree.
fn archive(wads: CompressionMethod) -> FantomeReader<Cursor<Vec<u8>>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("META/info.json", deflated).unwrap();
    zip.write_all(br#"{"Name":"Mod","Author":"A","Version":"1","Description":"d"}"#)
        .unwrap();
    zip.start_file(WAD_ENTRY, deflated.compression_method(wads))
        .unwrap();
    zip.write_all(&packed_wad_bytes()).unwrap();

    FantomeReader::new(Cursor::new(zip.finish().unwrap().into_inner())).unwrap()
}

/// The chunk `packed/file.bin`, read back through a mounted WAD.
fn read_the_chunk(reader: &mut FantomeReader<Cursor<Vec<u8>>>) -> Vec<u8> {
    let mut wad = reader.mount_packed_wad(WAD_NAME).unwrap().unwrap();
    let chunk = *wad.chunks().iter().next().unwrap();
    wad.load_chunk_decompressed(&chunk).unwrap().to_vec()
}

/// A stored entry is what normalization produces, and reading it is the point
/// of producing it: the WAD's own offsets land on its own bytes with nothing
/// inflated on the way.
#[test]
fn a_stored_packed_wad_is_read_where_the_archive_keeps_it() {
    let mut reader = archive(CompressionMethod::Stored);

    assert!(
        reader
            .packed_wad_source(WAD_NAME)
            .unwrap()
            .unwrap()
            .is_in_place(),
        "a stored entry should be read in place"
    );
    assert_eq!(read_the_chunk(&mut reader), PAYLOAD);
}

/// Deflate has no random access, so an archive nobody normalized still has to
/// be inflated whole. It must read back the same chunk regardless: which arm a
/// caller gets changes the cost and nothing else.
#[test]
fn a_deflated_packed_wad_is_inflated_and_reads_the_same() {
    let mut reader = archive(CompressionMethod::Deflated);

    assert!(
        !reader
            .packed_wad_source(WAD_NAME)
            .unwrap()
            .unwrap()
            .is_in_place(),
        "a deflated entry cannot be read in place"
    );
    assert_eq!(read_the_chunk(&mut reader), PAYLOAD);
}

/// A WAD shipped as a directory of loose files has no packed entry, and a name
/// the archive does not hold at all has none either. Neither is an error.
#[test]
fn a_wad_the_archive_holds_no_packed_copy_of_mounts_to_none() {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "WAD/Ahri.wad.client/data/loose.bin",
        SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"a loose file").unwrap();
    let bytes = zip.finish().unwrap().into_inner();

    let mut reader = FantomeReader::new(Cursor::new(bytes)).unwrap();
    assert!(
        reader
            .mount_packed_wad("Ahri.wad.client")
            .unwrap()
            .is_none()
    );
    assert!(
        reader
            .mount_packed_wad("Absent.wad.client")
            .unwrap()
            .is_none()
    );
}

/// The entry reader underneath answers a seek with the offset it reached in the
/// outer archive, which is not the number [`Seek`] asks for. A WAD's TOC holds
/// offsets from the WAD's own first byte, so a source reporting the archive's
/// would put every chunk read past where that chunk is.
#[test]
fn seeking_counts_from_the_wads_own_first_byte() {
    let wad_bytes = packed_wad_bytes();
    let end = wad_bytes.len() as u64;
    let mut reader = archive(CompressionMethod::Stored);
    let mut source = reader.packed_wad_source(WAD_NAME).unwrap().unwrap();

    assert_eq!(source.seek(SeekFrom::Start(0)).unwrap(), 0);
    assert_eq!(source.stream_position().unwrap(), 0);
    assert_eq!(source.seek(SeekFrom::Start(4)).unwrap(), 4);
    assert_eq!(source.seek(SeekFrom::Current(2)).unwrap(), 6);
    assert_eq!(source.seek(SeekFrom::End(0)).unwrap(), end);

    // Past the end lands on the end, and reads nothing rather than reading on
    // into whatever the archive keeps after the entry.
    assert_eq!(source.seek(SeekFrom::Start(end + 4096)).unwrap(), end);
    assert_eq!(source.read(&mut [0u8; 16]).unwrap(), 0);

    // And the bytes at an offset are the WAD's bytes at that offset.
    source.seek(SeekFrom::Start(0)).unwrap();
    let mut magic = [0u8; 2];
    source.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, &wad_bytes[..2]);
}

/// Both arms answer a seek past the end with the end, so a caller cannot tell
/// which one it got from the positions it reads back.
#[test]
fn both_arms_report_the_same_positions() {
    let end = packed_wad_bytes().len() as u64;

    for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        let mut reader = archive(method);
        let mut source = reader.packed_wad_source(WAD_NAME).unwrap().unwrap();

        assert_eq!(source.seek(SeekFrom::End(0)).unwrap(), end, "{method:?}");
        assert_eq!(
            source.seek(SeekFrom::Start(end + 4096)).unwrap(),
            end,
            "{method:?}"
        );
        assert_eq!(source.seek(SeekFrom::Start(7)).unwrap(), 7, "{method:?}");
        assert_eq!(source.stream_position().unwrap(), 7, "{method:?}");
    }
}

/// A negative seek is refused rather than wrapping into a far offset.
#[test]
fn a_seek_before_the_start_is_refused() {
    for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        let mut reader = archive(method);
        let mut source = reader.packed_wad_source(WAD_NAME).unwrap().unwrap();

        let refused = source.seek(SeekFrom::Current(-1)).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput, "{method:?}");
    }
}
