# ltk_meta on crates.io versus the manager's git pin

Question: does the published `ltk_meta` 0.8.2 expose the streaming bin reader the LTK Manager's
object index build uses, and which of the object index's `ltk_meta`, `ltk_file`, `ltk_wad` and
`ltk_hash` calls need the league-toolkit git rev `b800ad506d237c1c02128268389ceafaea7da281`.

## Summary

- The object index **build** (`object_index/build.rs`) compiles against `ltk_meta` 0.8.2
  unchanged. `BinStream::mount`, `BinStream::entries`, `ObjectEntry { path_hash, class_hash,
  offset, size }`, `BinOverride::from_reader`, `BinOverride::objects` and `ltk_meta::Error` are
  in the published crate with the same signatures as the git rev. `stream/toc.rs` and
  `stream/cursor.rs` are byte-identical between the two.
- The `PTCH` fallback is the manager's own code, not a crate feature. `BinStream::mount` rejects a
  `PTCH` with `Error::UnexpectedBinKind` in both trees. The build reads the magic, and a `PTCH` is
  read whole through `BinOverride::from_reader`. Neither tree has a streaming `PTCH` reader.
- Only the object index **walk** (`object_index/walk.rs`, the property scan behind
  `walk/run.rs`) needs the git rev: `ltk_meta::walk::{Child, Leaf, Node, OwnedNode, TreeNode,
  TreeValue, Visit, Visitor}` and `ObjectView::walk` are absent from 0.8.2. `BinStream::write_patched`
  and `BinDelta` are absent too; the object index does not call them, `bin_document/` and
  `problems/` do.
- Every `ltk_file`, `ltk_wad` and `ltk_hash` item the object index uses is on crates.io:
  `ltk_file` 0.2.11, `ltk_wad` 0.5.4, `ltk_hash` 0.4.0. These are the versions the manager, this
  repo and `ltk_meta` 0.8.2 all resolve to. The manager's `ltk_hash` comes from the git rev under
  `[patch.crates-io]`, and its `src/` is byte-identical to the crates.io 0.4.0 tarball.
- No dependency graph holds two `WadHash` or `BinHash` types. Each side's lock has exactly one
  `ltk_hash` 0.4.0. Consuming the manager's object index code from this repo needs the same
  `[patch.crates-io]` for `ltk_meta`, `ltk_hash`, `ltk_io_ext`, `ltk_primitives` until PR 227
  (`feat/value-walk`, open, head `b800ad5`) ships in a release; 0.8.2 is the newest published
  `ltk_meta`.

## Item table

Manager paths are under `/mnt/c/dev/ltk/ltk-manager/crates/ltk-manager-core/src/`. "Same" means
the item is present in `ltk_meta` 0.8.2 with the signature the manager calls, identical to the
git rev.

