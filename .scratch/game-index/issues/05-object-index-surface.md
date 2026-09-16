---
title: "Game index: object index surface"
labels: area:api
---

Part of the game index map.
Type: grilling
Status: resolved
Blocked by: 01, 03, 04

## Question

What is the object index's public surface behind its feature flag? Decide: the flag's name; the
build inputs (the chunk index, archive access, the resolver, a parallelism knob); how a bin chunk
is recognised (named `.bin`, bare-named, sniffed by magic); what one row carries (object hash,
class hash, declaring chunk hash, holder WAD); the queries the declaration engine needs
(`declares(object)`, declaring chunks of an object in archive order) and the ones the manager
needs beyond that; how names are attached and swapped without rebuilding rows; whether rows are
cached to disk. The manager's `object_index.rs`, `object_index/build.rs` and `object_index/names.rs`
are the source of signatures.

## Answer

- Feature flag `objects`. It pulls in `ltk_meta` 0.8.2 and `ltk_file` from crates.io, no git pin.
- Build: `ObjectIndex::build(game: &GameIndex)` and `build_with(game, &BuildOptions)`.
  `BuildOptions<'a>` derives `Default` and carries `resolver: Option<&'a dyn ResolveWadPath>`,
  `workers: NonZeroUsize` (default available parallelism), `called_off: Option<&'a (dyn Fn() -> bool + Sync)>`.
  Archive paths come from `GameIndex::archives()`.
- A chunk is a bin by `.bin` extension when named; a bare-named or unnamed chunk is sniffed by
  magic and read whole when the magic is a bin's. A `PTCH` is read eagerly through `BinOverride`.
- Row: `Declaration { object: BinHash, class: BinHash, chunk: WadHash, archive: ArchiveId }`.
- Queries: `declares(BinHash) -> bool`, `declarations(BinHash) -> &[Declaration]` in archive
  order, `for_each_chunk_declaration(WadHash, f)` for what one bin declares, `stats() -> ObjectStats`.
  Free function `for_each_declaration(reader, f)` reads one bin from any `Read + Seek`.
- Unreadable chunks and archives skip and count in `ObjectStats`; archive-level failures also
  land in a `skipped` list mirroring the chunk index.
- Rows cache alongside the chunk index: same fingerprint, same MessagePack format, own version tag.
- Parallelism through the default `rayon` feature, serial loop without it. `called_off` is tested
  before each archive.
