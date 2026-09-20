# ADR-0024: Pass-through decode size from the bytes

- **Status:** Accepted
- **Date:** 2026-09-20
- **Crates:** `ltk_overlay`
- **Related:** #257, ADR-0001,
  `docs/overlay-builder-design.md`

## Context and problem statement

A WAD TOC entry carries a compressed size, an uncompressed size and a checksum. The client
allocates a buffer from the uncompressed size and decodes the compressed bytes into it. A
TOC whose uncompressed size disagrees with what the bytes decode to makes the client write
past that buffer.

`EncodedChunk::pass_through` copies a mod container's stored bytes into the overlay without
decoding them. ADR-0001 settled the checksum: it is recomputed over the bytes in flight and
the source's claim is a hint. The stored arm derives the uncompressed size the same way,
from the byte count. The zstd arm copied `CompressedChunk::uncompressed_size` out of the
source TOC and wrote it into the overlay TOC. `ensure_chunk_fits` only rejects a size past
`u32::MAX`.

`FantomeContent` is the only non-test implementor of `read_wad_override_compressed`, so this
path is fed exclusively by the container class ADR-0001 exists because of.

A zstd frame header carries the content size when the encoder knew it. Measured on the
installation and the mod library of 2026-09-20:

| Corpus | zstd chunks | Frame states a size | Size agrees with the TOC | More than one frame |
| --- | --- | --- | --- | --- |
| `DATA/FINAL/Champions` of the installed patch | 20,527 | 20,527 | 20,527 | 0 |
| 8 `.fantome` archives, packed WADs | 16,779 | 14,790 | 14,790 | 0 |

Riot's compressor pledges the source size. A container built with a streaming encoder does
not, which is 11.9% of the mod chunks measured. `zstd::Encoder`, which this crate and
`ltk_wad::WadBuilder` both use, is a streaming encoder.

## Decision drivers

- A number the overlay TOC carries is derived from the bytes it describes.
- The pass-through exists to skip compression, which is what costs.
- A container that ships a wrong number produces a correct overlay, not a failed build.

## Considered options

1. **Read the frame header, and count by decoding where it states nothing.**
2. **Read the frame header, and decline the pass-through where it states nothing.** The
   chunk falls through to the ordinary path, which decodes it and compresses it again.
3. **Read the frame header, and fall back to the source TOC where it states nothing.**
4. **Leave the source TOC's number in place.**

## Decision

**The uncompressed size a pass-through writes is derived from the bytes, never from the
source TOC.** A stored chunk's is its own byte count. A zstd chunk's is what its frame
header states, and for a header that states nothing it is the count of a decode that keeps
no bytes. A disagreement with the source TOC is a warning and never a failed build, the
treatment ADR-0001 gives the checksum. Bytes that do not decode at all are not passed
through, and the chunk falls through to the ordinary read-and-compress path where the
failure is reported. The count stops at `u32::MAX`, the largest value the TOC's size field
holds, so a chunk whose ratio would hold the decoder for a long time is refused at that
point rather than decoded to its end.

## Consequences

- **Positive:** a container's claimed decode size cannot reach a WAD the game loads.
- **Positive:** 88% of the chunks measured need no decode at all, and none needs
  compression.
- **Negative:** the remaining chunks are decoded, in the sequential loop of
  `resolve_provider_overrides`. Decoding runs several times faster than the level-3
  compression a pass-through skips, and the decode keeps only the decoder's window, so the
  path stays worth taking; a container whose every frame is headless pays a serial decode of
  its whole override set on the builds that resolve it.
- **Negative:** the doc claim that a pass-through never decodes no longer holds for every
  chunk.
- **Revisit when:** `ltk_wad::WadBuilder` pledges the source size, which would leave the
  decode path reachable only from a foreign container.

## Pros and cons of the options

### Read the frame header, count by decoding where it states nothing

- Good: every chunk gets a size derived from its own bytes.
- Good: compression, the expensive half, is skipped for every chunk either way.
- Bad: a decode pass over the chunks whose header states nothing.

### Read the frame header, decline where it states nothing

- Good: no decode inside the pass-through.
- Bad: the declined chunk is decoded *and* compressed by the fallback, which is strictly
  more work than counting.
- Bad: 11.9% of the mod chunks measured take that fallback.

### Read the frame header, fall back to the source TOC

- Good: no decode, no extra work.
- Bad: the case the fix exists for is exactly a container whose TOC lies, and this trusts
  it whenever the frame is silent.

### Leave the source TOC's number in place

- Good: no change.
- Bad: a wrong number passes every integrity check the build runs and fails inside the
  client's decompressor at load.
