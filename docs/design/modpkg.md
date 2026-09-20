# The modpkg package format

The format specification for `.modpkg`, the LeagueToolkit mod package: why the format exists,
the byte layout of format version 1, the metadata schema, and what a reader and a writer are
required to do.

**This document states what is true.** Where the reference implementation and a section here
disagree, one of the two is the bug and gets fixed. The reference implementation is the
`ltk_modpkg` crate in the [league-mod](https://github.com/LeagueToolkit/league-mod) repository
([appendix B](#appendix-b)). Two things this document does not hold:

- **Why an option was chosen over the alternatives it beat** - `docs/adr/`, cited as ADR-NNNN.
- **The embedded hashtable standard** - the table grammar, the categories and the algorithms
  are the wiki's [Embedded Hashtables](https://wiki.leaguetoolkit.dev/reference/mod-packages/hashtables/)
  page. [Section 10.5](#s10.5) states only how a modpkg carries them.

## Contents

- [1. Summary](#s1)
- [2. Motivation](#s2)
- [3. Vocabulary](#s3)
- [4. Conventions](#s4)
- [5. Container layout](#s5)
- [6. Names and identity](#s6)
- [7. Chunks](#s7)
- [8. Layers](#s8)
- [9. WAD targets](#s9)
- [10. Meta chunks](#s10)
- [11. Metadata schema](#s11)
- [12. Reader requirements](#s12)
- [13. Writer requirements](#s13)
- [14. Extraction layout](#s14)
- [15. Fantome interoperability](#s15)
- [16. Versioning](#s16)
- [17. Rules](#s17)
- [Appendix A. A worked example](#appendix-a)
- [Appendix B. Reference implementation](#appendix-b)

## <a id="s1"></a>1. Summary

A `.modpkg` file is a single binary container holding one mod: its content, organized by layer
and by target WAD, and its metadata. It is designed and maintained by
[LeagueToolkit](https://github.com/LeagueToolkit), the organization that, under its earlier name
Fantome, designed the `.fantome` format that preceded it. It is the native package of the
LeagueToolkit toolchain: the Creator Workshop in LTK Manager packs a mod project into one, and
LTK Manager installs one into a library and builds game overlays from it.

The container is a table of contents over a run of chunks. Every file of the mod is one chunk,
addressed by the 64-bit hash of its path inside its target WAD, exactly as the game addresses a
chunk inside a `.wad.client` archive. A chunk's stored bytes are either raw or one Zstandard
frame, carry an XXH3 checksum, and have the shape a game WAD stores. An overlay builder lifts
them into a game WAD without re-encoding. Layers, WAD targets and paths sit in three string
tables ahead of the contents; a chunk record refers into each by position. The mod's metadata
is itself a chunk, a MessagePack document under a reserved path, with an additive schema of its
own.

Format version 1 is the only version. [Section 5](#s5) is its byte layout.

## <a id="s2"></a>2. Motivation

### <a id="s2.1"></a>2.1 What a package has to do

A mod package is read far more often than it is written. Every install, every overlay build and
every library listing reads it. The operations a package serves, in the order the toolchain
performs them:

1. **Identify the mod** - name, version, authors, license, thumbnail - without reading its
   content.
2. **List its content** - which layers it has, which WADs each layer touches, which chunks each
   WAD holds - without reading the bytes of any chunk.
3. **Read one chunk** by its path hash, at its position in the file, without reading its
   neighbours.
4. **Copy a chunk into a game WAD.** The game's `.wad.client` stores each chunk as a Zstandard
   frame or raw bytes with an XXH3 checksum of the stored bytes. A package whose stored bytes are
   already in that shape lets an overlay builder copy them through, checksum verified in flight
   (ADR-0001), and a package whose bytes are in any other shape has to be decoded and re-encoded
   for every build.
5. **Round-trip to a mod project.** A package unpacks to the project layout it was packed from
   and packs back to an equivalent package. Nothing the project declares is lost on the way.

### <a id="s2.2"></a>2.2 The fantome format

`.fantome` is a ZIP archive under another extension: metadata in `META/info.json`, content
under `WAD/`, a single content layer. It is the format most League mods are distributed in.
Mod sites host it and every mod manager reads it. LTK Manager supports it in full and it stays
the format to publish to a mod site. Its weaknesses are structural, and the wiki's
[Fantome Package](https://wiki.leaguetoolkit.dev/reference/mod-packages/fantome/) page is the
full account. The facts that bear on the design of a successor:

- **Three shapes of content.** Game files arrive as loose files under `WAD/<name>/`, as one
  pre-built WAD archive at `WAD/<name>`, or as files under `RAW/` with no WAD assignment at all.
  A reader implements all three. A `RAW/` file is placed by indexing the whole game install. A
  packed WAD carries only path hashes: the names of the files inside it survive nowhere in the
  archive.
- **Double compression.** A packed WAD holds Zstandard frames, and the ZIP entry around it is
  Deflate-compressed on top. The outer pass costs pack time and install time and saves nothing
  on data that is already compressed.
- **No random access into content.** Deflate has no seek. A chunk inside a Deflate-compressed
  packed WAD is reachable only after the whole WAD is inflated. A loose file is reachable only
  by name through the ZIP central directory, and its bytes are inflated and re-encoded into a
  WAD chunk at every overlay build.
- **Schemaless metadata.** `info.json` has four fields and no format version. Tools extend it
  independently. A file one manager understands, another silently does not. The LeagueToolkit
  extension is strictly additive; nothing in the format enforces that on anyone else.
- **No conformance at creation.** Nothing validates a fantome when it is written. Archives in
  the field spell `META/` in three casings, start `info.json` with a byte-order mark, and carry
  ZIP CRC32 values that do not match their bytes. Every reader carries the leniency for all of
  it, forever.
- **One layer, flat metadata.** One content layer, one author string, one version string. Layer
  metadata, per-layer string overrides, structured authorship and licensing exist only through
  the extension, and content for any layer but `base` cannot ship at all.

### <a id="s2.3"></a>2.3 What modpkg answers with

Each design choice below answers one item in [section 2.2](#s2.2).

- **One shape of content.** Every file is a chunk, every chunk has a path, a layer and a WAD
  target. There is no packed-WAD form and no unassigned form.
- **One compression, once.** A chunk is stored raw or as one Zstandard frame. There is no outer
  container compression.
- **Random access by design.** A fixed-layout table of contents holds each chunk's absolute
  offset and sizes. Reading one chunk is one seek and one read. The stored bytes and their
  checksum are the shape a game WAD stores, and pass through into an overlay unchanged.
- **Versioned metadata.** The container carries a format version. The metadata document carries
  a schema version and grows only by adding optional keys ([section 16](#s16)).
- **Refused at the door.** A reader refuses a package whose magic, version, table positions or
  names are wrong, whole and before any content is read ([section 12](#s12)). A writer refuses
  to produce one ([section 13](#s13)).
- **Layers and structured metadata are native.** Layers with priorities are a header table.
  Authors have roles, the license is typed, tags, champions and maps are lists, and a layer
  carries string overrides ([section 11](#s11)).

## <a id="s3"></a>3. Vocabulary

Every term this document uses in a specific sense. The definitions agree with the workspace's
`CONTEXT.md`.

**The container**

- **package** - one `.modpkg` file.
- **header** - everything before the table of contents: the magic, the format version, the
  signature, and the three string tables.
- **table of contents**, **TOC** - the array of chunk records ([section 5.6](#s5.6)).
- **chunk record** - one TOC entry: where a chunk's bytes are, how they are stored, and what
  they belong to.
- **chunk** - one file of the mod, as the package stores it. A chunk record plus the bytes it
  points at.
- **meta chunk** - a chunk under the reserved `_meta_/` path that belongs to no layer and no
  WAD ([section 10](#s10)).
- **content chunk** - every chunk that is not a meta chunk.
- **stored bytes** - the bytes a chunk record points at, raw or compressed.
- **content bytes** - a chunk's bytes after decompression: the file as the mod author wrote it.

**Names**

- **chunk path** - a chunk's path *inside its target WAD*. It excludes the layer and the
  `.wad.client` directory. `DATA/Characters/Aatrox/Skins/Skin0/Skin0.bin`, never
  `base/Aatrox.wad.client/DATA/...`.
- **stored path** - a chunk path spelled as its author wrote it, separators normalized to `/`.
  What the path table holds and what an extraction names files by.
- **canonical name** - a name after ASCII-lowercasing. The exact bytes that get hashed.
- **path hash** - the xxHash64 of a chunk path's canonical name. A chunk's identity within a
  layer, and the same value the game keys the chunk by inside a WAD.
- **hex name** - a path hash rendered as sixteen lowercase hex digits, used as a chunk path for
  a chunk whose real path is unknown ([section 6.4](#s6.4)).
- **layer** - a named, prioritized slice of the mod's content. Every package has `base`.
- **WAD target**, **WAD** - the game archive a chunk belongs in, named as the game names the
  file: `Aatrox.wad.client`.
- **slug** - a layer name: non-empty, ASCII lowercase letters, digits and `-`, neither starting
  nor ending with `-`.

**The metadata**

- **metadata document** - the MessagePack map in the `_meta_/info.msgpack` chunk.
- **schema version** - the integer in the metadata document naming which keys it may carry.
- **manifest entry** - one declaration of an embedded hashtable: its chunk path, category,
  algorithm and key width.

## <a id="s4"></a>4. Conventions

- **Byte order.** Every integer is little-endian.
- **Integer types.** `u8`, `u32`, `i32`, `u64` have their usual widths. Nothing is padded
  except where [section 5.5](#s5.5) says.
- **Strings.** UTF-8. A *counted string* is a `u32` byte length followed by that many bytes, no
  terminator. A *NUL-terminated string* is its bytes followed by one `0x00`; the bytes contain no
  `0x00`.
- **Hashes.** Two functions, both seed 0, both over the canonical name:
  - `xxh64` - xxHash64. Chunk paths.
  - `xxh3` - XXH3-64, default secret. Layer names, WAD names, and every checksum.
- **Offsets** are absolute: counted from the first byte of the file.
- **Sentinels.** `0xFFFFFFFF` as a table position, written `NONE`, means "no entry".
- **Requirement words.** *MUST*, *MUST NOT*, *SHOULD* and *MAY* carry their RFC 2119 meaning.
  A requirement on a *reader* binds anything that mounts a package. A requirement on a *writer*
  binds anything that produces one.

## <a id="s5"></a>5. Container layout

A package is five regions, in this order, with nothing between them but the one alignment gap:

```
+-----------------------+
| header                |  section 5.1
+-----------------------+
| layer table           |  section 5.2
+-----------------------+
| path table            |  section 5.3
+-----------------------+
| WAD table             |  section 5.4
+-----------------------+
| alignment to 8        |  section 5.5
+-----------------------+
| table of contents     |  section 5.6
+-----------------------+
| chunk data            |  section 5.7
+-----------------------+
```

### <a id="s5.1"></a>5.1 Header

| Offset | Size | Type       | Field            | Value                                    |
| -----: | ---: | ---------- | ---------------- | ---------------------------------------- |
|      0 |    8 | bytes      | `magic`          | `5F 6D 6F 64 70 6B 67 5F` (`_modpkg_`)   |
|      8 |    4 | `u32`      | `version`        | `1`                                      |
|     12 |    4 | `u32`      | `signature_size` | Byte length of `signature`               |
|     16 |    4 | `u32`      | `chunk_count`    | Number of records in the TOC             |
|     20 |    n | bytes      | `signature`      | `signature_size` opaque bytes            |

`signature` is reserved. Format version 1 defines no signature scheme; a writer writes
`signature_size = 0` and a reader skips whatever is there.

### <a id="s5.2"></a>5.2 Layer table

```
u32   layer_count
repeated layer_count times:
    counted string   name        a slug (section 3)
    i32              priority
```

The table position of an entry, counting from 0, is its *layer index*. [Section 8](#s8) states
the rules on names and priorities.

### <a id="s5.3"></a>5.3 Path table

```
u32   path_count
repeated path_count times:
    NUL-terminated string   stored path
```

The table position is the *path index*. Each stored path appears once, judged by canonical
name. The table holds meta chunk paths and content chunk paths alike.

### <a id="s5.4"></a>5.4 WAD table

```
u32   wad_count
repeated wad_count times:
    NUL-terminated string   WAD name
```

The table position is the *WAD index*. Each WAD appears once, judged by canonical name.

### <a id="s5.5"></a>5.5 Alignment

Zero bytes, `(8 - offset mod 8) mod 8` of them, bringing the next offset to a multiple of 8.

### <a id="s5.6"></a>5.6 Table of contents

`chunk_count` records of 61 bytes each, contiguous. One record:

| Offset | Size | Type  | Field                   | Meaning                                                |
| -----: | ---: | ----- | ----------------------- | ------------------------------------------------------ |
|      0 |    8 | `u64` | `path_hash`             | The chunk's path hash ([section 6.1](#s6.1))           |
|      8 |    8 | `u64` | `data_offset`           | Absolute offset of the stored bytes                    |
|     16 |    1 | `u8`  | `compression`           | `0` raw, `1` Zstandard ([section 7.1](#s7.1))          |
|     17 |    8 | `u64` | `compressed_size`       | Length of the stored bytes                             |
|     25 |    8 | `u64` | `uncompressed_size`     | Length of the content bytes                            |
|     33 |    8 | `u64` | `compressed_checksum`   | `xxh3` of the stored bytes                             |
|     41 |    8 | `u64` | `uncompressed_checksum` | `xxh3` of the content bytes                            |
|     49 |    4 | `u32` | `path_index`            | Position in the path table                             |
|     53 |    4 | `u32` | `layer_index`           | Position in the layer table, or `NONE` for a meta chunk |
|     57 |    4 | `u32` | `wad_index`             | Position in the WAD table, or `NONE` for a meta chunk  |

For a raw chunk, `compressed_size` equals `uncompressed_size` and the two checksums are equal.

Records are in no defined order. A reader MUST NOT depend on the order of records, and a writer
MAY emit them in any order.

### <a id="s5.7"></a>5.7 Chunk data

The stored bytes of every chunk, each run located by its record's `data_offset` and
`compressed_size`. The runs are contiguous, unaligned and unpadded. Two records MAY point at the
same run ([section 7.3](#s7.3)). The file ends with the last run.

## <a id="s6"></a>6. Names and identity

### <a id="s6.1"></a>6.1 Chunk paths

A chunk path is the path of the file inside its target WAD. Its identity is its canonical name.

- **Separators.** The stored path uses `/`. A writer normalizes every `\` to `/` before storing
  or hashing. A reader normalizes `\` to `/` in a path it is asked to look up.
- **Casing.** The stored path keeps the author's casing. Hashing ASCII-lowercases it first, and
  only ASCII: `É` and `é` are two different names.
- **Hash.** `path_hash = xxh64(ascii_lowercase(stored_path))`, seed 0. This is the hash the
  game computes over the same path inside a `.wad.client`, and the hash the `game` category of
  the embedded hashtable standard is keyed by.
- **Uniqueness.** Two stored paths with one canonical name are one path. A writer MUST NOT emit
  a path table with two entries of one canonical name.

### <a id="s6.2"></a>6.2 Layer and WAD names

Layer names and WAD names hash with `xxh3` over their ASCII-lowercased form, seed 0. The hashes
do not appear in the file. They are how an implementation identifies a layer or a WAD, and they
make both name spaces case-insensitive: `Aatrox.wad.client` and `aatrox.wad.client` are one WAD.

The reference writer stores WAD names ASCII-lowercased. A reader MUST NOT depend on that.

### <a id="s6.3"></a>6.3 Chunk identity

A chunk's identity is the pair `(path_hash, layer)`. Within one layer, one path names one chunk.

WAD membership is not part of identity. A chunk MAY be registered under several WADs: the TOC
then holds one record per WAD, all with the same `path_hash` and `layer_index` and different
`wad_index` values. Every such record MUST describe the same content bytes
([section 12.2](#s12.2)).

### <a id="s6.4"></a>6.4 Hex-named chunks

A chunk whose real path is unknown is stored under its hex name: the sixteen lowercase hex
digits of its path hash, with any extension after the first `.` kept as a hint
(`0123456789abcdef.dds`). For such a chunk:

- `path_hash` is the value the hex digits encode, not the hash of the hex string.
- The path table holds the hex name as the stored path, and `path_index` points at it.
- The hash of the stored path as a string is *not* the record's `path_hash`. A reader
  MUST resolve a record's path through `path_index`, never by hashing the stored path.

A hex name is recognized by its shape: the part before the first `.` is exactly sixteen ASCII
hex digits, no `0x` prefix. Anything else is a real path, including a sixteen-digit name inside
a directory.

A package MAY carry an embedded `game` hashtable ([section 10.5](#s10.5)) naming the real path
of a hex-named chunk.

## <a id="s7"></a>7. Chunks

### <a id="s7.1"></a>7.1 Compression

| `compression` | Stored bytes                                                     |
| ------------: | ---------------------------------------------------------------- |
|           `0` | The content bytes, verbatim.                                     |
|           `1` | One Zstandard frame whose decoded content is the content bytes. |

Any other value is invalid and a reader MUST refuse the package.

The frame is a plain single-segment frame with no dictionary. A reader decodes it with
`uncompressed_size` as the expected output length and MUST treat a frame that decodes to a
different length as corrupt.

The stored form of a Zstandard chunk is exactly what a game WAD stores for a Zstandard chunk,
and the two checksums have the same definition as the WAD TOC checksum. An overlay builder MAY
copy the stored bytes and `compressed_checksum` into a WAD chunk record without decoding
(ADR-0001 states that the copy recomputes the checksum in flight).

A writer chooses the compression per chunk. Compression is a *request*: a writer that finds the
frame no smaller than the content MAY store the chunk raw. The reference writer requests
Zstandard for everything except Wwise audio containers (`.bnk`, `.wpk`, judged by extension,
case-insensitively), which the game stores raw, and stores a chunk raw when the frame is 95% of
the content size or larger.

### <a id="s7.2"></a>7.2 Checksums

Both checksums are `xxh3` (XXH3-64, seed 0, default secret).

- `uncompressed_checksum` identifies the content. Two chunks with equal
  `(uncompressed_checksum, uncompressed_size)` hold the same content.
- `compressed_checksum` identifies the stored bytes. It is the value a WAD TOC carries for the
  same bytes.

A writer MUST compute both over the bytes it writes. A reader SHOULD verify the checksum of any
bytes it hands on, and MUST NOT write a chunk into a game WAD under a checksum it did not verify
or recompute (ADR-0001).

XXH3 is the checksum for one reason: it is the game's. The `.wad.client` TOC of the game's
shipping WAD version carries the XXH3-64 of a chunk's stored bytes, and an overlay builder that
passes a modpkg chunk through writes `compressed_checksum` into that field. Earlier WAD versions
carried the first eight bytes of a SHA-256 digest in the same field, and the game's move to XXH3
is what fixes the choice here. XXH3 is not a cryptographic hash. A modpkg checksum detects
corruption and identifies content; it does not authenticate either, and a package's integrity
against a deliberate change is not something format version 1 provides ([section 5.1](#s5.1)
reserves the signature field for that). The checksum algorithm tracks the game's: a change in
the game's WAD checksum is a new modpkg format version carrying the same algorithm
([section 16](#s16)).

### <a id="s7.3"></a>7.3 Shared stored bytes

Two records with the same content share one run of stored bytes: the reference writer stores
each distinct `(uncompressed_checksum, uncompressed_size)` once and points every later record
with that content at the first run. The records then agree in `data_offset`, `compression`,
`compressed_size` and `compressed_checksum`. A reader treats each record independently and
needs no knowledge of the sharing.

### <a id="s7.4"></a>7.4 Sizes

`compressed_size` and `uncompressed_size` are 64-bit. A reader MUST check that
`data_offset + compressed_size` lies within the file before reading a chunk.

## <a id="s8"></a>8. Layers

A layer is a named, prioritized slice of the mod's content. A chunk belongs to exactly one layer
through `layer_index`, or to none (`NONE`) when it is a meta chunk.

- **`base` is required.** Every package declares a layer named `base`. A reader MUST refuse a
  package without one.
- **Names are slugs.** Every name in the layer table is a slug ([section 3](#s3)). Identity is
  case-insensitive ([section 6.2](#s6.2)); a slug is already lowercase.
- **Priorities.** `priority` is a signed integer. `base` has priority `0`. When layers are
  composed, a higher priority applies later and overrides a lower one. Two layers with equal
  priority are ordered by name. The layer table is the only source of a layer's priority; the
  `priority` in the metadata document ([section 11.3](#s11.3)) is informational.
- **Every referenced layer is declared.** A record's `layer_index` is either `NONE` or a valid
  position in the layer table.
- **A declared layer MAY be empty.** A layer with no chunk is a layer.

A layer that is not `base` is optional content. A consumer composing a mod (LTK Manager's
overlay builder) applies `base` and whichever other layers the user enables, in ascending
priority.

## <a id="s9"></a>9. WAD targets

A WAD target is the game archive a content chunk belongs in, named as the file is named in the
game install: `Aatrox.wad.client`, `Map11.wad.client`, `Global.en_us.wad.client`. A chunk's
`wad_index` points at it.

- A content chunk SHOULD have a WAD target. A content chunk with `wad_index = NONE` is stored
  and extracted at the layer root ([section 14](#s14)) and is not placed in any WAD by the
  reference overlay builder.
- A WAD name is a file name: no directory component. A name that is not one the game ships is
  legal; the overlay builder's name resolution is the consumer's concern, not the format's.
- The same chunk MAY belong to several WADs ([section 6.3](#s6.3)).

## <a id="s10"></a>10. Meta chunks

The path prefix `_meta_/` is reserved. A chunk under it is a meta chunk: `layer_index` and
`wad_index` are both `NONE`, and it describes the mod rather than being part of it. A reader
MUST refuse to treat a chunk under `_meta_/` as a meta chunk if either index is not `NONE`.

| Chunk path               | Required | Content                                   | Reference compression |
| ------------------------ | -------- | ----------------------------------------- | --------------------- |
| `_meta_/info.msgpack`    | yes      | The metadata document ([section 11](#s11)) | raw                   |
| `_meta_/thumbnail.webp`  | no       | The preview image, WebP                   | raw                   |
| `_meta_/readme.md`       | no       | Documentation, Markdown                   | raw                   |
| `_meta_/license`         | no       | The full license text                     | Zstandard             |
| `_meta_/hashes/<file>`   | no       | One embedded hashtable                    | Zstandard             |

Meta chunks are content chunks in every other respect: same record layout, same compression
choices, same checksums. The compression column is what the reference writer does, not a rule.

A meta chunk path a reader does not recognize is ignored. A tool that rewrites a package MUST
carry such a chunk through unchanged.

### <a id="s10.1"></a>10.1 `_meta_/info.msgpack`

The metadata document. Present in every package. [Section 11](#s11) is its schema.

### <a id="s10.2"></a>10.2 `_meta_/thumbnail.webp`

A WebP image, still or animated. The reference writer converts any input image to WebP and
refuses an input over 5 MiB. A reader displays it or ignores it; nothing depends on it.

### <a id="s10.3"></a>10.3 `_meta_/readme.md`

The mod's README, verbatim.

### <a id="s10.4"></a>10.4 `_meta_/license`

The full text of the mod's license terms, verbatim. Naming the license and shipping its text
are independent: the `license` key of the metadata document names the terms
([section 11.4](#s11.4)); this chunk carries them. A package may have either, both or neither.

### <a id="s10.5"></a>10.5 `_meta_/hashes/<file>`

One embedded hashtable per chunk, in the grammar of the embedded hashtable standard: one name
per line, printable ASCII, LF, no hash column. The chunk path is `_meta_/hashes/` followed by
one file name with no further `/`.

The `hashtables` list of the metadata document ([section 11.5](#s11.5)) is the manifest, and
it is authoritative: a chunk under `_meta_/hashes/` that no manifest entry declares is not a
table and is not read. A manifest entry whose chunk is missing is an error. Two manifest
entries MAY declare one chunk path (one table read under two shapes); the chunk is stored once.

A `game` table in a modpkg MAY be *trimmed*: a name whose key equals the path hash of a chunk
the package stores under a real path MAY be left out, the stored path being the name's
surviving copy. Only `game` is ever trimmed. Names in other categories survive nowhere else in
a package.

## <a id="s11"></a>11. Metadata schema

The metadata document is one MessagePack map with string keys. Keys are `snake_case`. A key
whose value is absent is either omitted or holds `nil`, as the table says. A reader MUST ignore
a key it does not know and MUST supply the documented default for a key that is missing.

### <a id="s11.1"></a>11.1 Schema versions

| `schema_version` | Adds                                            |
| ---------------: | ----------------------------------------------- |
|              `1` | The base document                               |
|              `2` | `string_overrides` on a layer                   |
|              `3` | `hashtables`                                    |
|              `4` | `game_data` on a layer                          |
|              `5` | `generator`                                     |

The schema is additive. Every key added after `1` is optional, and a package written under
schema `n` decodes under a reader that knows only `m < n`, with the keys it does not know
ignored. A reader MUST NOT refuse a document over its `schema_version`. A writer writes the
highest version it knows; `5` is the highest.

A document without a `schema_version` key is read as the highest version the reader knows.

### <a id="s11.2"></a>11.2 The document

| Key              | Type                                       | Absent as | Meaning                                                       |
| ---------------- | ------------------------------------------ | --------- | ------------------------------------------------------------- |
| `schema_version` | uint                                       | see above | [Section 11.1](#s11.1)                                        |
| `name`           | str                                        | required  | The mod's machine name: no spaces, `_` and `-` allowed         |
| `display_name`   | str                                        | required  | The name shown to a user                                      |
| `description`    | str or nil                                 | `nil`     | What the mod does                                             |
| `version`        | str                                        | required  | The mod's version, a semantic version (`1.2.0`)               |
| `distributor`    | map or nil                                 | `nil`     | [Section 11.6](#s11.6)                                        |
| `authors`        | array of map                               | required  | [Section 11.7](#s11.7); MAY be empty                          |
| `license`        | map                                        | required  | [Section 11.4](#s11.4)                                        |
| `tags`           | array of str                               | omitted   | Categories: `champion-skin`, `sfx`, ... ([section 11.8](#s11.8)) |
| `champions`      | array of str                               | omitted   | Champions the mod targets, by name                            |
| `maps`           | array of str                               | omitted   | Maps the mod targets ([section 11.8](#s11.8))                 |
| `layers`         | array of map                               | omitted   | [Section 11.3](#s11.3)                                        |
| `hashtables`     | array of map                               | omitted   | [Section 11.5](#s11.5), schema 3                              |
| `generator`      | str                                        | omitted   | [Section 11.9](#s11.9), schema 5                              |

"Omitted" means the writer leaves the key out when the list is empty and a reader reads a
missing key as an empty list. The reference writer writes `nil` for an absent `description` and
`distributor` and writes every other key that is not marked omitted.

### <a id="s11.3"></a>11.3 Layer metadata

One map per layer the writer has metadata for. The list is informational: the layer table
([section 5.2](#s5.2)) is the source of which layers exist and of their priorities, and a
layer here that the table does not declare is ignored.

| Key                | Type       | Absent as | Meaning                                                    |
| ------------------ | ---------- | --------- | ---------------------------------------------------------- |
| `name`             | str        | required  | The layer's slug, matching a layer table entry             |
| `display_name`     | str        | omitted   | The name shown to a user                                   |
| `priority`         | int        | required  | A copy of the layer table's value                          |
| `description`      | str        | omitted   | What the layer changes                                     |
| `string_overrides` | map        | omitted   | Schema 2. See below                                        |
| `game_data`        | map        | omitted   | Schema 4. A declaration document, `game-data.md` [section 5](game-data.md#s5) |

`string_overrides` is a map from locale to a map from stringtable field name to replacement
string: `{"en_us": {"game_character_displayname_Ahri": "Fox Spirit"}}`. The locale `default`
applies to every locale. A field name is a key of `data/menu/<locale>/lol.stringtable` in
`Global.<locale>.wad.client`; a field name that is exactly sixteen hex digits is a precomputed
hash of one. Overrides carry only the strings the mod changes. A consumer applies them over the
game's own stringtable of the moment, in ascending layer priority, `default` before the
specific locale within one layer.

### <a id="s11.4"></a>11.4 License

A map with a `type` key selecting the shape:

| `type`   | Other keys                                | Meaning                                                |
| -------- | ----------------------------------------- | ------------------------------------------------------ |
| `none`   |                                           | No license is declared                                 |
| `spdx`   | `spdx_id`: str                            | A license on the SPDX list, by identifier (`MIT`)      |
| `custom` | `name`: str, `url`: str                   | A license by display name and, optionally, a link      |

Under `custom`, `url` is always written; the empty string means no URL. A reader treats a
missing `url` and an empty one alike.

A reader MUST treat an `spdx_id` it does not recognize as an opaque display string.

### <a id="s11.5"></a>11.5 Hashtable manifest

One map per embedded hashtable, in the order the tables merge. All four keys are required.

| Key         | Type | Meaning                                                            |
| ----------- | ---- | ------------------------------------------------------------------ |
| `path`      | str  | The table's chunk path, `_meta_/hashes/<file>`                     |
| `category`  | str  | The lookup domain: `game`, `binentries`, `binhashes`, or another   |
| `algorithm` | str  | The hash function: `xxh64`, `fnv1a_32`, `xxh3`, or another         |
| `bits`      | uint | The key width in bits, `1` to `64`                                 |

Category and algorithm registries are open. A reader ignores an entry whose category it does
not know, skips a table whose algorithm it cannot compute, skips an entry whose `bits` is
outside `1..=64`, and preserves all three when rewriting a package. Merging, duplicates and
collisions are the embedded hashtable standard's rules.

### <a id="s11.6"></a>11.6 Distributor

Where the package was published, for a tool that checks for updates or links back.

| Key         | Type | Meaning                                                |
| ----------- | ---- | ------------------------------------------------------ |
| `site_id`   | str  | The site's identifier (`runeforge`)                    |
| `site_name` | str  | The site's display name (`Runeforge`)                  |
| `site_url`  | str  | The site's base URL (`https://runeforge.dev`)          |
| `mod_id`    | str  | The mod's identifier on that site                      |

A package packed from a mod project has no distributor. A distribution site stamps one when it
accepts an upload.

### <a id="s11.7"></a>11.7 Author

| Key    | Type       | Meaning                                    |
| ------ | ---------- | ------------------------------------------ |
| `name` | str        | The author's name                          |
| `role` | str or nil | What they did (`Developer`, `Artist`, ...)  |

### <a id="s11.8"></a>11.8 Tags and maps

`tags` and `maps` hold kebab-case strings. The well-known values are the mod project's:

- **tags** - `league-of-legends`, `tft`, `champion-skin`, `map-skin`, `ward-skin`, `emote`,
  `summoner-icon`, `companion`, `ui`, `hud`, `font`, `sfx`, `announcer`, `structure`, `minion`,
  `jungle-monster`, `misc`.
- **maps** - `summoners-rift`, `aram`, `teamfight-tactics`, `arena`, `swarm`.

Any other string is a custom value. A reader carries an unknown value through and MAY display
it verbatim.

### <a id="s11.9"></a>11.9 Generator

The tool that wrote the package: its name, one space, its version. `ltk_mod_project 0.9.2`.
Free text, informational. A writer names itself. A tool that rewrites a package replaces the
value with its own. A reader MAY display the value and MUST NOT vary its behaviour on it. A
package that fails to mount carries, in the value, the one fact a bug report needs.

## <a id="s12"></a>12. Reader requirements

A reader *mounts* a package: it reads the header, the tables and the TOC, checks them, and only
then answers questions about chunks. A package that fails any check below is refused whole,
before any chunk data is read.

### <a id="s12.1"></a>12.1 Mount

1. `magic` MUST equal `_modpkg_`. Otherwise the file is not a package.
2. `version` MUST equal `1`. A reader MUST refuse any other value; it MUST NOT attempt to read
   a version it does not implement.
3. Skip `signature_size` bytes.
4. Read the three tables. Every name in them MUST be valid UTF-8.
5. One layer name MUST be `base`.
6. Skip to the next multiple of 8.
7. Read `chunk_count` records. For each:
   - `compression` MUST be `0` or `1`.
   - `path_index` MUST be less than `path_count`.
   - `layer_index` MUST be `NONE` or less than `layer_count`.
   - `wad_index` MUST be `NONE` or less than `wad_count`.
   - `data_offset + compressed_size` MUST NOT exceed the file length.

### <a id="s12.2"></a>12.2 Consistency

Two records with one `(path_hash, layer_index)` MUST agree in `uncompressed_checksum` and
`uncompressed_size`. A package where they disagree describes one chunk with two contents and is
refused.

### <a id="s12.3"></a>12.3 Containment

Every stored path, layer name and WAD name is joined onto an output directory by an extraction.
A reader MUST refuse a package holding a name that escapes: a name with a `..` component, a
name starting with `/` or `\`, or a name containing `:`. `\` counts as a separator on every
platform, and `:` is refused on every platform. A `.` component is allowed.

### <a id="s12.4"></a>12.4 Lookup

To find a chunk by a path and a layer:

1. Normalize `\` to `/`, compute `path_hash` over the ASCII-lowercased path, and look up
   `(path_hash, layer)`.
2. On a miss, if the path's file name (after the last `/`) has the hex-name shape
   ([section 6.4](#s6.4)), look up the hash the digits encode under the same layer.
3. Otherwise the chunk is absent.

A meta chunk is looked up with no layer. A reader MUST check that a chunk it takes for a meta
chunk has both indices `NONE`.

### <a id="s12.5"></a>12.5 Reading a chunk

Seek to `data_offset`, read `compressed_size` bytes. For `compression = 1`, decode the frame
into exactly `uncompressed_size` bytes. A reader SHOULD verify `compressed_checksum` over the
bytes read and `uncompressed_checksum` over the bytes decoded, and MUST do one of the two before
placing the bytes in a game WAD.

### <a id="s12.6"></a>12.6 Tolerance

A reader MUST ignore, and a rewriting tool MUST preserve:

- a meta chunk path it does not recognize;
- a metadata key it does not recognize, at any depth;
- a hashtable manifest entry whose category or algorithm it does not recognize;
- bytes in `signature`.

A reader MUST NOT infer anything from the order of TOC records, from the order of table
entries beyond their positions, or from the order of chunk data runs.

## <a id="s13"></a>13. Writer requirements

A writer produces a package a reader accepts under [section 12](#s12), and more:

### <a id="s13.1"></a>13.1 Structure

- `magic`, `version = 1`, `signature_size = 0`.
- The layer table declares `base` with priority `0` and every layer any record refers to.
  Every name is a slug.
- The path table holds each canonical name once and every path any record refers to.
- The WAD table holds each canonical name once and every WAD any record refers to.
- Zero padding to a multiple of 8 before the TOC.
- `chunk_count` equals the number of records; every record's `data_offset` and
  `compressed_size` lie inside the file.

### <a id="s13.2"></a>13.2 Chunks

- Every content chunk names a real chunk path or a hex name, a declared layer, and (SHOULD) a
  declared WAD.
- A stored path has `/` separators and the author's casing. A hex name is lowercase.
- A writer MUST refuse two files that map to one `(path_hash, layer)` with different content,
  including two paths that differ only in case. It MAY store two files with one
  `(path_hash, layer)` and identical content under different WADs.
- Both checksums are computed over the bytes written.
- `compression = 1` bytes are one valid Zstandard frame that decodes to exactly
  `uncompressed_size` bytes.
- A writer SHOULD store identical content once ([section 7.3](#s7.3)).

### <a id="s13.3"></a>13.3 Meta chunks

- `_meta_/info.msgpack` is present, with `layer_index = wad_index = NONE`, holding a document
  that satisfies [section 11](#s11) at the highest schema version the writer knows.
- The document SHOULD carry `generator` naming the writer ([section 11.9](#s11.9)).
- Every chunk under `_meta_/hashes/` is declared by a manifest entry, every manifest entry's
  chunk exists, and its content fits the hashtable grammar.
- A `game` table has no name whose key collides with another name's key in the merged category;
  a writer MUST refuse to produce the package on a collision.
- The thumbnail, if present, is WebP.

### <a id="s13.4"></a>13.4 Names

- Every stored path, layer name and WAD name satisfies [section 12.3](#s12.3).
- A writer MUST NOT store a path under `_meta_/` for a content chunk.

### <a id="s13.5"></a>13.5 File name

The conventional file name is `<name>_<version>.modpkg`, from the metadata's `name` and
`version`: `my-mod_1.2.0.modpkg`.

## <a id="s14"></a>14. Extraction layout

A package unpacks to the mod project layout it packs from. The mapping, for a package extracted
into one directory:

| Chunk                                    | Lands at                              |
| ---------------------------------------- | ------------------------------------- |
| content, `wad_index` set                 | `<layer>/<wad>/<stored path>`         |
| content, `wad_index = NONE`              | `<layer>/<stored path>`               |
| `_meta_/readme.md`                       | `README.md`                           |
| `_meta_/license`                         | `LICENSE`                             |
| `_meta_/thumbnail.webp`                  | `thumbnail.webp`                      |
| `_meta_/hashes/<file>`                   | `hashes/<file>`                       |
| `_meta_/info.msgpack`                    | not a file; it becomes `mod.config.json` |
| any other meta chunk                     | not written                           |

A chunk under several WADs lands once under each. A project keeps its layers under `content/`;
the three root files and `hashes/` sit beside it:

```
my-mod
|-- mod.config.json
|-- README.md
|-- LICENSE
|-- thumbnail.webp
|-- hashes
|   |-- game.hashes.txt
|-- content
|   |-- base
|   |   |-- aatrox.wad.client
|   |   |   |-- DATA
|   |   |   |   |-- Characters
|   |   |   |   |   |-- Aatrox
|   |   |   |   |   |   |-- Skins
|   |   |   |   |   |   |   |-- Skin0
|   |   |   |   |   |   |   |   |-- Skin0.bin
|   |-- high-res
|   |   |-- aatrox.wad.client
|   |   |   |-- ASSETS
|   |   |   |   |-- ...
```

Packing the reverse: every top-level directory of a layer whose name ends in `.wad.client`
(case-insensitively) is a WAD target, and the path of a file beneath it is the chunk path. A file
at a WAD root whose stem is exactly sixteen hex digits is a hex-named chunk. A file in a layer
outside any WAD directory is a content chunk with no WAD.

## <a id="s15"></a>15. Fantome interoperability

Both formats pack from one mod project. What crosses between them:

| Project feature                    | modpkg                     | fantome                                              |
| ---------------------------------- | -------------------------- | ---------------------------------------------------- |
| Content of `base`                  | chunks                     | one packed WAD per WAD target                        |
| Content of other layers            | chunks                     | not carried                                          |
| Layer names and priorities         | layer table                | `Layers` in `info.json`, only for layers with string overrides |
| Per-layer string overrides         | metadata                   | `Layers` in `info.json` (extension)                  |
| Machine name                       | `name`                     | not carried; re-derived from `Name` on import        |
| Authors with roles                 | `authors`                  | one `Author` string, comma-joined, roles dropped     |
| License, typed                     | `license`                  | `License` (extension)                                |
| License text                       | `_meta_/license`           | `META/LICENSE` (extension)                           |
| Tags, champions, maps              | lists                      | `Tags`, `Champions`, `Maps` (extension)              |
| Thumbnail                          | WebP                       | `META/image.png`, re-encoded to PNG                  |
| Readme                             | `_meta_/readme.md`         | `META/README.md`                                     |
| Embedded hashtables                | `_meta_/hashes/`           | `META/hashes/` (extension)                           |
| Chunk paths                        | path table, authored casing | none inside a packed WAD; recovered through a harvested `game` table |
| Distributor                        | `distributor`              | not carried                                          |
| Packing tool                       | `generator`                | `Generator` (extension)                              |

A modpkg holds everything a project declares. A fantome holds the base layer and whatever the
LeagueToolkit extension of `info.json` carries. A tool that converts a modpkg to a fantome
drops the rest and SHOULD say so.

## <a id="s16"></a>16. Versioning

Two independent numbers version a package.

- **Format version** (`version` in the header) covers the byte layout of
  [section 5](#s5). A change to any table, the record layout, a hash function or a
  compression code is a new format version. A reader implements the versions it knows and
  refuses the rest ([section 12.1](#s12.1)). Version `1` is the only version.
- **Schema version** (`schema_version` in the metadata document) covers the keys of
  [section 11](#s11). It grows by adding optional keys only. A reader never refuses a document
  over it ([section 11.1](#s11.1)).

Room for extension without a new format version:

- a new meta chunk path, which a reader ignores;
- a new metadata key, which a reader ignores;
- a new hashtable category or algorithm, which a reader skips;
- the `signature` bytes, which a reader skips.

A new compression code, a new record field, or a change of hash function requires a new format
version and an ADR. A change of the game's WAD checksum algorithm is one such change: the
modpkg checksum exists to be the WAD's ([section 7.2](#s7.2)), and a package whose checksums
the game's TOC cannot hold has lost the pass-through property.

## <a id="s17"></a>17. Rules

Every settled question too small for its own section. IDs are stable citation keys.

| ID  | Rule                                                                                                  | Instead of                                   | Why                                                                                              | Spec                     |
| --- | ----------------------------------------------------------------------------------------------------- | -------------------------------------------- | ------------------------------------------------------------------------------------------------ | ------------------------ |
| D1  | All integers little-endian.                                                                           | big-endian, mixed                            | The game's formats are little-endian; the reference hardware is x86.                             | [section 4](#s4)         |
| D2  | Chunk paths hash with xxHash64 over the ASCII-lowercased path.                                        | XXH3, case-sensitive hashing                 | Identical to the game's WAD chunk hash and to the `game` hashtable category.                     | [section 6.1](#s6.1)     |
| D3  | The stored path keeps the author's casing; identity is the canonical name.                            | lowercasing the stored path                  | An extraction names files as the author did; two casings are one chunk.                          | [section 6.1](#s6.1)     |
| D4  | Layer and WAD names hash with XXH3 over the lowercased name; the hashes are not stored.               | storing the hashes                           | The names are in the tables; the hash is an implementation's key.                                | [section 6.2](#s6.2)     |
| D5  | WAD membership is not part of chunk identity.                                                         | `(path, layer, wad)` identity                | One file shared by several WADs is one chunk stored once.                                        | [section 6.3](#s6.3)     |
| D6  | A record's path is resolved through `path_index`, never by hashing the stored path.                   | hashing the stored path                      | A hex-named chunk's `path_hash` is the hash it names, not the hash of the hex string.            | [section 6.4](#s6.4)     |
| D7  | A hex name is sixteen hex digits before the first `.`, at a WAD root only.                            | `0x` prefixes, any depth                     | Matches the extraction escape hatch of the hashtable standard.                                   | [section 6.4](#s6.4)     |
| D8  | Compression is `0` raw or `1` one Zstandard frame; any other code refuses the package.                | ignoring unknown codes                       | A chunk a reader cannot decode is not one to skip silently.                                      | [section 7.1](#s7.1)     |
| D9  | Both checksums are XXH3-64, seed 0, and track the game's WAD checksum.                                | CRC32, SHA-256, a hash chosen on its merits  | Pass-through writes the value into the WAD TOC; XXH3 is the game's choice, not a cryptographic one. | [section 7.2](#s7.2)     |
| D10 | Compression is a request; a frame no smaller than the content is stored raw.                         | always compressing                           | Already-compressed formats gain nothing from a frame.                                            | [section 7.1](#s7.1)     |
| D11 | Wwise audio containers (`.bnk`, `.wpk`) are stored raw.                                               | compressing them                             | The game stores them raw; pass-through keeps that.                                               | [section 7.1](#s7.1)     |
| D12 | Identical content is stored once; records share the run.                                             | one run per record                           | A chroma pack repeats most of its files.                                                         | [section 7.3](#s7.3)     |
| D13 | Records with one `(path_hash, layer)` agree in content or the package is refused.                     | first wins                                   | One chunk has one content.                                                                       | [section 12.2](#s12.2)   |
| D14 | `base` is required and has priority `0`.                                                              | any layer set                                | Every consumer composes from `base`.                                                             | [section 8](#s8)         |
| D15 | Layer names are slugs.                                                                                | free strings                                 | A layer is a directory name on every platform.                                                   | [section 8](#s8)         |
| D16 | The layer table is the only source of priority; the metadata copy is informational.                   | metadata as the source                       | The header is read before the metadata chunk.                                                    | [section 8](#s8)         |
| D17 | `_meta_/` is reserved; a meta chunk has both indices `NONE`.                                          | a flag bit                                   | The reserved prefix needs no record field.                                                       | [section 10](#s10)       |
| D18 | The metadata document is MessagePack with string keys.                                                | JSON, a binary struct                        | Self-describing and additive like JSON, compact, and needs no text parser.                      | [section 11](#s11)       |
| D19 | The schema is additive; a reader never refuses a document over `schema_version`.                      | strict versioning                            | An old reader reads a new package minus the keys it cannot use.                                  | [section 11.1](#s11.1)   |
| D20 | A missing `schema_version` reads as the highest known.                                                | reading as `1`                               | Every writer writes the key; the default only decides a hand-built document.                     | [section 11.1](#s11.1)   |
| D21 | An empty list key is omitted.                                                                         | writing `[]`                                 | A package without a feature serializes byte-identically to one written before the feature.       | [section 11.2](#s11.2)   |
| D22 | Under `custom`, `url` is always written; `""` means none.                                             | omitting the key                             | A reader predating the optional URL decodes the whole document or none of it.                    | [section 11.4](#s11.4)   |
| D23 | The hashtable manifest is authoritative; no table is discovered by path.                              | scanning `_meta_/hashes/`                    | Discovery by name is how fantome metadata diverged between tools.                                | [section 10.5](#s10.5)   |
| D24 | Only a `game` table is trimmed against stored paths.                                                  | trimming every category                      | Nothing in a package deduces a `binentries` or `binhashes` name.                                 | [section 10.5](#s10.5)   |
| D25 | A name with a `..` component, a leading `/` or `\`, or a `:` refuses the package.                     | sanitizing on extraction                     | Zip slip; a package one host refuses and another unpacks is worse than either answer.            | [section 12.3](#s12.3)   |
| D26 | A package failing any mount check is refused whole, before any chunk data is read.                    | partial mounts                               | A chunk that cannot be named cannot be extracted; an unpack that stops halfway is a mess.        | [section 12.1](#s12.1)   |
| D27 | TOC record order is undefined.                                                                        | sorted records                               | A reader indexes by key; an order is a promise no reader needs.                                  | [section 5.6](#s5.6)     |
| D28 | The `signature` field is reserved and empty.                                                          | dropping the field                           | Signing is a plausible extension and the field costs four bytes.                                   | [section 5.1](#s5.1)     |
| D29 | The conventional file name is `<name>_<version>.modpkg`.                                              | free naming                                  | A library can tell two versions apart by file name.                                              | [section 13.5](#s13.5)   |

## <a id="appendix-a"></a>Appendix A. A worked example

A package built by the reference writer, `ltk_modpkg` 0.9.2, from one content file. Its
metadata: name `example`, display name `Example`, description `An example`, version `1.0.0`,
one author `Crauzer` with no role, license `MIT`. Its content: the five bytes `hello` at
`DATA/Characters/Aatrox/Skins/Skin0/Skin0.bin` in layer `base`, WAD `Aatrox.wad.client`,
stored raw.

```
00000000  5f 6d 6f 64 70 6b 67 5f 01 00 00 00 00 00 00 00  _modpkg_........
00000010  02 00 00 00 01 00 00 00 04 00 00 00 62 61 73 65  ............base
00000020  00 00 00 00 02 00 00 00 5f 6d 65 74 61 5f 2f 69  ........_meta_/i
00000030  6e 66 6f 2e 6d 73 67 70 61 63 6b 00 44 41 54 41  nfo.msgpack.DATA
00000040  2f 43 68 61 72 61 63 74 65 72 73 2f 41 61 74 72  /Characters/Aatr
00000050  6f 78 2f 53 6b 69 6e 73 2f 53 6b 69 6e 30 2f 53  ox/Skins/Skin0/S
00000060  6b 69 6e 30 2e 62 69 6e 00 01 00 00 00 61 61 74  kin0.bin.....aat
00000070  72 6f 78 2e 77 61 64 2e 63 6c 69 65 6e 74 00 00  rox.wad.client..
00000080  de 5a 05 40 51 19 6f 73 fa 00 00 00 00 00 00 00  .Z.@Q.os........
00000090  00 a1 00 00 00 00 00 00 00 a1 00 00 00 00 00 00  ................
000000a0  00 3a 0b 5d 1b 81 8f 8a c0 3a 0b 5d 1b 81 8f 8a  .:.].....:.]....
000000b0  c0 00 00 00 00 ff ff ff ff ff ff ff ff 35 c3 b5  .............5..
000000c0  98 34 03 fa 35 9b 01 00 00 00 00 00 00 00 05 00  .4..5...........
000000d0  00 00 00 00 00 00 05 00 00 00 00 00 00 00 fd dc  ................
000000e0  62 5c 55 e8 55 95 fd dc 62 5c 55 e8 55 95 01 00  b\U.U...b\U.U...
000000f0  00 00 00 00 00 00 00 00 00 00 88 ae 73 63 68 65  ............sche
00000100  6d 61 5f 76 65 72 73 69 6f 6e 03 a4 6e 61 6d 65  ma_version..name
00000110  a7 65 78 61 6d 70 6c 65 ac 64 69 73 70 6c 61 79  .example.display
00000120  5f 6e 61 6d 65 a7 45 78 61 6d 70 6c 65 ab 64 65  _name.Example.de
00000130  73 63 72 69 70 74 69 6f 6e aa 41 6e 20 65 78 61  scription.An exa
00000140  6d 70 6c 65 a7 76 65 72 73 69 6f 6e a5 31 2e 30  mple.version.1.0
00000150  2e 30 ab 64 69 73 74 72 69 62 75 74 6f 72 c0 a7  .0.distributor..
00000160  61 75 74 68 6f 72 73 91 82 a4 6e 61 6d 65 a7 43  authors...name.C
00000170  72 61 75 7a 65 72 a4 72 6f 6c 65 c0 a7 6c 69 63  rauzer.role..lic
00000180  65 6e 73 65 82 a4 74 79 70 65 a4 73 70 64 78 a7  ense..type.spdx.
00000190  73 70 64 78 5f 69 64 a3 4d 49 54 68 65 6c 6c 6f  spdx_id.MIThello
```

Decoded:

| Offset | Bytes                    | Field                                                            |
| -----: | ------------------------ | ---------------------------------------------------------------- |
|  `000` | `5f..5f`                 | `magic` = `_modpkg_`                                             |
|  `008` | `01 00 00 00`            | `version` = 1                                                    |
|  `00c` | `00 00 00 00`            | `signature_size` = 0                                             |
|  `010` | `02 00 00 00`            | `chunk_count` = 2                                                |
|  `014` | `01 00 00 00`            | `layer_count` = 1                                                |
|  `018` | `04 00 00 00` `base`     | layer 0: name (counted string)                                   |
|  `020` | `00 00 00 00`            | layer 0: priority = 0                                            |
|  `024` | `02 00 00 00`            | `path_count` = 2                                                 |
|  `028` | `_meta_/info.msgpack\0`  | path 0                                                           |
|  `03c` | `DATA/...Skin0.bin\0`    | path 1, authored casing kept                                     |
|  `069` | `01 00 00 00`            | `wad_count` = 1                                                  |
|  `06d` | `aatrox.wad.client\0`    | WAD 0, lowercased by the reference writer                        |
|  `07f` | `00`                     | alignment to 8                                                   |
|  `080` | `de 5a 05 40 51 19 6f 73`| record 0: `path_hash` = `736f195140055ade` = xxh64(`_meta_/info.msgpack`) |
|  `088` | `fa 00 ..`               | `data_offset` = 250                                              |
|  `090` | `00`                     | `compression` = raw                                              |
|  `091` | `a1 00 ..`               | `compressed_size` = 161                                          |
|  `099` | `a1 00 ..`               | `uncompressed_size` = 161                                        |
|  `0a1` | `3a 0b 5d 1b 81 8f 8a c0`| `compressed_checksum`                                            |
|  `0a9` | `3a 0b 5d 1b 81 8f 8a c0`| `uncompressed_checksum`, equal for a raw chunk                   |
|  `0b1` | `00 00 00 00`            | `path_index` = 0                                                 |
|  `0b5` | `ff ff ff ff`            | `layer_index` = `NONE`                                           |
|  `0b9` | `ff ff ff ff`            | `wad_index` = `NONE`                                             |
|  `0bd` | `35 c3 b5 98 34 03 fa 35`| record 1: `path_hash` = `35fa033498b5c335` = xxh64(`data/characters/aatrox/skins/skin0/skin0.bin`) |
|  `0c5` | `9b 01 ..`               | `data_offset` = 411                                              |
|  `0cd` | `00`                     | `compression` = raw                                              |
|  `0ce` | `05 ..`                  | `compressed_size` = 5                                            |
|  `0d6` | `05 ..`                  | `uncompressed_size` = 5                                          |
|  `0de` | `fd dc 62 5c 55 e8 55 95`| `compressed_checksum` = xxh3(`hello`)                            |
|  `0e6` | `fd dc 62 5c 55 e8 55 95`| `uncompressed_checksum`                                          |
|  `0ee` | `01 00 00 00`            | `path_index` = 1                                                 |
|  `0f2` | `00 00 00 00`            | `layer_index` = 0 (`base`)                                       |
|  `0f6` | `00 00 00 00`            | `wad_index` = 0 (`aatrox.wad.client`)                            |
|  `0fa` | `88 ae ...`              | chunk 0 data: the metadata document, 161 bytes                   |
|  `19b` | `hello`                  | chunk 1 data                                                     |

The metadata document, decoded from MessagePack:

```
{
  "schema_version": 3,
  "name": "example",
  "display_name": "Example",
  "description": "An example",
  "version": "1.0.0",
  "distributor": nil,
  "authors": [ { "name": "Crauzer", "role": nil } ],
  "license": { "type": "spdx", "spdx_id": "MIT" }
}
```

`tags`, `champions`, `maps`, `layers` and `hashtables` are omitted: all are empty.

The name hashes an implementation derives and does not store:

| Name                 | Function | Value                |
| -------------------- | -------- | -------------------- |
| `base`               | xxh3     | `83629ab85e092fcd`   |
| `aatrox.wad.client`  | xxh3     | `e6dba7fd262f1e7d`   |

## <a id="appendix-b"></a>Appendix B. Reference implementation

`ltk_modpkg`, in [league-mod](https://github.com/LeagueToolkit/league-mod), MIT OR Apache-2.0.
The crate reads, writes and extracts packages; `ltk_mod_project` packs a mod project into one
and imports one back; `ltk_overlay` reads one when building a game overlay. Where each rule of
this document lives:

| Subject                                  | Location                                          |
| ---------------------------------------- | ------------------------------------------------- |
| Mount, tables, containment               | `crates/ltk_modpkg/src/read.rs`                   |
| Record layout                            | `crates/ltk_modpkg/src/chunk.rs`                  |
| Path normalization and hashing           | `crates/ltk_modpkg/src/chunk_path.rs`             |
| Layer, WAD and path hashes, hex names    | `crates/ltk_modpkg/src/hashes.rs`                 |
| Table positions and `NONE`               | `crates/ltk_modpkg/src/indices.rs`                |
| Writing, compression, deduplication      | `crates/ltk_modpkg/src/builder.rs`                |
| Decompression                            | `crates/ltk_modpkg/src/decoder.rs`                |
| Metadata document                        | `crates/ltk_modpkg/src/metadata.rs`, `license.rs` |
| Hashtable manifest and chunks            | `crates/ltk_modpkg/src/hashtable.rs`              |
| Meta chunk paths                         | `readme.rs`, `thumbnail.rs`, `license.rs`         |
| Extraction layout                        | `crates/ltk_modpkg/src/plan.rs`, `extractor.rs`   |
| Slugs                                    | `crates/ltk_modpkg/src/slug.rs`                   |
| Project to package                       | `crates/ltk_mod_project/src/modpkg/`              |

LTK Manager is the consumer that packs projects and builds overlays; its checkout is the
reference for what this format has to serve.