| Item | Manager use site | 0.8.2 | Git rev `b800ad5` |
| --- | --- | --- | --- |
| `ltk_meta::stream::BinStream::mount(source: R) -> Result<Self, Error>` | `object_index/build.rs:224`, `object_index/walk.rs:106` | Same (`stream/prop.rs:88`) | Same |
| `ltk_meta::stream::BinStream::entries(&mut self) -> Entries<'_, R, M>` | `object_index/build.rs:225` | Same (`stream/prop.rs:188`) | Same |
| `Entries: Iterator<Item = Result<ObjectEntry, Error>>` | `object_index/build.rs:226` | Same (`stream/cursor.rs:77`) | Same |
| `ltk_meta::stream::ObjectEntry { path_hash: BinHash, class_hash: BinHash, offset: u64, size: u32 }` | `object_index/build.rs:227-230` (`entry.path_hash`, `entry.class_hash`) | Same (`stream/toc.rs:10`); file identical | Same |
| `ltk_meta::stream::BinStream::objects(&mut self) -> Objects<'_, R, M>` | `object_index/walk.rs:107` | Same (`stream/prop.rs:181`) | Same |
| `Objects::next(&mut self) -> Result<Option<ObjectStream>, Error>` | `object_index/walk.rs:108` | Same (`stream/cursor.rs:41`) | Same |
| `ObjectStream::view(&mut self) -> Result<ObjectView<'_, M>, Error>` | `object_index/walk.rs:109` | Same (`stream/cursor.rs:161`) | Same |
| `ObjectView::path_hash()`, `ObjectView::class_hash()` | `object_index/walk.rs:110` | Same (`stream/view.rs:94,100`) | Same |
| `ObjectView::walk(&self, visitor) -> Result<WalkOutcome, W::Error>` | `object_index/walk.rs:111` | **Absent** | Present (`walk.rs:620`) |
| `ltk_meta::BinOverride<M>` (re-export of `data_override::BinOverride`) | `object_index/build.rs:13,213`, `object_index/walk.rs:13,98` | Same (`lib.rs:219`, `data_override.rs:53`) | Same |
| `BinOverride::from_reader(reader: &mut R) -> Result<Self, Error>` | `object_index/build.rs:213`, `object_index/walk.rs:98` | Same (`data_override/read.rs:45`) | Same |
| `BinOverride::objects: IndexMap<BinHash, BinObject<M>>`; `BinObject::{path_hash, class_hash}` | `object_index/build.rs:214-218`, `object_index/walk.rs:99-100` | Same (`data_override.rs:61`, `tree/object.rs:50`) | Same |
| `ltk_meta::Error` | `object_index/build.rs:174,205`, `object_index/walk.rs:13` | Same (`lib.rs:234`); variants `DeltaLegacyNumbering`, `DeltaMissingObject`, `DeltaDuplicateObject` absent | Same plus the three `Delta*` variants |
| `ltk_meta::PropertyValueEnum` | `object_index/walk.rs:13,100` | Same (`lib.rs:213`) | Same |
| `ltk_meta::property::{Kind, NoMeta}` | `object_index/walk.rs:10`, `object_index/tests.rs:12`, `object_index/tests/walk.rs:7` | Same | Same |
| `ltk_meta::walk::{Child, Leaf, Node, OwnedNode, TreeNode, TreeValue, Visit, Visitor}` | `object_index/walk.rs:12` and the `Visitor` impl at `walk.rs:158` | **Absent** (no `walk` module in `lib.rs`) | Present (`lib.rs:404`, `walk.rs`, `walk/tree.rs`, `walk/owned.rs`) |
| `OwnedNode::from(&BinObject)` | `object_index/walk.rs:101` | **Absent** | Present (`walk/owned.rs:21`) |
| `Node::class_hash()` | `object_index/walk.rs:167,191,212` | **Absent** | Present (`walk.rs:243`) |
| `BinStream::write_patched(&mut self, delta: &BinDelta<M>, out: &mut W)` | Not in `object_index/`; `bin_document/`, `problems/`, `material/` | **Absent** | Present (`stream/delta.rs:183`) |
| `ltk_meta::stream::BinDelta` | Not in `object_index/` | **Absent** | Present (`stream/delta.rs:51`) |
| `ltk_meta::{Bin, BinObject, PropertyPatch}`, `ltk_meta::path::PropertyPath`, `ltk_meta::property::values` | `object_index/tests.rs:11-14` (tests only) | Same | Same |
| `ltk_file::LeagueFileKind::identify_from_bytes(data: &[u8]) -> LeagueFileKind` | `object_index/build.rs:129` | Present in `ltk_file` 0.2.11 (`src/kind.rs:146`) | Same crate, same version |
| `ltk_file::LeagueFileKind::{PropertyBin, PropertyBinOverride}` | `object_index/build.rs:130` | Present (`src/kind.rs:18-19`) | Same |
| `ltk_file::MAX_MAGIC_SIZE` | `object_index/build.rs:11,121` | Present (`src/pattern.rs`, re-exported at crate root) | Same |
| `ltk_wad::Wad::mount(source) -> Result<Wad<TSource>, WadError>` | `object_index/build.rs:63`, `object_index/walk/run.rs:323`, `game_wads.rs:170,293` | Present in `ltk_wad` 0.5.4 (`src/lib.rs:261`) | Same |
| `Wad::chunks().get(WadHash) -> Option<&WadChunk>` | `object_index/build.rs:117,146`, `game_wads.rs:172,265` | Present (`src/lib.rs:257`, `src/chunks.rs:46`) | Same |
| `Wad::load_chunk_decompressed(&WadChunk) -> Result<Box<[u8]>, WadError>` | `game_wads.rs:268` | Present (`src/lib.rs:373`) | Same |
| `Wad::load_chunk_raw_prefix`, `Wad::subchunk_toc` | `game_wads.rs:337,340` (`chunk_head`) | Present (`src/lib.rs:355,452`) | Same |
| `ltk_wad::ChunkDecoder::new()`, `ChunkDecoder::decompress_chunk_prefix` | `object_index/build.rs:78,115`, `game_wads.rs:330,340` | Present (`src/decoder.rs:223,236,340`) | Same |
| `ltk_wad::hex_name(path_hash: WadHash) -> String` | `object_index/build.rs:104,124`, `object_index/search.rs:8`, `object_index/walk/run.rs:297,351,358` | Present (`src/extractor/resolver.rs:156`) | Same |
| `ltk_wad::WadHash` (re-export of `ltk_hash::WadHash(pub u64)`) | `object_index/build.rs:15`, `names.rs:11`, `references.rs:8`, `wire.rs:8`, `game_wads.rs:12` | Present (`ltk_wad/src/lib.rs:205`, `ltk_hash/src/lib.rs:26`) | Same |
| `ltk_wad::{WadChunk, WadError}` | `game_wads.rs:12` | Present (`src/chunk.rs:36`, `src/error.rs:9`) | Same |
| `ltk_wad::{WadBuilder, WadChunkBuilder}` | `object_index/tests.rs:15` (tests only) | Present (`src/builder.rs:48,256`) | Same |
| `ltk_hash::BinHash(pub u32)` | `build.rs`, `browse.rs`, `find.rs`, `names.rs`, `references.rs`, `search.rs`, `spells.rs`, `walk.rs`, `walk/run.rs` | Present in `ltk_hash` 0.4.0 (`src/lib.rs:98`) | Same; `src/` byte-identical |
| `ltk_hash::Hash` trait | `object_index/spells.rs:3`, `object_index/tests.rs:9` | Present (`src/lib.rs:18`) | Same |

