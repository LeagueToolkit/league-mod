use super::*;
use crate::content::CompressedChunk;
use ltk_wad::WadChunkCompression;
use xxhash_rust::xxh3::xxh3_64;

fn meta_with_content_hash(content_hash: ContentHash) -> OverrideMeta {
    OverrideMeta {
        content_hash,
        uncompressed_size: 0,
        source: OverrideSource::Raw {
            mod_id: "m".to_string(),
            rel_path: Utf8PathBuf::from("assets/x.bin"),
        },
        fallback_wad: None,
        unlocalized_wad: None,
        linked_bins: Vec::new(),
    }
}

/// Two chunks with the same content must end up sharing one compressed
/// buffer: that is what makes every copy of a cross-WAD chunk carry the
/// same compressed checksum, which the game validates. The second chunk's
/// bytes are never even requested - `needs` answers false for it.
#[test]
fn identical_content_is_read_and_compressed_once() {
    let all_meta = HashMap::from([
        (
            WadHash(0xAAAA),
            meta_with_content_hash(ContentHash(0xC0FFEE)),
        ),
        (
            WadHash(0xBBBB),
            meta_with_content_hash(ContentHash(0xC0FFEE)),
        ),
    ]);
    let mut preparer = OverrideCompressor::new(&all_meta, HashMap::new(), BATCH_BUDGET_BYTES);

    assert!(preparer.needs(WadHash(0xAAAA)));
    preparer
        .supply(
            WadHash(0xAAAA),
            Arc::from(b"the same asset, twice".repeat(64).as_slice()),
        )
        .unwrap();
    assert!(
        !preparer.needs(WadHash(0xBBBB)),
        "a second path hash with the same content must not be read again"
    );

    let prepared = preparer.finish().unwrap();

    assert_eq!(prepared.len(), 2);
    let a = &prepared[&WadHash(0xAAAA)];
    let b = &prepared[&WadHash(0xBBBB)];
    assert!(
        std::ptr::eq(a.compressed(), b.compressed()),
        "equal content must share one compressed buffer, not two equal copies"
    );
    assert_eq!(a.checksum(), b.checksum());
}

/// Different content still gets its own compression, and the writer's TOC
/// fields come straight off each prepared override.
#[test]
fn differing_content_is_compressed_separately() {
    let all_meta = HashMap::from([
        (WadHash(0xAAAA), meta_with_content_hash(ContentHash(1))),
        (WadHash(0xBBBB), meta_with_content_hash(ContentHash(2))),
    ]);
    let mut preparer = OverrideCompressor::new(&all_meta, HashMap::new(), BATCH_BUDGET_BYTES);

    assert!(preparer.needs(WadHash(0xAAAA)));
    preparer
        .supply(WadHash(0xAAAA), Arc::from(b"first".repeat(64).as_slice()))
        .unwrap();
    assert!(preparer.needs(WadHash(0xBBBB)));
    preparer
        .supply(WadHash(0xBBBB), Arc::from(b"second".repeat(64).as_slice()))
        .unwrap();

    let prepared = preparer.finish().unwrap();

    assert_ne!(
        prepared[&WadHash(0xAAAA)].checksum(),
        prepared[&WadHash(0xBBBB)].checksum()
    );
    assert_eq!(prepared[&WadHash(0xAAAA)].uncompressed_size(), 5 * 64);
    assert_eq!(prepared[&WadHash(0xBBBB)].uncompressed_size(), 6 * 64);
}

/// A chunk the metadata pass never saw cannot be deduplicated against
/// anything, but it must still be prepared rather than dropped.
#[test]
fn content_without_metadata_still_gets_prepared() {
    let empty_meta = HashMap::new();
    let mut preparer = OverrideCompressor::new(&empty_meta, HashMap::new(), BATCH_BUDGET_BYTES);

    assert!(preparer.needs(WadHash(0xAAAA)));
    preparer
        .supply(WadHash(0xAAAA), Arc::from(b"orphan".as_slice()))
        .unwrap();

    let prepared = preparer.finish().unwrap();

    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[&WadHash(0xAAAA)].uncompressed_size(), 6);
}

