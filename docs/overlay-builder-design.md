# Overlay builder design

How `ltk_overlay` turns a set of enabled mods into a directory of patched WAD
files that the patcher redirects the game to load.

This describes the crate as it is. The `OverlayBuilder` API is documented in the
crate itself; this document covers the parts that are hard to see from any one
module: the build strategies, the patched-WAD file layout, and the trust rules
that let a rebuild skip work.

## The pipeline

A build runs in two passes over the mods, with the routing decisions in between.

1. **Index the game.** Every `.wad.client` under `Game/DATA/FINAL` is mounted
   and reduced to two maps: WAD filename to path, and chunk path hash to the
   WADs holding it. The index is cached in the state directory as
   `game_index.bin` and reused until the game is patched.

2. **Choose a strategy** by comparing `overlay.json` against the current
   configuration. See [Build strategies](#build-strategies).

3. **Pass 1 - metadata.** Each mod's override files are read, hashed, and
   dropped. What survives is an `OverrideMeta` per chunk: its content hash, its
   size, and where to re-read it from. A per-mod cache
   (`override_meta.bin`) skips this entirely for mods that have not changed.

   Overrides that can never reach the overlay are dropped here rather than
   carried: SubChunkTOC entries, mod-shipped stringtable chunks, and *lazy
   overrides* - bytes identical to the game's own, whose declared WAD already
   holds the chunk. A content hash is enough to recognise all three, which is
   why this pass never needs the bytes themselves.

4. **Route.** Each override is distributed to every game WAD that holds its path
   hash, plus the mod's declared WAD for new entries and cross-WAD imports. A
   per-WAD fingerprint over `(path_hash, content_hash)` pairs decides which WADs
   need rebuilding at all.

5. **Pass 2 - bytes.** Only the WADs being rebuilt have their override bytes
   re-read, from mod content providers or, for stringtable patches, generated
   from the game's own table.

   Splitting the passes on the content hash is what keeps peak memory
   proportional to the WADs being written rather than to every enabled mod: an
   incremental build that touches one champion WAD never loads the bytes behind
   the others. The writers take a resolve callback rather than a filled map, so
   each override's bytes are handed over on demand and freed once the last WAD
   holding them has written it.

6. **Compress once.** Every distinct override content is compressed a single
   time, in parallel, memoized on its content hash.

7. **Write.** WADs are patched in parallel, each either rewritten in full or
   updated in place. See [Patched WAD layout](#patched-wad-layout).

8. **Persist** the new `overlay.json`.

## Build strategies

Three levels, decided per build and then per WAD.

| Level           | Condition                                                 | Cost                             |
| --------------- | --------------------------------------------------------- | -------------------------------- |
| Skip            | State version, mod list, per-mod content fingerprints, game fingerprint, blocked WADs and string-override locales all match, no WAD is marked dirty, and every recorded WAD exists | Nothing is written |
| Incremental     | State version and game fingerprint match                   | Only WADs whose fingerprint changed |
| Full rebuild    | State version or game fingerprint differs                  | The overlay is wiped and rebuilt  |

Within an incremental build, each WAD that needs rebuilding is then either
rewritten in place (tail only) or rebuilt in full.

Per-mod content fingerprints participate in the skip because a mod ID is not
enough: a workshop project directory keeps its ID while its files change.

## Patched WAD layout

WAD v3.4 fixes the TOC directly after the 268-byte header and a `u32` chunk
count, so the first TOC entry starts at offset 272. But each TOC entry carries an
absolute `data_offset`, so data placement is free. A patched WAD uses that:

```text
[header 268 B][chunk count u32][TOC: 32 B x toc_capacity]
[source data region - the game WAD's data region, copied intact]
[override tail     - one entry per overridden or new chunk]
```

The header is magic (2) + version (2) + signature (256) + checksum (8).

The source data region is copied as **one sequential block**, from the first
byte any source chunk points at to the last. That includes the bytes of chunks
the mod overrides, which end up unreferenced by the TOC. Keeping them is
deliberate: dropping an override later becomes a TOC edit, with no need to
reopen the game WAD.

Transient TOC entries are the source entries with `data_offset` shifted by one
constant delta; every other field carries over, because the bytes did.

The header's signature and checksum are copied from the source verbatim. Riot's
RSA signature covers the *original* TOC, so it does not validate the patched
one - it is provenance, letting a verifier prove which signed WAD an overlay
came from.

**TOC slack is zero.** The `TOC_SLACK_ENTRIES` constant reserves no spare
entries. Reserving some would let a WAD gain or lose a chunk without moving
data, but it leaves a gap between the last TOC entry and the first data byte,
and the game has not been observed tolerating that gap in a real session. The
capacity is recorded and honoured throughout, so enabling slack later is that
constant plus an in-game test.

## Rebuilding a WAD in place

When a WAD's override *bytes* change but its chunk set does not, the file keeps
its header and its copied region: only the tail and the TOC are rewritten. That
is the difference between writing a mod's own bytes and copying 2.4 GiB.

`overlay.json` records a `WadLayoutRecord` per WAD - the source WAD's identity,
the region and tail offsets, the reserved TOC capacity, and a
`path_hash -> content_hash` map of what is currently in the tail.

A record is a **hint, never a fact**. Before it is used, all of this is
re-verified:

1. The state version is current, a record exists, and the WAD is not marked
   dirty.
2. The game WAD's length, mtime and TOC hash match the record.
3. The overlay file exists, parses, carries the source's signature, and is at
   least as long as the recorded tail offset.
4. Every transient entry in the overlay's TOC equals the source entry shifted
   by the recorded delta. Two TOCs compared in memory, milliseconds even for the
   largest map WAD.
5. The new override set's entry count fits the reserved capacity - with slack at
   zero, that means the entry set is unchanged.

Any failure drops the WAD onto the full-rebuild path, which is the same code
that wrote it the first time. There is no repair path to get wrong.

The rewrite itself partitions the new override set against the record: overrides
whose content hash is unchanged have their compressed bytes lifted straight out
of the old tail - never re-read from the mod, never recompressed - and the rest
go through pass 2. The file is then truncated at the tail offset, the new tail
appended, and the chunk count and TOC written at the front.

### Crash safety

Every WAD about to be rewritten in place is added to `dirty_wads` and the state
file is saved **once**, atomically, before any byte is touched; the flags are
cleared when the rewrites succeed. A build killed part-way leaves its WADs
marked, and the next build rebuilds them in full.

Marking is batched rather than per-WAD on purpose: over-invalidating costs a few
extra full rebuilds - the designed fallback anyway - and keeps serialized state
writes out of the parallel patch loop.

This rests on builds running before the game launches, which is how the
consuming apps drive it. Nothing in this crate reads an overlay WAD during a
build.

## Invariants

1. A chunk routed to several WADs has byte-identical compressed data in every
   output of the same build. The game validates a shared chunk by its compressed
   checksum, so divergent copies crash the client. Compressing once per content
   hash makes this structural rather than a bet on the compressor being
   deterministic; bytes reused from an old tail seed the same memo, so they hold
   even across a zstd version change.
2. The TOC is strictly ascending by path hash, the chunk count matches the
   entries, and every entry's data range is inside the file.
3. The source WAD's signature and checksum reach every rebuild of its overlay.
4. The builder writes only under `overlay_root`, and never opens a game WAD for
   writing.
5. Every trust decision has a full-rebuild fallback.

## Deliberately absent

Two things earlier drafts of this design called for, which the crate does not
have and is not waiting on.

**No health check or repair.** There is no `validate()`, no `repair()`, no
overlay-health type. Verification happens only where a build is about to trust
something, and its remedy is always the full rebuild that would have run anyway.
A separate repair path would be a second implementation of the write path, with
its own bugs, guarding a case the fallback already covers.

**No conflict detection.** `OverlayBuildResult::conflicts` is always empty.
Overlapping overrides resolve by load order - the first mod in the list wins -
and nothing reports the overlap. The linked-bin pre-flight is the only
cross-mod advisory the build produces.

## State files

All three live in the state directory, which is typically the profile folder.

| File                 | Contents                                                       | Invalidated by                     |
| -------------------- | -------------------------------------------------------------- | ---------------------------------- |
| `overlay.json`       | Build configuration, per-WAD fingerprints, layout records, dirty flags | Schema version bump, game patch |
| `game_index.bin`     | Filename and hash indexes over the game's WADs                 | Game fingerprint change            |
| `override_meta.bin`  | Per-mod pass-1 metadata                                        | Mod content fingerprint, game patch |

`overlay.json` is written through a temp file and renamed, so a crash mid-write
leaves the previous state readable rather than a truncated one. A state file
that will not parse is treated as no state at all: it costs one full rebuild,
which is better than refusing to build because a cache is unreadable.

The schema version is bumped whenever the format changes *or* the build
semantics change such that WADs already on disk may not match what a fresh build
would produce. Old state fails the version check, which forces the one clean
rebuild that also migrates every WAD to the current layout. There is no
migration code.

## Measuring

`cargo run --release -p ltk_overlay --example overlay_bench` times a real
install against a fixture synthesized from the install's own chunks. Synthetic
small-WAD benchmarks measure allocator noise rather than the copy-versus-tail
effect that dominates here, so there are no criterion benches or CI benchmarks;
the harness is run by hand and its numbers pasted into a PR. See the example's
own documentation for the environment variables it takes.