`ltk_hashdb::LayeredHashDb` (`object_index/tests.rs:10`, `game_wads.rs:11`) comes from the mimir
repository at rev `670196996ae219cbae5e64d40667363ff19a028f`, not from league-toolkit, and is out
of scope.

## What the git rev adds over 0.8.2

`diff -r` of `ltk_meta/src` between the 0.8.2 tarball and the rev: 240 changed lines, all
additive.

- `walk.rs`, `walk/{mutable,owned,tree,view}.rs`: the `walk` module (`Visitor`, `VisitorMut`,
  `Node`, `Trail`, `TreeValue`, `TreeNode`, `Leaf`, `Child`, `OwnedNode`, `Visit`, `WalkOutcome`)
  and `walk` methods on `Bin`, `BinObject`, `BinOverride`, `ObjectView`, `ObjectStream`,
  `BinStream`.
- `stream/delta.rs`: `BinDelta` and `BinStream::write_patched`; `concrete::BinDelta` alias.
- `error.rs`: `DeltaLegacyNumbering`, `DeltaMissingObject`, `DeltaDuplicateObject`.
- `stream/prop.rs`: `pub(crate) fn copy_range`. `stream/view.rs`: `pub(crate) fn as_struct`.
  `property/values/{container,map}.rs`: `pub(crate)` mutable iterators.
- `lib.rs`: a "Walking a bin" doc section.

The crate version at the rev is `0.8.2`, the same number as the release. The rev is the head of
branch `feat/value-walk`, PR 227 (state `open`, not merged); `origin/main` is at `eab99ad`
(release-plz merge of 2026-09-07) and does not contain the rev. Commits on `crates/ltk_meta`
between tag `ltk_meta-v0.8.2` (`5cea6e3`) and the rev: `d46c0de` value walk, `56e6f6c`
non-exhaustive Leaf, `4b11f1e` node predicates under walk, `e4b0e8e` walk examples, `171c23c`
mutable walk, `1dee55f` delta write-back, `fbb9ea0` defer leaves and enforce output format,
`b800ad5` expose borrowed walk values.

## Version table

