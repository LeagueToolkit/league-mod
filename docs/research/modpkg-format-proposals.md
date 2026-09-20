# Research: proposals for the modpkg format beyond version 1

A research note, not a spec. It lists the gaps in `.modpkg` format version 1 measured against
the five jobs the format serves (`docs/design/modpkg.md` [section 2.1](../design/modpkg.md#s2.1))
and a proposal for each. Nothing here is decided. A proposal that is taken becomes a PRD
requirement, an ADR where it beat an alternative, and an edit to the spec; a proposal that is
dropped stays here with the reason.

Facts about the reference implementation are read at `league-mod` `049ab957` (`main`),
`ltk_modpkg` 0.9.2.

## Contents

- [1. The jobs and the gaps](#s1)
- [2. Proposals without a format bump](#s2)
- [3. Proposals that need format version 2](#s3)
- [4. Proposals above the format](#s4)
- [5. Rejected](#s5)
- [6. Summary table](#s6)

## <a id="s1"></a>1. The jobs and the gaps

The spec names five things a package does: identify the mod, list its content, read one chunk,
copy a chunk into a game WAD, and round-trip to a mod project. Version 1 does each of them.
Where each falls short:

| Job                    | Gap in version 1                                                                                   |
| ---------------------- | -------------------------------------------------------------------------------------------------- |
| Identify               | Finding the metadata chunk reads all three tables and the whole TOC. No game version, no generator, no dependencies. |
| List                   | Layers are a flat priority list; "choose one of these" cannot be expressed.                        |
| Read one chunk         | No cryptographic identity, no signature. TOC order is undefined, so mount is a hash-map build.    |
| Copy into a WAD        | Only replace. No delete, no patch. No subchunked chunk.                                            |
| Round-trip             | Transformer sources, `game_data` declarations and `.modignore` do not travel. Output is not reproducible. |

## <a id="s2"></a>2. Proposals without a format bump

Each of these is a new metadata key, a new meta chunk path, or a writer policy. A version 1
reader ignores the first two by rule (`modpkg.md` [section 12.6](../design/modpkg.md#s12.6))
and never sees the third.

### <a id="s2.1"></a>2.1 Game version targeting

**Gap.** A mod is built against one game patch. Bin properties are retyped between patches
(patch 16.17 retyped over 300 `String` properties to `File`), and a mod built against an
earlier patch fails in the client with no warning from the manager.

**Proposal.** Two metadata keys: `game_version` (the patch string the mod was packed against,
`16.17`) and `wad_version` (the WAD format version of that patch, `3.4`). A manager compares
them against the installed game and warns on a mismatch. A project declares `game_version` in
its manifest; the packer fills `wad_version` from the game index it already holds.

**Cost.** Two optional keys, schema version 4.

### <a id="s2.2"></a>2.2 Layer groups

**Gap.** Layers are a flat list with priorities. A chroma pack with eight chromas is eight
layers, and a manager renders them as eight independent toggles. The original fantome
`DESIGN.md` sketched a `Groups` object (`Kind`, `Members`) for this and nothing implemented
it.

**Proposal.** A `groups` metadata list: `{name, display_name, kind, members}` where `kind` is
`one-of` (radio) or `any` (checkbox) and `members` names layers. A layer belongs to at most
one group. A manager renders a `one-of` group as a single choice and enables exactly one
member.

**Cost.** One optional key, schema version 4. The project manifest gains the same list.

### <a id="s2.3"></a>2.3 Per-layer thumbnails and a gallery

**Gap.** One thumbnail per package. A chroma layer has no preview; a mod site has no
screenshots.

**Proposal.** Reserved meta chunk paths `_meta_/layers/<slug>/thumbnail.webp` and
`_meta_/gallery/<n>.webp`. Both WebP under the existing thumbnail rules. The gallery order is
numeric.

**Cost.** Two reserved paths. No schema change.

### <a id="s2.4"></a>2.4 Generator

**Gap.** A package that fails to mount gives no clue which tool wrote it. Fantome's history is
a catalogue of writers that could not be told apart.

**Proposal.** A `generator` metadata key: the writing tool and its version,
`ltk_mod_project 0.9.2`. Free text, informational.

**Cost.** One optional key, schema version 5.

**Taken.** `modpkg.md` [section 11.9](../design/modpkg.md#s11.9). The fantome extension carries
the same value as `Generator`.

### <a id="s2.5"></a>2.5 Dependencies and conflicts

**Gap.** A mod that requires a shared asset pack, or that is known to break beside another,
has no way to say so. A manager learns of a conflict at overlay build, by chunk collision.

**Proposal.** `depends: [{name, version_req}]` and `conflicts: [{name, version_req}]`, where
`name` is the other mod's machine name and `version_req` a semver requirement. A manager
orders enabled mods to satisfy `depends` and refuses or warns on `conflicts`.

**Cost.** Two optional keys, schema version 4. Needs a decision on what a machine name
identifies across distributors ([section 4.2](#s4.2)).

### <a id="s2.6"></a>2.6 Deterministic output

**Gap.** `ModpkgBuilder::collect_regular_chunks` sorts content chunks by WAD name then layer
name and leaves ties in hash-map order. The path table follows that order. One project packs
to different bytes on two runs.

**Proposal.** A writer rule in the spec: content chunks are ordered by `(wad, layer,
path_hash)`, meta chunks first in the order of `modpkg.md` [section 10](../design/modpkg.md#s10),
and the path and WAD tables follow first use. Reproducible bytes are a precondition for
signing ([section 3.4](#s3.4)), for content-addressed distribution ([section 4.1](#s4.1)) and
for CI that diffs a build against the last one.

**Cost.** A builder sort key and one spec rule. No format change.

### <a id="s2.7"></a>2.7 Compression level

**Gap.** The reference writer compresses at zstd level 3. Decompression cost does not depend
on the level, and pass-through keeps the frame, so every install pays for the level chosen
once at pack time.

**Proposal.** A writer recommendation: level 19 for a package built for distribution. A
packer MAY expose the level; the default for a release build is high.

**Cost.** A constant. No format change.

### <a id="s2.8"></a>2.8 String overrides out of the metadata document

**Gap.** String overrides are content and live in the metadata document. Every library
listing decodes them along with the name and version. A mod with many locales makes the one
document every listing reads large.

**Proposal.** Move them to a per-layer chunk, `_meta_/layers/<slug>/strings.msgpack`, with
the same `{locale: {field: replacement}}` shape. The metadata document keeps the layer list
without the overrides. Better: express them through the patch kind of [section 3.2](#s3.2),
as a patch over `data/menu/<locale>/lol.stringtable`, and retire the special case.

**Cost.** One reserved path now; a removed key at schema version 4. The overlay builder's
string merge is unchanged.

## <a id="s3"></a>3. Proposals that need format version 2

Each of these changes the header or the chunk record. A version 1 reader refuses the package
(`modpkg.md` [section 12.1](../design/modpkg.md#s12.1)).

### <a id="s3.1"></a>3.1 Feature flags

**Gap.** The header carries one version number and a reader refuses any it does not know.
Every additive change to the record strands every older reader.

**Proposal.** Two `u64` bitsets in the header after `version`: `required_features` and
`optional_features`. A reader refuses a package with a required bit it does not implement and
ignores optional bits. The pattern is ext4's compat/incompat flags and PNG's critical chunk
bit. Every later proposal in this section becomes a flag rather than a version.

**Cost.** Sixteen header bytes, format version 2.

### <a id="s3.2"></a>3.2 Chunk kind

**Gap.** A chunk can only replace the game's chunk. Two other operations have no expression:

- **Delete.** Removing a game chunk from its WAD. A VFX mod that silences one emitter, or a
  sound mod that removes one line, ships an empty or dummy file instead.
- **Patch.** Applying a change over the game's copy of a file rather than replacing it. Every
  bin override is a whole-file replacement and breaks on the first patch that touches the
  file. The declarative bin edits settled for issues 190 and 191, and the `PTCH` property
  patches specified in `league-toolkit`, need a chunk the overlay builder applies rather than
  copies.

**Proposal.** A `u8 kind` in the record: `0` replace, `1` delete, `2` patch. A delete record
has no data (`compressed_size = 0`). A patch record's content is in a format the path's
extension names (`.ptch` for a property patch); the overlay builder reads the game's chunk,
applies the patch, and writes the result. String overrides become a patch over
`lol.stringtable` ([section 2.8](#s2.8)).

**Cost.** One record byte, format version 2. The overlay builder gains an apply step per patch
format.

### <a id="s3.3"></a>3.3 Cryptographic content digests

**Gap.** Both checksums are XXH3-64, chosen to match the game's WAD TOC
(`modpkg.md` [section 7.2](../design/modpkg.md#s7.2)). XXH3 is not collision-resistant. A
signature over the TOC binds nothing about the content, and a library that deduplicates
chunks across mods by checksum can be made to serve one mod's bytes for another's.

**Proposal.** A BLAKE3 digest of the content bytes per chunk. Either a 32-byte record field
or, cheaper for readers that do not need it, a `_meta_/digests` meta chunk holding one digest
per record in TOC order. The digest table is optional; a signature ([section 3.4](#s3.4))
requires it.

**Cost.** 32 bytes per chunk. As a meta chunk: no record change and no version bump, only a
feature flag or a metadata key naming it. As a record field: format version 2.

### <a id="s3.4"></a>3.4 Signing

**Gap.** The `signature` field is reserved and empty. A redistributed package can be altered
and nothing detects it before the client does.

**Proposal.** `signature` holds an Ed25519 signature over the header (with `signature_size`
and `signature` treated as zero), the three tables, the TOC and the digest table of
[section 3.3](#s3.3). The signer's public key is a metadata key, `signing_key`. A manager
shows a verified author when the key matches one it trusts and refuses a package whose
signature does not verify. A mod site verifies on upload.

**Cost.** 64 signature bytes and one metadata key. Requires [section 3.3](#s3.3) and
[section 2.6](#s2.6).

### <a id="s3.5"></a>3.5 Metadata pointer in the header

**Gap.** Identifying a mod means reading the three tables and the whole TOC to find the record
of `_meta_/info.msgpack`. For a map mod that is several megabytes before the first metadata
byte. A site scanning uploads or a manager listing a large library pays it per package.

**Proposal.** `u64 metadata_offset` and `u64 metadata_size` in the header. A reader that only
identifies the mod does two reads. The TOC record stays, so a full mount is unchanged.

**Cost.** Sixteen header bytes, format version 2.

### <a id="s3.6"></a>3.6 Table of contents at the end

**Gap.** The TOC sits before the data with a fixed count. Adding, replacing or removing a
chunk rewrites the TOC and everything after it. `ltk_fantome` has `apply_delta` for a
repair that rewrites only a WAD's tail; modpkg has no equivalent and its layout makes one
cost the file. A writer needs `Seek` to fill the TOC placeholder, so a package cannot be
streamed to a socket.

**Proposal.** ZIP's layout: data first, then the tables and TOC, then a fixed-size trailer
holding their offset and the magic. A repair appends new data and rewrites the tables, TOC
and trailer. A writer streams without seeking. A reader seeks to the trailer first.

**Cost.** A different layout, format version 2. The metadata pointer of
[section 3.5](#s3.5) then lives in the trailer.

### <a id="s3.7"></a>3.7 Subchunk parity with WAD

**Gap.** A WAD 3.4 chunk can be `ZstdMulti`: several frames with a subchunk table the client
reads separately. A modpkg chunk is one frame. `ltk_fantome`'s delta refuses a subchunked
chunk for the same reason. Whether the client accepts a single frame where the original was
subchunked is not verified.

**Proposal.** First, measure: replace a subchunked texture with a single frame and run the
client. If it loads, no change. If it does not, add compression code `2` (Zstandard,
subchunked) with a subchunk length table in the chunk data, and let the overlay builder pass
the frames and table through.

**Cost.** Unknown until measured. A compression code and a data prefix if needed.

### <a id="s3.8"></a>3.8 Content chunks without a WAD

**Gap.** A content chunk with `wad_index = NONE` is legal, extracts at the layer root, and is
placed nowhere by the overlay builder. It exists for fantome `RAW/` imports.

**Proposal.** Either forbid it (a writer error, a reader refusal) or define it: the overlay
builder routes the chunk by the game index, as it routes a fantome `RAW/` file. The second is
one rule in the spec and matches what the overlay builder does for fantome.

**Cost.** A spec rule. No format change under either option.

## <a id="s4"></a>4. Proposals above the format

### <a id="s4.1"></a>4.1 Delta distribution

**Gap.** A new version of a map mod is a full download. The TOC already carries content
checksums, so a client could ask for only the chunks it lacks.

**Proposal.** A published manifest export, one row per record: `path_hash`, layer, WAD,
`uncompressed_checksum`, `uncompressed_size`, and the BLAKE3 digest of [section 3.3](#s3.3).
A site serves it beside the package. A manager holding the previous version downloads only
the runs whose digest it does not hold and assembles the new package locally. Reproducible
output ([section 2.6](#s2.6)) makes the assembled package byte-identical to the published
one, so the signature ([section 3.4](#s3.4)) verifies.

**Cost.** A site-side format and a manager-side assembler. No package change beyond the
digests.

### <a id="s4.2"></a>4.2 The package as the project's archive

**Gap.** Round-trip is one of the five jobs, and it is lossy for anything the project holds
outside `content/`: transformer sources, `game_data` declarations, `.modignore`, tool
dot-files. A package is the build of a project, not its archive.

**Proposal.** A reserved prefix `_project_/` carrying the project's own files verbatim:
`_project_/mod.config.json`, `_project_/.modignore`, transformer sources under their project
paths. An import writes them back. The overlay builder ignores the prefix. A package then
distributes the editable mod as well as its build, and a project can be reconstructed from
its release.

**Cost.** One reserved prefix and an import rule. Package size grows by the sources.

### <a id="s4.3"></a>4.3 Mod identity across distributors

**Gap.** `name` is a machine name chosen by the author, `distributor.mod_id` an identifier on
one site. Two authors can pick one name; one mod on two sites has two ids. Dependencies
([section 2.5](#s2.5)) and update checks need one stable identity.

**Proposal.** A `uuid` metadata key generated at project creation and carried through every
build. A dependency names a uuid. A site indexes by it.

**Cost.** One key, schema version 4, and a project manifest field.

## <a id="s5"></a>5. Rejected

- **Zstandard dictionaries.** A dictionary trained on property bins would shrink bin-heavy
  packages substantially. The client cannot use a dictionary, so a dictionary-compressed
  chunk cannot pass through and the overlay builder re-encodes every one. The format's
  defining property is that stored bytes are WAD bytes. Rejected.
- **Container-level compression** of the tables and TOC. A map mod's header is a few
  megabytes read once at mount. The saving is small and it costs a decode before the first
  table entry. Rejected until a mount is measured to be I/O bound.
- **Encryption.** Nothing in the toolchain's goals needs a package a manager cannot read.
  Rejected.

## <a id="s6"></a>6. Summary table

| ID    | Proposal                          | Job served      | Bump          | Depends on |
| ----- | --------------------------------- | --------------- | ------------- | ---------- |
| P2.1  | Game version targeting            | Identify        | schema 4      |            |
| P2.2  | Layer groups                      | List            | schema 4      |            |
| P2.3  | Layer thumbnails, gallery         | Identify        | none          |            |
| P2.4  | Generator (taken)                 | Identify        | schema 5      |            |
| P2.5  | Dependencies, conflicts           | Identify        | schema 4      | P4.3       |
| P2.6  | Deterministic output              | Round-trip      | none          |            |
| P2.7  | Compression level                 | Copy into WAD   | none          |            |
| P2.8  | String overrides as a chunk       | Identify        | none / schema 4 | P3.2     |
| P3.1  | Feature flags                     | all             | format 2      |            |
| P3.2  | Chunk kind: delete, patch         | Copy into WAD   | format 2      |            |
| P3.3  | BLAKE3 content digests            | Read one chunk  | none / format 2 |          |
| P3.4  | Signing                           | Read one chunk  | none          | P3.3, P2.6 |
| P3.5  | Metadata pointer                  | Identify        | format 2      |            |
| P3.6  | TOC at the end                    | Copy, Round-trip | format 2     |            |
| P3.7  | Subchunk parity                   | Copy into WAD   | measure first |            |
| P3.8  | Content without a WAD             | Copy into WAD   | none          |            |
| P4.1  | Delta distribution                | Identify        | none          | P3.3, P2.6 |
| P4.2  | `_project_/` archive              | Round-trip      | none          |            |
| P4.3  | Mod uuid                          | Identify        | schema 4      |            |