/// A budget small enough to flush on every supply must not break content
/// sharing: a duplicate arriving after its content already flushed still
/// skips the read and lands on the memoized buffer.
#[test]
fn sharing_survives_a_batch_flush_boundary() {
    let all_meta = HashMap::from([
        (WadHash(0xAAAA), meta_with_content_hash(ContentHash(7))),
        (WadHash(0xBBBB), meta_with_content_hash(ContentHash(7))),
        (WadHash(0xCCCC), meta_with_content_hash(ContentHash(8))),
    ]);
    // A 1-byte budget forces a flush after every supply.
    let mut preparer = OverrideCompressor::new(&all_meta, HashMap::new(), 1);

    assert!(preparer.needs(WadHash(0xAAAA)));
    preparer
        .supply(WadHash(0xAAAA), Arc::from(b"shared".repeat(64).as_slice()))
        .unwrap();
    assert!(preparer.needs(WadHash(0xCCCC)));
    preparer
        .supply(WadHash(0xCCCC), Arc::from(b"other".repeat(64).as_slice()))
        .unwrap();
    assert!(
        !preparer.needs(WadHash(0xBBBB)),
        "content compressed in an earlier batch must not be requested again"
    );

    let prepared = preparer.finish().unwrap();

    assert_eq!(prepared.len(), 3);
    assert!(std::ptr::eq(
        prepared[&WadHash(0xAAAA)].compressed(),
        prepared[&WadHash(0xBBBB)].compressed()
    ));
}

/// A chunk handed over already compressed is never queued for compression,
/// and every other path hash carrying the same content lands on exactly the
/// bytes it copied - which is what keeps every WAD holding a shared chunk on
/// one compressed checksum.
#[test]
fn a_passed_through_chunk_is_never_compressed_or_read_again() {
    let all_meta = HashMap::from([
        (WadHash(0xAAAA), meta_with_content_hash(ContentHash(0xF00D))),
        (WadHash(0xBBBB), meta_with_content_hash(ContentHash(0xF00D))),
    ]);
    let mut preparer = OverrideCompressor::new(&all_meta, HashMap::new(), BATCH_BUDGET_BYTES);

    let stored = b"bytes lifted straight out of a packed WAD".to_vec();
    assert!(preparer.needs(WadHash(0xAAAA)));
    preparer.supply_prepared(
        WadHash(0xAAAA),
        PreparedOverride::pass_through(
            WadHash(0xAAAA),
            CompressedChunk {
                compressed: stored.clone(),
                compression: WadChunkCompression::None,
                uncompressed_size: stored.len(),
                claimed_checksum: xxh3_64(&stored),
            },
        )
        .unwrap()
        .unwrap(),
    );
    assert!(
        !preparer.needs(WadHash(0xBBBB)),
        "content already passed through must not be read or compressed again"
    );

    let prepared = preparer.finish().unwrap();

    assert_eq!(
        prepared[&WadHash(0xAAAA)].compressed(),
        stored,
        "the container's bytes must reach the writer verbatim"
    );
    assert!(std::ptr::eq(
        prepared[&WadHash(0xAAAA)].compressed(),
        prepared[&WadHash(0xBBBB)].compressed()
    ));
}

/// Reused overrides recovered from a tail rewrite seed the memo: content
/// they already cover is never requested, and the reused entry itself
/// survives into the result under its own path hash.
#[test]
fn reused_content_is_never_requested() {
    let all_meta = HashMap::from([
        (WadHash(0xAAAA), meta_with_content_hash(ContentHash(9))),
        (WadHash(0xBBBB), meta_with_content_hash(ContentHash(9))),
    ]);
    let reused_override =
        PreparedOverride::compress(WadHash(0xAAAA), b"recovered from the tail").unwrap();
    let reused = HashMap::from([(WadHash(0xAAAA), reused_override)]);
    let mut preparer = OverrideCompressor::new(&all_meta, reused, BATCH_BUDGET_BYTES);

    assert!(
        !preparer.needs(WadHash(0xBBBB)),
        "content a tail rewrite recovered must not be read or compressed again"
    );

    let prepared = preparer.finish().unwrap();

    assert_eq!(prepared.len(), 2);
    assert!(std::ptr::eq(
        prepared[&WadHash(0xAAAA)].compressed(),
        prepared[&WadHash(0xBBBB)].compressed()
    ));
}