| Crate | Manager `Cargo.toml` requirement | Manager `Cargo.lock` resolution | league-mod `Cargo.toml` requirement | league-mod `Cargo.lock` resolution | `ltk_meta` 0.8.2 `Cargo.toml` requirement | Git rev crate version | crates.io newest |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `ltk_meta` | `0.8.2` + `[patch.crates-io]` git rev | 0.8.2 `git+...?rev=b800ad5` | `0.8.2` (`crates/ltk_game_data`) | 0.8.2 registry | - | 0.8.2 | 0.8.2 (2026-09-07) |
| `ltk_hash` | `0.4.0` + `[patch.crates-io]` git rev | 0.4.0 `git+...?rev=b800ad5` (single entry) | `0.4` (workspace) | 0.4.0 registry (single entry) | `0.4.0` | 0.4.0 | 0.4.0 (2026-07-12) |
| `ltk_wad` | `0.5.4` | 0.5.4 registry | `0.5.4` (workspace) | 0.5.4 registry | `0.5.4` (dev-dependency) | 0.5.4 | 0.5.4 (2026-08-30) |
| `ltk_file` | `0.2.11` | 0.2.11 registry | `0.2.8` (`ltk_overlay`, `ltk_fantome`, `ltk_mod_project`) | 0.2.11 registry | - (`ltk_wad` requires `0.2.11`) | 0.2.11 | 0.2.11 (2026-08-25) |
| `ltk_io_ext` | `[patch.crates-io]` git rev | 0.4.4 git | - | 0.4.4 registry | `0.4.4` | 0.4.4 | 0.4.5 (2026-09-14) |
| `ltk_primitives` | `[patch.crates-io]` git rev | 0.3.5 git | - | 0.3.5 registry | `0.3.5` | 0.3.5 | 0.3.5 (2026-07-12) |

`ltk_wad` 0.5.4 requires `ltk_hash = "0.4.0"` and `ltk_file = "0.2.11"`. `ltk_meta` 0.8.2
requires `ltk_hash = "0.4.0"`, `ltk_io_ext = "0.4.4"`, `ltk_primitives = "0.3.5"`. The git rev's
`ltk_meta/Cargo.toml` requires the same three by `path` with the same version numbers.

Mismatch check. Every `ltk_hash` requirement on both sides is `0.4.0`, and each lock file holds
exactly one `ltk_hash` package. The `[patch.crates-io]` entry replaces the registry `ltk_hash`
for the whole manager graph, `ltk_wad` included, which is what keeps `WadHash` a single type
there. A workspace that adds the git `ltk_meta` as a plain `git = ...` dependency without the
patch would hold two `ltk_hash` 0.4.0 packages (one registry, one git) and two `BinHash` /
`WadHash` types; the manager's comment at `Cargo.toml:92-97` names this as the reason for the
patch form. The crates.io `ltk_hash` 0.4.0 `src/` is byte-identical to the rev's
`crates/ltk_hash/src`, so the two types differ only by package identity, not by code.

`ltk_io_ext` 0.4.5 is on crates.io (2026-09-14) while both sides resolve 0.4.4; semver-compatible,
no effect on the hash types.

## Sources

- Ticket: `/home/crauzer/dev/lol/league-mod/.scratch/game-index/issues/01-ltk-meta-pin.md`
- Manager use sites: `/mnt/c/dev/ltk/ltk-manager/crates/ltk-manager-core/src/object_index/build.rs`
  (lines 11-15, 63, 78, 104-130, 146, 174-231), `object_index/walk.rs` (lines 10-13, 93-114,
  158-168), `object_index/walk/run.rs` (lines 12-13, 297, 323, 351, 358), `object_index/names.rs`,
  `browse.rs`, `search.rs`, `find.rs`, `references.rs`, `spells.rs`, `wire.rs`, `tests.rs`,
  `tests/walk.rs`; `/mnt/c/dev/ltk/ltk-manager/crates/ltk-manager-core/src/game_wads.rs`
  (lines 11-12, 170-172, 260-268, 293, 327-340)
- Manager pins: `/mnt/c/dev/ltk/ltk-manager/Cargo.toml` lines 29-38 and 92-102;
  `/mnt/c/dev/ltk/ltk-manager/Cargo.lock` packages `ltk_file` (line 3107), `ltk_hash` (3117),
  `ltk_meta` (3191), `ltk_wad` (3356)
