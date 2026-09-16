---
title: "Game index: ltk_meta reader available on crates.io"
labels: area:research
---

Part of the game index map.
Type: research
Status: resolved

## Question

Does the published `ltk_meta` (0.8.2 on crates.io) expose the streaming bin reader the manager's
object index build uses, `BinStream::mount` and `BinStream::entries` yielding
`(path_hash, class_hash, offset, size)` per object, and the `PTCH` fallback the build relies on?
The manager pins `ltk_meta`, `ltk_hash` at league-toolkit rev `b800ad506d237c1c02128268389ceafaea7da281`
and comments say `ltk_meta::walk` and `BinStream::write_patched` are unpublished. State which of
the object index's `ltk_meta` calls (see `object_index/build.rs`) compile against 0.8.2, which
need the git rev, and which `ltk_wad`/`ltk_hash`/`ltk_file` versions each side uses.

## Answer

- The object index build (`object_index/build.rs`) compiles against `ltk_meta` 0.8.2 unchanged:
  `BinStream::mount`, `BinStream::entries`, `ObjectEntry { path_hash, class_hash, offset, size }`,
  `BinOverride::from_reader`, `BinOverride::objects` and `ltk_meta::Error` are in the published
  crate with the same signatures as the git rev.
- The `PTCH` fallback is the manager's own code. `BinStream::mount` rejects a `PTCH` with
  `Error::UnexpectedBinKind` in both trees; the build reads the magic and reads a `PTCH` whole
  through `BinOverride::from_reader`. Neither tree has a streaming `PTCH` reader.
- Only the object index walk (`object_index/walk.rs`) needs the git rev: `ltk_meta::walk::*` and
  `ObjectView::walk` are absent from 0.8.2. `BinStream::write_patched` and `BinDelta` are absent
  too; the object index does not call them, `bin_document/` and `problems/` do.
- Every `ltk_file`, `ltk_wad`, `ltk_hash` item the object index uses is on crates.io at the
  versions both sides resolve: `ltk_file` 0.2.11, `ltk_wad` 0.5.4, `ltk_hash` 0.4.0. The manager's
  git `ltk_hash` is byte-identical to the crates.io 0.4.0 source.
- Each lock holds one `ltk_hash`, so no graph carries two `WadHash`/`BinHash` types. Consuming the
  walk from this repo needs the manager's `[patch.crates-io]` for `ltk_meta`, `ltk_hash`,
  `ltk_io_ext`, `ltk_primitives` until PR 227 (`feat/value-walk`, open) is released; 0.8.2 is the
  newest published `ltk_meta`.

See [findings](../research/ltk-meta-pin.md).
