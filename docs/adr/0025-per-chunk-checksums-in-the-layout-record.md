# ADR-0025: Per-chunk checksums in the layout record

- **Status:** Accepted
- **Date:** 2026-09-20
- **Crates:** `ltk_overlay`
- **Related:** #257, ADR-0001, ADR-0024,
  `docs/overlay-builder-design.md`

## Context and problem statement

The game validates a chunk two mounted WADs share against its compressed checksum. Two
overlay WADs carrying one path hash under two checksums crash the client.

Invariant 1 of `docs/overlay-builder-design.md` covers one build: a content is compressed
once per content hash, and every WAD holding it gets that one buffer. It covers a WAD taking
the tail-rewrite path, whose reused bytes seed the same memo. It does not cover a WAD taking
the **full-rebuild** path beside a WAD **reused untouched**. The full rebuild seeds from
nothing; the reused WAD keeps the previous build's bytes.

Two things make this build's encoding differ from the previous build's:

- **The memo winner depends on arrival order.** `OverrideCompressor::supply_prepared`
  memoizes by content hash, so two path hashes carrying identical bytes - one passed through
  from a packed `.fantome`, one read loose - share whichever encoding arrived first. Arrival
  order is `HashSet` and `HashMap` iteration order in `ChunkSources::classify` and
  `resolve_provider_overrides`, randomized per process. A path hash that was `Stored` in one
  build can be `Zstd` in the next.
- **zstd output drifts.** A `zstd` dependency bump moves level-3 output, and nothing ties
  the state's `CURRENT_VERSION` to the compressor version.

The scenario: chunk H lives in `Aatrox.wad.client` and `Map11.wad.client`, and routing fans
it to both. A mod adds a file to Aatrox. Aatrox's fingerprint changes, its entry count
changes, slack is 0 so the tail rewrite is refused, and it is fully rebuilt with H
recompressed from scratch. Map11's fingerprint is unchanged, so it is reused with the old
bytes for H.

`WadLayoutRecord.overrides` recorded `path_hash -> content_hash`, which answers whether the
next build wants the same content and says nothing about how the bytes were encoded.

## Decision drivers

- Every overlay WAD holding one path hash holds one encoding of it.
- A build rebuilds the WADs it has to, and no more.
- A record describes the file on disk.

## Considered options

1. **Record the compressed checksum per override, and rebuild a reused WAD whose record
   disagrees with this build.**
2. **Widen the rebuild set.** When any WAD is rebuilt, pull in every WAD holding one of its
   override hashes, to a fixpoint. No state change, no second resolution.
3. **Seed the memo from the reused WAD's file.** Lift the old encoding out of the reused
   overlay WAD and make the rebuilt WAD write that, generalizing the tail rewrite's
   `reuse_unchanged_overrides` across WADs.
4. **Tie the state version to the compressor version.** Covers the zstd bump and not the
   memo ordering.

## Decision

**`WadLayoutRecord.overrides` records an `OverrideRecord` per override: the content hash it
came from and the `xxh3_64` of the compressed bytes the file holds. A reused WAD whose
record disagrees with the encoding this build produced for the same path hash is rebuilt.**

The state format version moves to 9, so a state written before this loses its layout
records and costs one full rebuild.

The rebuild set is decided in two steps. The first resolution pass encodes what the WADs
already chosen for rebuild need. `diverged_reuses` then compares each reused WAD's recorded
checksums against that pass's encodings; a WAD that disagrees, and a reused WAD with no
layout record that shares any chunk with a rebuilt one, moves into the rebuild set. A second
resolution pass covers the additions, seeded from the first pass's encodings through
`OverrideCompressor::seed`, so a content encoded in the first pass keeps that encoding in
the second and both passes write one encoding of a shared chunk.

## Consequences

- **Positive:** the shared-chunk invariant holds across builds, not only within one.
- **Positive:** only a WAD that actually disagrees is rebuilt. A build where every shared
  chunk still encodes the same way reuses everything it would have reused before.
- **Negative:** a `zstd` bump costs one build in which every WAD holding a shared chunk is
  rebuilt. That is the correct cost and it is paid once.
- **Negative:** the resolution pass can run twice, and the second pass re-walks the mod
  providers for the additions' own chunks.
- **Negative:** `overlay.json` grows by eight bytes per override.
- **Revisit when:** the memo's winner stops depending on iteration order, which would leave
  the compressor version as the only source of drift and make option 4 sufficient.

## Pros and cons of the options

### Record the compressed checksum, rebuild on a disagreement

- Good: exact. A WAD is rebuilt when and only when its bytes would diverge.
- Good: the record describes the file, which is what every other field of it does.
- Bad: a state format change, and a resolution pass that can run twice.

### Widen the rebuild set

- Good: about forty lines, no state change, no second pass.
- Bad: rebuilds every WAD sharing a chunk with a rebuilt one on every build, whether or not
  anything would have diverged. A mod touching a chunk several map WADs carry pays it every
  time.

### Seed the memo from the reused WAD's file

- Good: nothing extra is rebuilt at all.
- Bad: the build mounts reused overlay WADs and lifts bytes out of them, which is a read
  path with its own corruption fallbacks.
- Bad: it carries an old encoding forward indefinitely, so a `zstd` bump never reaches the
  chunks that most want it.

### Tie the state version to the compressor version

- Good: trivial.
- Bad: leaves the memo ordering, which is the reachable half - it needs no dependency change
  at all.