- league-mod pins: `/home/crauzer/dev/lol/league-mod/Cargo.toml` lines 18-19;
  `/home/crauzer/dev/lol/league-mod/crates/ltk_game_data/Cargo.toml` line 16;
  `/home/crauzer/dev/lol/league-mod/crates/{ltk_overlay,ltk_fantome,ltk_mod_project}/Cargo.toml`;
  `/home/crauzer/dev/lol/league-mod/Cargo.lock` packages `ltk_file` (1407), `ltk_hash` (1430),
  `ltk_io_ext` (1453), `ltk_meta` (1465), `ltk_primitives` (1573), `ltk_wad` (1596)
- Published `ltk_meta` 0.8.2: `https://crates.io/api/v1/crates/ltk_meta/0.8.2/download`,
  unpacked at `/tmp/ltk_meta_crate/ltk_meta-0.8.2/` (also cached at
  `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ltk_meta-0.8.2/`); `Cargo.toml`
  `[dependencies]` block; `src/lib.rs` lines 210-236; `src/stream.rs` lines 42-65;
  `src/stream/prop.rs` lines 88, 181, 188; `src/stream/cursor.rs` lines 41, 65-95, 112-179;
  `src/stream/toc.rs` lines 10-28; `src/stream/view.rs` lines 94-123;
  `src/data_override.rs` lines 53-61; `src/data_override/read.rs` line 45
- Published `ltk_file` 0.2.11, `ltk_wad` 0.5.4, `ltk_hash` 0.4.0:
  `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/{ltk_file-0.2.11,ltk_wad-0.5.4,ltk_hash-0.4.0}/`;
  `ltk_file/src/kind.rs` lines 18-19, 146; `ltk_wad/src/lib.rs` lines 205, 257, 261, 355, 373,
  452; `ltk_wad/src/decoder.rs` lines 223, 236, 340; `ltk_wad/src/extractor/resolver.rs` line
  156; `ltk_wad/src/chunks.rs` line 46; `ltk_wad/src/builder.rs` lines 48, 256; `ltk_wad/Cargo.toml`
  `[dependencies.ltk_file]`, `[dependencies.ltk_hash]`; `ltk_hash/src/lib.rs` lines 18, 26, 98
- crates.io version index: `https://crates.io/api/v1/crates/ltk_meta` (max 0.8.2, released
  2026-09-07), `.../ltk_wad` (0.5.4), `.../ltk_hash` (0.4.0), `.../ltk_file` (0.2.11),
  `.../ltk_io_ext` (0.4.5), `.../ltk_primitives` (0.3.5); queried 2026-09-16
- Git rev: `https://github.com/LeagueToolkit/league-toolkit`, commit
  `b800ad506d237c1c02128268389ceafaea7da281` ("feat(ltk_meta): expose borrowed walk values",
  2026-09-15), checked out at `/tmp/league-toolkit`; `crates/ltk_meta/Cargo.toml`;
  `crates/ltk_meta/src/lib.rs` line 404; `crates/ltk_meta/src/walk.rs` lines 90-131, 220-262,
  565-663; `crates/ltk_meta/src/walk/{tree,owned}.rs`; `crates/ltk_meta/src/stream/delta.rs`
  lines 51, 183; `crates/ltk_meta/src/error.rs`; `git branch -r --contains` =
  `origin/feat/value-walk`; `git merge-base --is-ancestor <rev> origin/main` = false;
  `git log ltk_meta-v0.8.2..<rev> -- crates/ltk_meta crates/ltk_hash`
- PR 227: `https://api.github.com/repos/LeagueToolkit/league-toolkit/pulls/227` (title
  "feat(ltk_meta): value walk", state open, head `feat/value-walk` at `b800ad5`)
- Comparison: `diff -rq /tmp/ltk_meta_crate/ltk_meta-0.8.2/src /tmp/league-toolkit/crates/ltk_meta/src`;
  `diff -rq ~/.cargo/registry/src/*/ltk_hash-0.4.0/src /tmp/league-toolkit/crates/ltk_hash/src`
  (no output, exit 0)
