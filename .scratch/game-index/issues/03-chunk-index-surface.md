---
title: "Game index: chunk index surface"
labels: area:api
---

Part of the game index map.
Type: grilling
Status: resolved
Blocked by: 02

## Question

What is the public surface of the chunk index? Decide: the crate and type names; how a holder WAD
is identified (`DATA/FINAL`-relative path, ordinal into an archive list, or both); what one chunk
row carries (size, compression, checksum, every holder); the lookups (`holders_of(hash)`,
`find_wad(name)`, chunk by path with lowercase XXH64 done inside); ambiguity and missing-directory
errors; the fingerprint's inputs; the versioned cache format and its `save`/`load_or_build` pair;
both build entry points (game directory, archive list); which archive read failures skip and which
fail the build. Lift signatures from `crates/ltk_overlay/src/game_index.rs` and the manager's
`game_index.rs` build path where they already answer the question.

## Answer

- Crate name `ltk_game_index`. Type `GameIndex` for the chunk index.
- Archive identity is `ArchiveId(u32)`, an ordinal into a sorted archive list. An `Archive` entry
  carries the `DATA/FINAL`-relative forward-slash name and the absolute path. Holders are
  `&[ArchiveId]` in ordinal order. The first holder is ordinal order's first.
- A chunk row carries the uncompressed size and, per holder, the WAD TOC checksum. No offsets, no
  compression kind.
- Lookups: `holders(WadHash) -> &[ArchiveId]` (empty when absent), `holders_of_path(&str)` hashing
  with ASCII lowercase XXH64 seed 0 inside, `size(WadHash) -> Option<u64>`,
  `archive(ArchiveId) -> &Archive`, `archives() -> &[Archive]`,
  `archive_by_filename(&str) -> Result<ArchiveId, ArchiveLookupError>` with `Absent` and
  `Ambiguous(Vec<ArchiveId>)`, `for_each_chunk(FnMut(WadHash, &ChunkRow))`,
  `dominant_holder(&[WadHash]) -> Option<ArchiveId>` (the archive with the most hits, the
  overlay's `find_best_matching_wad`), `fingerprint() -> Fingerprint` (newtype over `u64`).
- Fence helpers: `compute_content_hashes_batch`, `localized_global_wads` and `subchunktoc_blocked`
  stay in `ltk_overlay` as functions over the index. The SubChunkTOC block list is an overlay
  concern.
- Build: `build(game_dir)` and `build_from_archives(&[path])`. An unreadable archive is skipped and
  recorded on the built index in a `skipped` list of archive and error. The fingerprint covers
  every archive, skipped ones included. Fingerprint inputs stay file size and modification time.
- Cache: MessagePack via `rmp-serde`, version tag, fingerprint check, `save(path)` and
  `load_or_build(game_dir, cache_path)` with a best-effort save. Rows serialize with ordinals.
- Fields are private. Tests build from fixture archives through `build_from_archives`.
