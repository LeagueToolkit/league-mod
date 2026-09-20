# Research: declaring property-bin links and patches in a mod project (issue 190)

A research note, not a spec. It gathers the primary facts behind
[league-mod#190](https://github.com/LeagueToolkit/league-mod/issues/190), lays out the candidate
shapes a declaration can take, and recommends one. Every claim carries its source. Sources are
read at these revisions:

| Repository | Revision | Where |
| --- | --- | --- |
| `league-mod` | `049ab957a572b6c5df0370a6de9f3b4d9a8d7dbd` (`main`) | `/home/crauzer/dev/lol/league-mod` |
| `league-toolkit` | `11bb8ba2cf52154b99922891ab8ca89afb4e2dbc` (`origin/main`) | `/home/crauzer/dev/lol/league-toolkit`, via `git show origin/main:` |
| `league-toolkit` PR 227 | `0bc9d0ea352b8e28d5aeef3c589bd970ed290e38` (`feat/value-walk`, the checkout's HEAD) | same checkout |
| `ltk-manager` | `main` on 2026-09-02, fetched over HTTPS | not checked out locally |
| `league_structs` | `53c080995865d80b2f204b55777b3d1c9e206d8e` (`origin/main`, 2026-09-02) | `/mnt/x/lol/dev/league_structs` |

## <a id="s1"></a>1. The question

Issue 190 is titled "`ltk_mod_project`: Declarative property bin links". Its whole body:

> This feature will make it possible to mod .bin links in a declarative way. A mod project should
> be able to declare the links it wishes to add and their targets. The overlay builder can take
> the specified links and add them to the target .bin.

([issue 190](https://github.com/LeagueToolkit/league-mod/issues/190), opened 2026-08-16 by
Crauzer, label `research`, assignee Crauzer, no comments.)

The subject is the **linked-file list** a property bin carries in its header - the
`dependencies` field of `ltk_meta::Bin`, printed by ritobin as `linked: list[string]` - and not
the `PTCH` property-patch records. Three things are asked for:

1. A mod project declares **links to add** and, for each, the **target bin** they are added to.
2. The declaration is **declarative**: data in the project, not a modified copy of the bin.
3. The **overlay builder** performs the addition at build time.

The sibling issue, [league-mod#191](https://github.com/LeagueToolkit/league-mod/issues/191)
"`ltk_mod_project`: Declarative PTCH targeting" (opened 2026-08-16, no comments), asks for the
same declaration shape for a different payload:

> We can support a lite version of .bin delta patching by processing custom PTCH containers
> provided by a mod. The patches can be declared in the project config and list targets to
> override. The overlay builder can then take those files and apply their patches to the
> specified targets using the same rules the game client uses.

The two issues share one declaration surface - "a mod-side artifact, a list of target bins, an
overlay-side application step" - and differ in the artifact: 190's is a list of path strings,
191's is a `PTCH` file. This note answers 190 and keeps the container open for 191.

## <a id="s2"></a>2. Facts established

### <a id="s2.1"></a>2.1 What a link is on the wire

- A `PROP` header is `'PROP'`, `version: u32`, and for `version >= 2` a `count: u32` followed by
  `count` length-prefixed strings (`len: u16`, UTF-8 bytes). The object table follows. A `PTCH`
  wraps the same header behind `'PTCH'`, `version: u32`, `deleteCount: u32` and the delete list.
  Sources: `crates/ltk_overlay/src/linked_bins.rs:129-175` (`parse_linked_bins`, with the layout
  in its doc comment); `league-toolkit` `docs/design/ptch-property-patches.md`
  [section 4](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s4)
  (lines 161-184 of the file at `origin/main`).
- A link is a **path string**, never a hash. `ltk_meta::Bin::dependencies: Vec<String>`, documented
  as "List of other property bins this file depends on. Property bins can depend on other property
  bins in a similar fashion to importing code libraries."
  (`league-toolkit/crates/ltk_meta/src/tree.rs:52-56`). `Bin::add_dependency` appends one
  (`tree.rs:160-163`); `Bin::to_writer` always writes version 3 and then the list
  (`league-toolkit/crates/ltk_meta/src/tree/write.rs:37-45`).
- A `PTCH` must declare **zero** links: "`dependencyCount` must be 0: the client reads the count
  and never skips the strings behind it, so any non-zero value desyncs the loader"
  (`ptch-property-patches.md` [section 4](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s4);
  rule D3 in [section 17](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s17)).
  `BinOverride` has no dependencies field at all (`ptch-property-patches.md`
  [section 5.1](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s5.1)).
  A link addition targets a `PROP` bin only.
- A `PTCH` "is never loaded as a base file and cannot be pulled in through a `linked` entry"
  (`ptch-property-patches.md` [section 4](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s4),
  last bullet). A declared link names a `PROP` bin.
- Shipped `PROP` files exist at version 2 and 3 ("`PROP` v2 files exist in the wild", rule D2;
  "the base may be v2 or v3", PRD-001
  [section 5](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/prd/001-ptch-property-patches.md#s5)).
  A version-1 header has no link list; `parse_linked_bins` returns an empty list for it
  (`linked_bins.rs:157-160`).
- **What the client does with the list at load time** is traced in the `league_structs`
  reversing notes ([section 2.9](#s2.9) carries the detail). The parser hands each string to
  `BinFileCache_acquireByPath`; the key is `Asset_HashName` = ASCII lowercase (`A-Z` + 32) then
  plain XXH64 with seed 0; nothing prepends `DATA/`, appends `.bin` or retries; a miss returns
  false and the dependency is dropped with no log line
  (`docs/reversing/AssetNamespacing_Repathing.md` section 2.1, lines 69-83;
  `docs/reversing/BinLoadPoints_DataOnly.md` section 1, lines 31-53). The client accepts a base
  `PROP` at version 2 or 3 only: `if (version - 2 > 1) return 0`
  (`BinLoadPoints_DataOnly.md` line 35). The loop is sequential in declaration order and the
  loaded entries land in a hash-sorted table afterwards; whether duplicate or self-referential
  links are rejected is flagged as unverified there (`BinLoadPoints_DataOnly.md` lines 265-267).
  The dependency list does not scope `Link` resolution - that is a cache-wide first-hit sweep
  (`AssetNamespacing_Repathing.md` section 2.2, lines 91-106). `ltk-manager`'s event contract,
  "missing linked bins are non-fatal at injection"
  (`ltk-manager/crates/ltk-manager-core/src/patcher/events.rs:25-28`), matches the traced
  behaviour. **Unverified:** de-duplication of a repeated string, and what happens to a
  self-referential link. No `ltk-wiki` page mentions links (grep over the local checkout found
  nothing).

### <a id="s2.2"></a>2.2 What `league-toolkit` PR 227 adds

PR 227 is "feat(ltk_meta): value walk" (`feat/value-walk` -> `main`, opened 2026-09-02, no body,
one review by alanpq requesting changes with one inline comment on the naming of `holds_node` /
`is_node` in `crates/ltk_meta/src/property/enum.rs`). Files: `crates/ltk_meta/src/walk.rs` (+670),
`walk/owned.rs` (+318), `walk/tree.rs` (+296), `walk/view.rs` (+290), `walk/tests.rs` (+1143), plus
edits to `lib.rs`, `stream/view.rs`, `stream/view/value.rs`, `tests/corpus.rs`, `README.md`,
`docs/LTK_GUIDE.md`, `docs/design/value-walk.md` and the ticket
`.scratch/value-walk/issues/01-walk.md` (issue 225).

What it is: "One read-only traversal over every node of a bin object, driven by a `Visitor`.
The walk is written once, against two sealed traits - `TreeValue` and `TreeNode` - that the owned
tree (`&PropertyValueEnum<M>`) and the streaming view (`ValueView`) both implement"
(`crates/ltk_meta/src/walk.rs:1-16` at the PR head). Its surface is `Visit` (`Abort`, `Stop`,
`Skip`, `Continue`), `WalkOutcome`, the `Visitor<'a, V>` trait with `enter_node` / `exit_node` /
`enter_property` / `exit_property` defaults (`walk.rs:81-140`), and `Bin::walk`, `BinObject::walk`,
`ObjectView::walk`, `BinStream::walk`. `docs/design/value-walk.md`
[section 4](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/value-walk.md#s4)
specifies `ValuePath`, `Step` and `MapKey` - the address a walk reports with - as ticket #219.

Its consumers are named in the ticket: `ltk-manager`'s problems pass, `Bin::merge` (#220) and
`Bin::diff` (`.scratch/value-walk/issues/01-walk.md`, "The consumer is ..."). **Nothing in PR 227
touches the link list.** It matters to issue 190 only indirectly: the walk is the traversal
`Bin::merge` is built on, and merge is what the overlay build runs under `ltk-manager` ADR-0012
([section 2.4](#s2.4)).

### <a id="s2.3"></a>2.3 What `bin-streaming.md` constrains

`docs/design/bin-streaming.md` at `origin/main`. Its preamble (lines 14-16) lists the `PROP` half
as implemented and the `PTCH` stream (#210) and the delta write-back (#211) as unbuilt:

- **Mount reads the links for free.** "Mounting reads the header, dependencies and class-hash
  table - all sequential, no seeking - and stops" ([section 1](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/bin-streaming.md#s1));
  `BinStream::dependencies(&self) -> &[String]` is a header fact "free after mount"
  ([section 4](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/bin-streaming.md#s4),
  lines 150-152).
- **The delta rewrite is the shape a link addition needs.** `BinDelta` carries
  `dependencies: Option<Vec<String>>` - "`None` keeps the base's dependency list" - and
  `BinStream::write_patched(&mut self, delta: &BinDelta<M>, out)` writes the base with the delta
  applied: "Header and class table are rebuilt for the final entry set; every untouched object is
  raw-copied **byte for byte** from its `ObjectEntry` range" ([section 10.2](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/bin-streaming.md#s10.2),
  lines 775-798). Invariants: "Untouched means bit-identical", "The version passes through",
  "A legacy-latched base refuses the delta write", "Size mismatches cannot reach this path"
  ([section 10.3](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/bin-streaming.md#s10.3)).
  A link addition is the degenerate delta: `dependencies: Some(base + new)`, no objects touched.
- **Not `PTCH` authoring, not in-place.** "The write always produces a complete new stream"
  ([section 10.4](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/bin-streaming.md#s10.4)).
- **Strictness.** A declared size the walk disagrees with is `Error::InvalidSize`
  ([section 7](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/bin-streaming.md#s7),
  rule S11). A link-rewriting build that goes through `ltk_meta` inherits this: a corrupt
  mod-shipped bin fails to rewrite rather than passing through.
- **Every sized region is skippable**; the header is not a sized region and is parsed
  sequentially ([section 3](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/bin-streaming.md#s3)).
  A header-only rewrite (read header, write header with more strings, copy the rest verbatim) is
  sound for version 2 and 3 bins and needs no object parse. For a version-1 base the count field
  does not exist; adding one changes the version.

### <a id="s2.4"></a>2.4 What `ptch-property-patches.md` and PRD-001 constrain

- **Route 3 is the declared-target route, and it is league-mod's.** PRD-001
  [section 5](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/prd/001-ptch-property-patches.md#s5)
  lists three delivery routes for a patch: replace one Riot registers, hang one off an existing
  `UiPropertyOverrideLoadable` link property, or "**Declare the target and let a build apply it**
  (`league-mod` #191). Not a client mechanism, and the only route that generalises: any bin can be
  a target because nothing registers at runtime." Story 4 of
  [section 3](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/prd/001-ptch-property-patches.md#s3):
  "As **a mod build tool** (`league-mod`), I want to name a patch and its target declaratively".
  The toolkit side places the declaration in this repository.
- **Merge is the overlay's operation, and it unions links.** `Bin::merge` "layers one bin over
  another in place ... It is what `ltk-manager`'s overlay build runs (its ADR-0012)". In the merge
  walk, "Dependencies merge as a union: `base`'s list in its order, then anything only `edited`
  has" (`ptch-property-patches.md` [section 10.1](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s10.1),
  line 801). A mod that ships its own copy of a bin with extra links gets those links added to the
  game's list by merge alone, with no declaration. A declaration is what a mod needs when it does
  **not** want to ship the bin.
- **Merge is not built.** `ltk-manager`'s ticket 015-004 ("Overlay merge implementation") records
  that ADR-0012 "was accepted on 2026-08-31 and nothing implements it", that "`ltk_overlay` does
  not parse bins ... `linked_bins.rs` reads bin headers by hand with `byteorder` ... and the crate
  has no `ltk_meta` dependency at all", and that "the cost of 'recomputed on every build' is
  unpriced" (`ltk-manager/specs/015-game-as-parts-source/issues/004-build-the-overlay-merge.md`).
- **Apply is the client's semantics, per record, non-fatal**; `check` answers "does this patch
  still apply to this version of the base" ([section 9.5](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s9.5)).
  `join` concatenates several overrides over one target and reports collisions without
  resolving them: "which override should win is the caller's policy, and a manager that knows the
  user's load order has more to go on than this crate does" ([section 13](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s13),
  rule D23). Load order is exactly what `ltk_overlay` holds ([section 2.6](#s2.6)).
- **A wildcard declaration needs a distinct outcome.** "If declarative targeting grows wildcard
  support, 'this bin is not this record's target' has to become an outcome distinct from 'this
  record is broken'. Not yet ticketed." ([section 9.5](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s9.5),
  last paragraph).
- **No schema enters `ltk_meta`** (ADR-0006, rule D25). A declaration format cannot lean on
  Riot's meta classes for validation.
- **The path language is Riot's.** `PropertyPath` is `name`, `[index]` (decimal, `0x` hex or
  `0` octal, `strtol` base 0) and `{key}` (a JSON number, string or boolean), segments joined
  by `.`; a name is any byte but `.[]{}()` and controls; one subscript per segment. A name
  matches by `FNV1a32(lowercase(name))`; `{key}` converts the JSON scalar to the map's key
  kind, hashing a string for `Hash` or `WadChunkLink` keys; a Pointer is dereferenced, an
  Embed descended, a Link is a leaf
  ([section 8.1](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s8.1);
  `crates/ltk_meta/src/path.rs:1-16, 268-330`; `crates/ltk_meta/src/path/parse.rs:107-109`;
  `crates/ltk_meta/src/path/resolve.rs:228-280`). A `0x...` segment name is text to the
  client and hashes as such; the language has no hash escape.
- **Path limits.** `PropertyPath::MAX_LEN` is `u16::MAX`; shipped paths top out at 48 bytes
  ([section 5.3](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s5.3),
  [section 8.2](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s8.2)).
  A link string has the same `u16` length prefix on the wire ([section 2.1](#s2.1)).
- **ritobin text already has a shape for both.** `linked: list[string] = { ... }` is a root entry
  of every ritobin file, and `patches: map[hash,embed]` is the text form of `PTCH` records
  ([section 15](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s15);
  `league-toolkit/crates/ltk_ritobin/src/print.rs:180`). `ltk_ritobin` parses it
  (`typecheck/state.rs:89`, `RootKind::Linked`).

### <a id="s2.5"></a>2.5 What the mod project format expresses

The manifest is `mod.config.json` or `mod.config.toml` (`crates/ltk_mod_project/src/config_format.rs:11-16`),
deserialized into `ModProject` (`crates/ltk_mod_project/src/lib.rs:264-333`). Its fields: `name`,
`display_name`, `version`, `description`, `authors`, `license`, `tags`, `champions`, `maps`,
`transformers`, `layers`, `thumbnail`, `hashtables`. Each layer is a `ModProjectLayer` with
`name`, `display_name`, `priority`, `description`, and
`string_overrides: IndexMap<String, IndexMap<String, String>>` - "Outer key: locale ... Inner map:
field name (from lol.stringtable) -> new string value" (`lib.rs:493-521`).

Two precedents for "data the build acts on" already live in the project:

1. **`string_overrides`, inline per layer.** The manifest holds them; the overlay builder applies
   them over the game's own stringtable; a mod-shipped `lol.stringtable` chunk is rejected
   (`crates/ltk_overlay/src/strings.rs:1-36`). The modpkg format carries them in
   `ModpkgLayerMetadata::string_overrides` (schema v2; `crates/ltk_modpkg/src/metadata.rs:98-123`)
   and the modpkg content provider reconstructs `ModProjectLayer` from that metadata
   (`crates/ltk_overlay/src/modpkg_content.rs:36-57`). Fantome stores them in `info.json` under
   `Layers.<name>.StringOverrides` (`crates/ltk_fantome/DESIGN.md:26-71`).
2. **`hashtables`, a manifest entry pointing at a file.** The config declares
   `{path, category, algorithm, bits}`; the file lives under `hashes/`, "Deliberately outside
   `CONTENT_DIR_NAME`: a table is never a packing candidate and never meets `.modignore`"
   (`lib.rs:53-58`, `lib.rs:324-332`). The packer reads and validates every table before any
   format sees the plan (`crates/ltk_mod_project/src/pack/plan.rs:87-96`), and modpkg stores them
   as `_meta_/hashes/{file name}` chunks (`crates/ltk_modpkg/src/hashtable.rs:15-18`).

Content addressing: files under `content/<layer>/<Wad>.wad.client/<chunk path>` are chunks
(`CONTEXT.md:20-30`); `content/base/raw/` holds files "named by game asset path rather than by
their location inside a WAD, and are routed to a WAD when an overlay is built" (`lib.rs:578-584`).
A chunk path hashes as `ltk_modpkg::ChunkPath` after lowercasing, and a 16-hex-digit file name is
taken as the hash itself (`crates/ltk_overlay/src/content.rs:89-94`). `CONTEXT.md:170-173` names a
**file property** as "A `.bin` property whose value is the xxh64 of a path - the same hash space
chunks are addressed in".

Packing: `ProjectPacker` builds a `PackPlan` of `PlannedLayer` / `PlannedFile` /
`PlannedHashtable` and a `PackFormat` encodes it (`crates/ltk_mod_project/src/pack/mod.rs:1-103`).
"A format that does not store some part of the plan (Fantome keeps only the base layer, for
example) skips it rather than failing" (`pack/mod.rs:70-73`). The modpkg writer copies
`layer.string_overrides` into layer metadata (`crates/ltk_mod_project/src/modpkg/format.rs:422`);
modpkg meta chunks carry `layer_index == NONE` and `wad_index == NONE`
(`crates/ltk_modpkg/src/chunk.rs:32-48`), and the current metadata schema is 3
(`metadata.rs:244-245`). The CLI's `pack` warns that Fantome drops non-base layers
(`crates/league-mod/src/commands/pack.rs:181-214`). `league-mod init` writes a project with
`ModProjectLayer::default_table()` and no other build-affecting data
(`crates/league-mod/src/commands/init.rs:94-110`).

### <a id="s2.6"></a>2.6 What the overlay builder does with bins

- **Pass 1 parses every override's link list.** `build_override_meta` fills
  `OverrideMeta::linked_bins` from `parse_linked_bins(bytes)` for every override, layer and raw
  alike (`crates/ltk_overlay/src/builder/metadata.rs:61-74`); the field is documented as "Parsed
  once in pass 1 and cached so the linked-bin pre-flight needs no re-decompression"
  (`crates/ltk_overlay/src/builder/mod.rs:117-120`) and persisted in the per-mod cache
  (`crates/ltk_overlay/src/meta_cache.rs:57-60`, cache version 6).
- **The linked-bin pre-flight.** After routing, `collect_linked_bin_offenders` checks each
  declared link against "every chunk path the game will find once the overlay is in place" - the
  union of routed overrides and the game index minus blocked WADs - and reports
  `LinkedBinOffender { mod_id, wads, missing_links }` (`linked_bins.rs:35-127`;
  `builder/mod.rs:813-827`). The result is persisted in `overlay.json` as `linkedBinOffenders`
  (`crates/ltk_overlay/src/state.rs:143-147`) and is "the only cross-mod advisory the build
  produces" (`docs/overlay-builder-design.md:213-216`).
- **The synthetic-override precedent.** String overrides enter the build as
  `OverrideSource::StringPatch { chunk_path }`: "Its bytes are generated at resolve time (base
  chunk + key-level overrides) rather than re-read from a provider, so this variant never enters
  the per-mod metadata cache" (`builder/mod.rs:60-85`). `StringPatchPlan` is computed in pass 1
  with a deterministic `fingerprint()` used as the synthetic `content_hash`
  (`strings.rs:130-162`), `to_override_meta()` sets `fallback_wad` to the target WAD
  (`strings.rs:200-214`), `apply(base_bytes)` parses the game's chunk and writes the patched
  one (`strings.rs:164-198`), and `read_game_chunk` mounts the game WAD and decompresses one
  chunk (`strings.rs:311-331`). Pass 2 classifies chunk sources into `by_mod` and
  `string_patches` (`crates/ltk_overlay/src/builder/resolve.rs:31-68`). Merge order: "Across mods,
  the mod closer to the front of the enabled list wins. Within a mod, layers apply in ascending
  priority order" (`strings.rs:15-29`; `collect_effective_overrides`, `strings.rs:240-269`).
- **The lazy-override filter** drops a mod file byte-identical to the game's copy
  (`metadata.rs:386-460`). A mod that ships an unmodified bin as the base for its own declared
  links loses that bin to the filter unless the link step runs first or the filter is taught the
  exception.
- **Load order is the only conflict rule.** "Overlapping overrides resolve by load order - the
  first mod in the list wins - and nothing reports the overlap" (`overlay-builder-design.md:213-216`).
  `EnabledMod` position 0 has the highest priority (`builder/mod.rs:180-199`).
- **Pass 2 compresses once per content hash** and the writer takes a resolve callback
  (`resolve.rs:101-139`; `overlay-builder-design.md:39-69`).
- **The crate depends on `ltk_wad`, `ltk_file`, `ltk_mod_project`, `ltk_modpkg`, `ltk_fantome`,
  `ltk_rst`** and not on `ltk_meta` (`crates/ltk_overlay/Cargo.toml:14-22`). `ltk_file` is used
  once, for codec choice (`crates/ltk_overlay/src/wad_builder.rs:68`).
- **Two ADRs bound the build.** Pass-through recomputes checksums and never fails
  (`docs/adr/0001-pass-through-recomputes-checksums-and-warns.md`); the builder never mutates a
  source archive (`docs/adr/0002-normalization-happens-at-import-never-at-build.md`). A bin
  rewritten for links is a new chunk the build synthesizes, as the stringtable is; neither ADR is
  engaged.

### <a id="s2.7"></a>2.7 How LTK Manager consumes it

- The manager constructs `Vec<ltk_overlay::EnabledMod>`, calls `with_string_overrides`,
  `set_enabled_mods`, `build`, and drains `take_linked_bin_offenders()` into its own result
  (`ltk-manager/crates/ltk-manager-core/src/overlay/build.rs:27-74`, from a grep of the raw file).
  Offenders surface as `linked_bin_warning(count)`, "Advisory only: missing linked bins are
  non-fatal at injection, so the session carries on"
  (`ltk-manager-core/src/patcher/events.rs:25-28`), and reach the UI as
  `src/lib/bindings/LinkedBinWarningPayload.ts`.
- The manager reads a mod's layers through `ModContentProvider::mod_project()`; for a `.modpkg`
  that is `ModpkgContent::mod_project`, which rebuilds each `ModProjectLayer` from
  `ModpkgLayerMetadata` (`modpkg_content.rs:36-57`). **Any per-layer declaration reaches the
  manager only if it is a field of `ModProjectLayer` and of `ModpkgLayerMetadata`.**
- ADR-0012 fixes the build-time merge as the manager's repair for a bin the mod replaces:
  "Where the mod says nothing, the game's content survives ... It applies only where the mod
  overrides a chunk the game also has ... It is recomputed on every build"
  (`ltk-manager/docs/adr/0012-the-overlay-merges-a-mod-over-the-games-copy.md`). Its
  `CONTEXT.md` defines **Merge** and **Compensated** (lines 165-169, 208-212). ADR-0012 also states
  "If mods ever ship deltas rather than replacements, a shipped patch record and this build-time
  merge are the same semantic at different times, and neither needs a second vocabulary."
- `CLAUDE.md` in this repository: "An API question the manager raises is settled in this repo's
  spec, and the manager cites it."

### <a id="s2.8"></a>2.8 Other tools

- **Fantome** has no declaration surface for bin edits. Its format is `RAW/`, `WAD/` and `META/`
  (`info.json`, `image.png`); "Do not place new files into [`RAW`]" (Fantome wiki, *Mod File
  Format*). `ltk_fantome` adds `Layers` and `StringOverrides` to `info.json`
  (`crates/ltk_fantome/DESIGN.md:26-64`).
- **cslol-manager** is "in maintenance/deprecation mode" and describes itself as a Fantome-format
  manager with "Create mods from RAW folders. Pack/Unpack `.wad` files" (its README at `main`).
  No bin-editing declaration is documented there. Its source was not read.

### <a id="s2.9"></a>2.9 Hardcoded data points, namespacing and resolvers

Facts from the `league_structs` reversing notes at `53c0809`. File paths below are relative to
`docs/reversing/` in that repository; struct citations are to `include/`, whose umbrella header
`include/league_structs.hpp` pulls in `Character/CharacterRecord.hpp` (line 43),
`Environment/MapSkin.hpp` (line 51) and `Vfx/ResourceResolver.hpp` (line 80). The notes mark
each claim `[traced]` (read off the disassembly), `[attested]` (confirmed against shipped data),
`[inferred]` or `[design]`; the marks are carried over where the source gives them. Client
builds cited are 16.13 to 16.17; addresses drift per build and are quoted only as the notes'
identifiers.

#### <a id="s2.9.1"></a>2.9.1 Two namespaces

- A chunk is keyed by `XXH64(lower(path))` in one flat WAD keyspace; a bin entry is keyed by
  `FNV-1a32(lower(name))`, global across every loaded container. Chunk paths are generated by
  Riot's exporter and churn by construction (a merged `_multi_` bin's filename encodes the skins
  merged into it); entry names are hand-authored and stable, and a `Link` is the FNV-1a32 of the
  entry name and nothing else (`AssetNamespacing_Repathing.md` section 1, lines 26-61,
  `[attested]`).
- `Asset_HashName` (the cache key) lowercases `A-Z` only and does not normalise `\` to `/`;
  `Asset_HashWadPath` (the content lookup) does both. A backslash-spelled dependency opens the
  same chunk and caches it under a key nothing else computes; shipped data has 0 backslashes in
  30,880 dependency strings and 0 in 2,291,347 named chunks. The note's rule for a packer: "Do
  not accept backslashes in chunk names" (`AssetNamespacing_Repathing.md` section 5, lines
  234-271, `[traced]`, in-game effect `[inferred]`).
- Entry lookup with `followDeps = 0` searches this container's sorted entry table, then the
  thread-local "currently parsing" container, then **every loaded `BinFile` in the cache, first
  hit wins**. The dependency list guarantees the target file is loaded before links resolve; it
  does not scope resolution. An entry name that collides with Riot's is resolved by load order
  (`AssetNamespacing_Repathing.md` section 2.2, lines 91-106, `[traced]`). By-name lookups from
  gameplay code (`BinFileCache_findEntryByHash` with a scope hash) search one container only
  (`SkinResolution_Skin0Fallback.md` section 3, lines 80-97).
- Of 2,291,347 named chunks, 20 first path segments are real directories (`assets`, `data`,
  `loadouts`, `characters`, `levels`, `global`, `clientstates`, `uiautoatlas`, `shaders`,
  `maps`, `gameplay`, `ux`, `common`, `lcu`, `patching`, `loadingscreen`, `missions`,
  `rewards`, `pregame`, `passes`); `mods/`, `mod/`, `ltk/`, `custom/`, `overlay/`, `user/`,
  `local/`, `patch/`, `inject/` and `third_party/` have zero shipped names
  (`AssetNamespacing_Repathing.md` section 3 rule 1, lines 132-147, `[attested]`).
- Every failure on the load path is silent: a missing dependency is dropped, a property whose
  type differs from the registered type is skipped, an unresolved `Link` stays null, a bin that
  parses to zero entries is destroyed and not cached, a hash-targeted override with a wrong key
  never applies (`AssetNamespacing_Repathing.md` section 2.4, lines 116-126). A zero-entry but
  well-formed `PROP` reports success to the acquirer, which is what defeats the Skin0 fallback
  (`SkinResolution_Skin0Fallback.md` section 4, lines 113-139).

#### <a id="s2.9.2"></a>2.9.2 Three hash functions

| hash | input | used for | source |
| --- | --- | --- | --- |
| XXH64, seed 0 | lowercased chunk path | WAD chunk keys, `File`-typed properties, `PropertyLoadable::FilepathHash`, `PropertyOverrideLoadable` both fields, `linked` strings | `AssetNamespacing_Repathing.md` lines 69-83; `BinLoadPoints_DataOnly.md` section 2a |
| FNV-1a32 | lowercased name | class names, field names, bin `Hash` values, entry names (`Link`), **resolver keys**, `MetaPath_resolve` property names | `ResourceResolvers_VfxEffectKeys.md` section 9.1-9.2, lines 726-764; `PTCH_PropertyPatches.md` section 1 |
| FNV-1 32 | lowercased literal in the exe | engine-internal runtime keys: `MissingInstant` / `MissingLingering` placeholders, `AREA_*` region tags, Wwise ids | `ResourceResolvers_VfxEffectKeys.md` section 4.1 lines 255-262, section 9.2 |

"FNV-1a32 is the serialization hash. FNV-1 32 is the runtime string-key hash. If the number
lives in a `.bin`, it is FNV-1a" (`ResourceResolvers_VfxEffectKeys.md` line 758). A wrong
variant is a silent miss (section 9.4, lines 796-798). All 401,473 CommunityDragon
`hashes.binhashes.txt` entries reproduce under FNV-1a32 lowercased (line 881-882).

#### <a id="s2.9.3"></a>2.9.3 Hardcoded data points

A *hardcoded data point* is a name the client formats from a template in code or hashes from a
literal, rather than reads from a bin. The table lists every one the sources describe, where it
is resolved, and what a bin edit can do to it. "Reach" uses the note's three edit kinds: a
**link** (issue 190), a **PTCH override** record (issue 191; per `LeagueModding.md` section 4.3,
lines 401-427, a record can write any property at any depth, repoint a `Link`, replace a whole
container, and add whole objects; it cannot delete, grow a list through `[i]`, change an entry's
class, be a base file, arrive as a `linked` dependency, or reach outside its own bin), and a
**whole object** (a shipped copy of the bin, candidate D).

| # | data point | template or literal | resolved by | reach | source |
| --- | --- | --- | --- | --- | --- |
| H1 | skin bin file | `DATA/%s/%s/Skins/Skin%u.bin` with root `"Characters"` (global `AString`); fallback `Skin0.bin` when `skinId != 0`; then `DATA/Characters/<C>/<C>.bin` | `SkinData_resolveBinAndEntry` `0x140442090`, `BinHolder_acquireByPath` | The chunk at that path is replaceable (an override, or absent to force the fallback). The template cannot point elsewhere from data. | `SkinResolution_Skin0Fallback.md` section 1, lines 33-61, `[traced]`; `LeagueModding.md` section 6.4, lines 605-606; `BinLoadPoints_DataOnly.md` section 4, lines 217-219 |
| H2 | skin properties entry | `Characters/<name>/Skins/Skin<N>`, then the alias's `Characters/<alias>/Skins/Skin<N>`, then `Characters/<name>/Skins/Skin0` (literal `0`) - scoped to the loaded bin | `CharacterData_resolveSkinProperties` `0x1403F5330`; `BinFileCache_findEntryByHash` `0x1411AE280` with a scope hash | PTCH: any property of the `SkinCharacterDataProperties` object, including `mResourceResolver` (Link), `mAdditionalResourceResolvers` (List<Link>), `SkinParent` (I32). The entry must exist in that bin. | `SkinResolution_Skin0Fallback.md` section 2-3, lines 63-111; `SkinParent_ChromaSystem.md` sections 2-3, lines 27-67 |
| H3 | alias character | `CharacterRecord` field at `+80`, named `mFallbackCharacterName` in the struct header | `Character_resolveNameAliasFromCharacterRecords` `0x1403EC570` reading the record chosen by H4 | PTCH on the `CharacterRecord` object (String property). | `SkinResolution_Skin0Fallback.md` lines 74-78; `CharacterRecords_OverrideChain.md` section 6, lines 200-207; `include/Character/CharacterRecord.hpp:44-45` |
| H4 | character record override chain | `Characters/<char>/CharacterRecords/<token>` with tokens `Map%u` (map id from `g_GameConfig`), `CharacterDataOverride` (level property), `modeTok` (caller-supplied), seven concatenations in descending specificity, then `.../Root`; scoped to the character's own container; whole-record swap | `CharacterRecord_resolveOverrideChain` `0x1404001A0`, `BinContainer_findEntryByPath` `0x1403D2B40` | Whole object: a PTCH layer over the character's bin can add a complete `CharacterRecord` entry (layer objects enter the merged entry table). `Map<id>` needs no mutator; "Nothing ships one today". A foreign container cannot inject one. | `CharacterRecords_OverrideChain.md` sections 1-2, 7, lines 36-99, 209-219, `[traced]` + `[attested]`; `LeagueModding.md` section 4.3 "Add whole objects" |
| H5 | `CharacterDataOverride` token | level-property bag key, no registration, no default; one shipped producer: `GameMutatorExpansions` in `globals.bin` (`Global.wad.client / 2f30c6f7747eb925`) assigning `JADE`, `NEXUSBLITZ`, `ULTBOOK`, `URF` | `LevelProps_GetString` `0x1404C5490` | PTCH on `GameMutatorExpansions` (`mMutators` is `list2[string]`; a whole-list replacement carries the append). Server authority applies to gameplay. | `CharacterRecords_OverrideChain.md` section 4, lines 130-151; `GameMutators_LevelProperties.md` section 7, lines 441-491 |
| H6 | per-skin resolver entry | `%s/%s/Skins/Skin%u/Resources` built from the **original** skin id (no Skin0 fallback); scoped; a null result is tolerated and inherited from the parent skin's `SkinData` when one exists | `SkinData_resolveBinAndEntry`, `SkinDataCache_linkParentResolver` `0x140448230` | PTCH on the `ResourceResolver` object: `resourceMap` (Map<Hash, Link>) key-level via `{k}` (`[inferred]`, zero shipped examples) or whole-map replacement. Consumers reach the resolver through H2's `mResourceResolver` link; repointing that link to a mod-owned resolver is the other route. | `SkinResolution_Skin0Fallback.md` lines 52-57, 179-202; `ResourceResolvers_VfxEffectKeys.md` section 6.3, lines 470-473 |
| H7 | resolver key | `FNV-1a32(lower(effectName))`, the skin-independent name a spell script asks for; the value is a skin-specific `Link<IResource>`; a present key with a null link is a hit that suppresses; duplicate keys collapse, first-merged wins after `finalize` | `BaseResourceResolver_tryResolve` `0x141245600`, `VfxCreation_resolveEffectKey` `0x1404690B0` tiers: per-object scopes (array in reverse, then primary), caller list, two static scopes (spell aggregate, map skin), `sGlobalResolvers` in registration order, miss | PTCH on any `resourceMap`; the key namespace is whatever code and spell data request - a mod-invented key is inert unless something asks for it. Non-`VfxSystemDefinitionData` targets are a type-confusion hazard, not a graceful failure. | `ResourceResolvers_VfxEffectKeys.md` sections 2.1, 4.3, 7.6, lines 65-132, 337-370, 661-674; `include/Vfx/ResourceResolver.hpp:92-136` (`ResourceMapEntry`, `ResourceMap`, `BaseResourceResolver`), `:154-164` (`ResourceResolverComponent`) |
| H8 | resolver owners | 28 properties on 24 classes: typed (`SkinCharacterDataProperties.mResourceResolver` Link, `.mAdditionalResourceResolvers` List<Link>, `MapSkin.mResourceResolvers` List<Link>, `ItemData.mVFXResourceResolver` Pointer, ...) and by bare `Hash` (`SpellDataResource.mResourceResolvers` List<Hash>, `TFTDamageSkin.VfxResourceResolver` Hash, ...) | the property loader fills the field; `MetaClass_ResourceResolver` has no `BinObjectLink_resolve` caller; the runtime holds a plain pointer | PTCH: a `Link` or `Hash` repoint per owner. A `List<Link>` grows only by whole-list replacement. | `ResourceResolvers_VfxEffectKeys.md` sections 6.1-6.4, lines 424-490; `include/Spell/SpellDataResource.hpp:196` (`+1624 mResourceResolvers List<Hash>`) |
| H9 | `GlobalResourceResolver` | one shipped object, entry `0xd2343ed1` in the extensionless chunk `shared` (`XXH64("shared") = 0x5a791d818ae738a6`, `Global.wad.client`), 359 entries; instances self-register through vtable `+24` into `sGlobalResolvers` (`0x141FCCC78` in 16.17); force-load skipped in TFT | `GlobalResourceResolver_tryResolveAcross` `0x141245490`, tier 4 | PTCH on that chunk's `resourceMap`. Whether the client loads that object at all is open: "nothing links to it, its hash is not an immediate in the exe" (line 927-929). | `ResourceResolvers_VfxEffectKeys.md` sections 3-3.1, 6.3, 7.1, lines 156-209, 470-473, 515-517, 927-929 |
| H10 | per-mode extra bins | `"%s.bin"` over `GameModeMapData.AdditionalPropertyDataPaths[i]` (`list[string]`), loaded through `BinFileCache_loadByPath` in list order and released on unload | `sub_1404B5F00` load / `sub_1404C9590` unload | PTCH: whole-list replacement carrying the append. The only string-path load point besides `linked` whose target a mod names freely. Missing-file behaviour unverified. | `BinLoadPoints_StringPaths.md` section 1, lines 24-131, `[attested]` |
| H11 | UI state bins | `"%s.bin"` from a `ClientState` literal (`Gameplay`, `Patching`, `ChampSelect`); `"%s.%s.bin"` from the state plus the **last segment** of each `ViewControllerList.ViewControllers` string, gated by `ViewControllerSet.SpecifiedGameModes` and a per-entry condition | `ClientState_loadUiBins` `0x14064F480`, `ClientState_collectViewControllerBinHashes` `0x140E2E530` | PTCH on the list (whole-list replacement); the VC class must exist in the exe; the resulting chunk name is `<state>.<leaf>.bin` at the WAD root. | `BinLoadPoints_StringPaths.md` section 2, lines 135-201 |
| H12 | `MapSkin` character skin override | `List2<Embed CharacterSkinOverride>` at `MapSkin+488`, property hash `0x2D3285EB`, **name uncracked**; each element `{Character: Hash -> Characters/<Name>, SkinID: U32}`; consumed on the asset-preload path only | `MapSkin_FindCharacterSkinOverride` `0x1406B7770` | Not reachable by a named PTCH path: `MetaPath_resolve` matches a property by `FNV1a32(lower(name))` and the name is unknown. Whether the path language accepts a raw hash token is not stated. Whole object (`MapSkin`) only. `mObjectSkinFallbacks` (`Map<Hash, I32>`, +384) is the older, named, disjoint mechanism. | `MapSkin_CharacterSkinOverride.md` sections 1-3, lines 12-178; `include/Environment/MapSkin.hpp:43-49, 84-91` |
| H13 | `SkinParent` | I32, champion-local skin id; base skins omit it (default, not serialised); "the data-side lever for globally overriding a skin"; no consumer traced | `[attested]` from data only | PTCH on H2's object. A property absent from the base carries no type tag in the base. | `SkinParent_ChromaSystem.md` sections 2-5, lines 27-91; `LTK_NoSkinsMode.md` section 3, lines 58-87 (gate: "no consumer of `SkinParent` was traced") |
| H14 | `PropertyOverrideLoadable` | `{FilepathHash: File = patch, OverrideSrcFolder: File = target}`, size 24; registered by the owning `ViewController` on scene enter, activated by one of 87 hardcoded conditions; `OverrideSrcFolder` omitted = wildcard target `0` (attested statically, never executed) | `UiPropertyOverrideLoadable_registerOverride` `0x1413A7280`, `BinFileCache_createEntry` | The only data-declared PTCH channel the client has; UI-scene scope; conditional. | `BinLoadPoints_DataOnly.md` section 3, lines 155-201, section 5 lines 269-293; `PropertyOverrideLoadable.md` section 8, lines 266-293 |
| H15 | `objectPath` | `FNV1a32` of the object's own entry path, bound by 12 classes (`VfxSystemDefinitionData` at `+336`, `SkinCharacterDataProperties` at `+0x5AC`, `SpellObject`, ...); one read proven (a parent definition's path as the scope of a by-name VFX create) | `Vfx_resolveAndCreateInstance` `0x1412BE270` | A copied entry under a new name keeps the original's `objectPath`; keeping it in sync "costs nothing". 310,544 shipped rows agree with the entry key, 938 differ by casing only. | `LoadFromDefinitionSentinel.md` section 5b, 8 (item 4), 9, lines 208-231, 300-304, 335-352 |
| H16 | `<load_from_definition>` | a compile-time literal compared by **pointer**, not `strcmp`; the world-particle spawn path when `MapSkin.WorldParticles` is set, versus a by-name path from `mWorldParticlesINI` | `Obj_GeneralParticleEmitter_OnCreate` `0x140463E70` | Not data. A VFX reference resolves pointer, then hash (H7), then name; the earlier slot wins. | `LoadFromDefinitionSentinel.md` sections 4-5b, 8, lines 106-206, 287-299 |
| H17 | missing-effect placeholders | `MissingInstant` / `MissingLingering`, FNV-1 of exe literals, indexing a `std::map<u32, std::string>` | `VfxCreation_initMissingPlaceholderKeys` `0x1400E2D40` | Not data. | `ResourceResolvers_VfxEffectKeys.md` section 4.1, lines 255-265 |
| H18 | `GameModeConstants` keys | 160 literal names hashed at static init into a table; the value comes from `GameModeConstants.mGroups{}.mConstants{}` in data; presence of a key does not imply a reader | `sub_1400EA670` | PTCH on the constants object (map values). A key with no reader (`ShowBeta`) does nothing. | `BinLoadPoints_StringPaths.md` section 3, lines 204-305 |

Two more facts bound the table:

- **Every PTCH record is per bin and per record.** A layer reaches exactly the one bin it is
  layered over; a record whose entry hash is absent is skipped by size; the applier compares the
  record's type tag against the **running build's** registered type and skips on mismatch with
  no log line; a `List` / `List2` whose element type was retyped is cleared and resized before
  the check runs (`PTCH_PropertyPatches.md` section 2, lines 72-113;
  `AssetNamespacing_Repathing.md` section 6, lines 286-308). Of 395 retypes measured in 16.17,
  384 would no-op and 11 would empty a list.
- **A class cannot be introduced.** `GameObject_createByClassHash` resolves against a registry
  filled by static initialisers only (`LoadFromDefinitionSentinel.md` section 8 item 1,
  lines 289-292; `LeagueModding.md` section 6.4, line 603-604).

#### <a id="s2.9.4"></a>2.9.4 The namespacing rules the notes recommend

`AssetNamespacing_Repathing.md` section 3 (lines 130-212) states seven `[design]` rules. In the
note's vocabulary: (1) one unused top-level root for every chunk a mod authors, mirroring nothing
of Riot's directory shape; (2) entry names namespaced the same way, `Mods/<modid>/...`, with a
collision reserved for deliberate overriding; (3) shipped content names no Riot identifier -
every Riot name lives in a generated **binding** of `PTCH` or override records, produced from
resolution **rules** against the installed build; (4) a reference ladder, best to worst:
identifiers the client formats itself (champion name plus skin index, map id, mode name), entry
names, class and property names, literal chunk paths and `File` hashes; (5) patch properties
rather than replace files; (6) verify at install and at launch - every chunk depended on exists,
every entry name bound resolves, every patched property exists on the expected class with the
expected type tag, list-valued properties first - and fail closed with a report; (7) neither
mount order nor the backslash key split is a namespacing mechanism. Section 4 (lines 216-230)
draws the resulting shape:

```text
mods/<modid>/<version>/...              content; references only its own namespace
  entries named  Mods/<modid>/...
--------------------------------------------------------------------------------
binding/<gamebuild>.ptch                generated; the ONLY place Riot names appear
  resolved from rules:  "champion=Ahri skin=5"  ->  entry Characters/Ahri/Skins/Skin5
                        property X            ->  mods/<modid>/<version>/...
--------------------------------------------------------------------------------
manifest                                expected resolutions + fingerprints, checked at launch
```

Its open question 1 (lines 310-315) is whether the `CharacterRecords` fallback (H4) lets a mod
attach namespaced records without patching Riot's containers; unverified there and here.

#### <a id="s2.9.5"></a>2.9.5 Tooling status, and one discrepancy

- `AssetNamespacing_Repathing.md` rule 5 (lines 188-190) and `LeagueModding.md` section 6.3
  (lines 585-587) state that no tooling writes a patch record and that a binding layer has to be
  a rewritten base bin. The namespacing note dates its findings to 2026-08-19 to 2026-08-26
  (its section 8, lines 317-323). `league-toolkit` at `origin/main`
  (`11bb8ba`) carries `BinOverride::to_writer` in `crates/ltk_meta/src/data_override/write.rs`
  (line 35; the doc comment at line 17: "The output always uses `PTCH` version 1 around `PROP`
  version 3 with no dependencies") alongside `read.rs` and `apply.rs`. The reversing notes are stale on this point;
  what is unverified here is whether that writer round-trips against the client, which
  `ptch-property-patches.md` [section 17](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s17)
  is the place to check.
- The overlay applies a declared override at build time and ships a rewritten `PROP` (PRD-001
  route 3, [section 2.4](#s2.4)). None of the client's own override channels (H14, the
  in-process `BinFileCache_addDataOverride_*` API, `GameStartInfo`) is engaged by this
  repository's build.

## <a id="s3"></a>3. Candidate declaration designs

The mod project is the subject of the declaration. A `.modpkg` and a `.fantome` are build
artifacts of a project: each carries the project's declarations it has room for and drops the
rest with a warning ([section 2.5](#s2.5), `pack/mod.rs:70-73`). A candidate is judged on the
project first and on the artifacts second.

Every candidate has to answer the same six questions: what an author writes in the project; what
each build artifact carries of it; how layer priority and cross-mod order combine two
declarations; what is validated at pack time and what at build time; whether targets and links are
paths or hashes; and how the schema is versioned. The constraints of [section 2](#s2) fix some
answers regardless of candidate:

- **A link is a path string on the wire** ([section 2.1](#s2.1)). The declared link is a path.
- **A target is a chunk**, addressed the way every chunk is: a chunk path, lowercased before
  hashing, or a 16-hex-digit hash ([section 2.5](#s2.5)).
- **Combining is set union in load order** ([section 2.4](#s2.4), merge rule; [section 2.6](#s2.6),
  string-override order). Two mods adding links to one bin conflict only if removal is
  expressible.
- **The build step is a synthesized chunk** over a base that is either the game's copy or the
  highest-priority mod override of that chunk ([section 2.6](#s2.6)).
- **The declaration reaches the manager through `ModProjectLayer`** ([section 2.7](#s2.7)). A
  build artifact carries whatever is needed to rebuild that struct; the struct is the contract,
  the artifact encoding is not.

### <a id="s3.1"></a>3.1 Candidate A: inline in the layer, beside `string_overrides`

A new per-layer field. Keyed by target chunk path; the value is the list of links to add.

```json
{
  "layers": [
    {
      "name": "base",
      "priority": 0,
      "bin_links": {
        "data/characters/teemo/skins/skin0.bin": [
          "data/characters/jade_teemo/skins/skin0.bin"
        ]
      }
    }
  ]
}
```

```toml
[[layers]]
name = "base"
priority = 0

[layers.bin_links]
"data/characters/teemo/skins/skin0.bin" = [
  "data/characters/jade_teemo/skins/skin0.bin",
]
```

| Question | Answer |
| --- | --- |
| Packing | A field on `ModpkgLayerMetadata`, schema v4, omitted when empty (the pattern of `string_overrides` and `hashtables`, `metadata.rs:112-123`, `197-205`). Fantome drops it with a warning, as it drops non-base layers (`pack.rs:181-214`). |
| Layer and mod order | Inherited: the same fold `collect_effective_overrides` runs (`strings.rs:240-269`), producing one `target -> ordered link set` per build. |
| Pack-time validation | Serde shape; every key and value is a non-empty chunk path or hex hash; a value is not a `PTCH` (unknowable at pack). No game needed. |
| Build-time validation | The target resolves to a game chunk or a routed override, else a reported miss; every added link feeds `OverrideMeta::linked_bins`; the existing pre-flight reports links to bins that nothing mounts (`linked_bins.rs:80-127`). |
| Addressing | Target: chunk path or hex hash. Link: path only. |
| Schema versioning | The config has no version field (`lib.rs:264-333`); the modpkg schema bumps to 4. |
| Tooling reuse | None required in `ltk_mod_project`; the build uses `ltk_meta` (`BinDelta`, [section 2.3](#s2.3)) or a header-only rewrite. |

Trade-offs. Ergonomic for the common case of a handful of targets; the manifest is already where
an author looks for build-affecting data; one code path in the manager. It grows the config for a
mod that touches many bins, and TOML's nested-table syntax for a map of lists is the least
readable part of it. It commits a name (`bin_links`) that must also leave room for 191's `PTCH`
declarations; a `game_data` block with `links` and `overrides` sub-keys is the shape that does:

```toml
[[layers]]
name = "base"
priority = 0

[layers.game_data."data/characters/teemo/skins/skin0.bin"]
links = ["data/characters/jade_teemo/skins/skin0.bin"]
overrides = ["teemo-skin0.rito"]   # issue 191, file relative to the layer directory
```

### <a id="s3.2"></a>3.2 Candidate B: a layer manifest with direct or external modules

A layer with declarations has one `content/<layer>/game_data.yaml`, `game_data.yml`,
`game_data.toml` or `game_data.json`. The manifest carries `version: 1` and an ordered
`modules` array. Each item has an explicit `target` and either its edit body directly beside
`target`, or a `source` naming an external module. There is no `inline` key and no required
`game_data/` directory. A layer without a manifest has no declarations; file placement and
file-name suffixes never discover modules or select targets.

```yaml
# content/base/game_data.yaml
version: 1

modules:
  - target:
      path: data/characters/teemo/skins/skin0.bin
    links:
      - mods/teemo-classic/particles.bin
    Characters/Teemo/Skins/Skin0/Resources:
      +resourceMap:
        Teemo_R_Mis: Mods/TeemoClassic/Particles/R_Missile

  - target:
      path: shared
    source: shared.patch.yaml
```

```yaml
# content/base/shared.patch.yaml
version: 1

"0xd2343ed1":
  +resourceMap:
    Teemo_Classic_Ambient: Mods/TeemoClassic/Particles/Ambient
```

The first item contains its edits. The second takes its body from an external file while
keeping its target in the manifest. `source` is a single YAML, TOML or JSON file (`.yml`
accepted), resolved by its actual file name relative to the layer directory. Source files
may live anywhere within that layer. File names are author-chosen; `.patch` is an
illustrative suffix, and no source directory or hierarchy is prescribed. There is no
implicit `.bin` suffix or name rewriting.
The external file carries its own supported `version` and body; it cannot contain `target`,
`modules` or another `source`. The manifest version governs direct bodies. Moving a source
file never changes its target; relative resource references may need updating.

Each item has exactly one body form: compact bindings, `steps`, or `source`. A source item
cannot also contain local bindings or steps, and compact bindings cannot accompany steps.
The external file accepts compact bindings or steps under its `version`. A body contains at
least one binding; a steps array contains at least one nonempty binding batch. Empty
`modules` is a valid layer with no declarations.

`source` expands at its item's position in the `modules` array. A source and an equivalent
direct body have the same execution semantics. Applying a source and then customising its
result uses two consecutive items with the same target, the source item first. Modules
apply in array order, including when their targets differ; file names and mapping-key order
have no execution meaning. A compact body is one batch. A `steps` body is an ordered array
of batches, each reading the result of the preceding batch. Within a batch, the phases are
override files in listed order, object creation, entry edits, object removal and dependency
list edits. Clones read the state at the start of the object-creation phase. Dependent edits
use separate steps; later modules or steps may deliberately set the same property.

Within an entry-edit phase, independent operations have no order. For the same container,
set precedes removal, then addition. Duplicate mapping keys are errors before a parser can
discard them. Duplicate semantic sets, ancestor/descendant writes, or indexed edits that
overlap a structural list change require separate steps. Canonical duplicates that require
schema information are build-validation failures.

An explicit target selector contains exactly one of `path` or `hash`. `path` is a literal
game chunk path, including extensionless paths such as `shared`. `hash` is a quoted string
of exactly 16 hexadecimal digits without `0x`, normalised to lowercase. Dependency `links`
remain literal paths and never interpret a hexadecimal-looking string as a raw hash. Entry
and property hash escapes retain their separate 32-bit grammar in [section 4.5](#s4.5).

Override-file paths resolve relative to the external module containing them, or relative to
the layer directory for a direct body. Both source and resource paths must remain within
the layer, including after resolving symlinks. The loader classifies the manifest, referenced
source files and referenced PTCH/rito inputs before ordinary content collection. These are
build resources, excluded from game content even inside a `.wad.client` directory. A
`.modignore` rule matching a required input is a packing error. Unreferenced YAML, TOML or
JSON follows ordinary content rules; it is not automatically a declaration.

The [game-data wiki](https://wiki.leaguetoolkit.dev/reference/mod-packages/game-data/) is the
planned declaration contract; this note records the design grounds.

| Question | Answer |
| --- | --- |
| Packing | Read and validate the layer manifest, resolve sources and override inputs, and compile ordered modules into layer metadata. Authored files and executable package metadata are separate representations; execution does not reopen source files. |
| Layer and mod order | Same priority rules as A; modules and steps run in array order within each layer, against the result of preceding edits. |
| Pack-time validation | Multiple manifests, missing sources, unsupported versions, mixed body forms, recursive sources, malformed selectors, duplicate canonical target/source assignments, conflicting file roles and ignored required inputs are errors. The same source may target different bins; each assignment has separate identity and diagnostics. |
| Build-time validation | Same as A, plus semantic conflicts requiring the installed schema. Diagnostics retain the manifest reference and the external source location where applicable. |
| Addressing | One explicit `target.path` or `target.hash` per item; a source file's path never selects a target. |
| Schema versioning | A version in the manifest and each external module; direct bodies inherit the manifest version. |
| Tooling reuse | The filesystem provider and packer share manifest loading, input classification and compilation. Filesystem and package providers expose the same ordered program to the overlay builder. |

Trade-offs. Small edits fit in one layer manifest, alongside the layer's content. Larger
edits use named source files without mirroring game paths on disk. `mod.config.json` keeps
identity and layer order. An external module requires an explicit reference and target;
moving or renaming that file cannot silently retarget an edit. The manifest is the single
discovery point, and the program preserves ordering independently of file layout.

### <a id="s3.3"></a>3.3 Candidate C: a sidecar next to the target's position in the layer

The declaration sits at the target's own chunk path with a marker extension, inside the WAD
directory the target belongs to:

```text
content
|-- base
|   |-- Teemo.wad.client
|   |   |-- data
|   |   |   |-- characters
|   |   |   |   |-- teemo
|   |   |   |   |   |-- skins
|   |   |   |   |   |   |-- skin0.bin.links
```

with `skin0.bin.links` a plain list, one path per line.

| Question | Answer |
| --- | --- |
| Packing | Every `.links` file must be recognised and excluded from chunks by the packer, in every format. A file with that suffix is a legal chunk path; the convention shadows a real (if unlikely) name. |
| Layer and mod order | The target is identified by location, which is how chunks are identified. Union across layers and mods as in A. |
| Pack-time validation | Syntactic. |
| Build-time validation | As A, plus: the target's WAD is declared by the directory; a target the game holds in a different WAD needs the cross-WAD routing rules to reach it (`builder/mod.rs:139-177`). |
| Addressing | Target by location (path only, no hex form unless the sidecar is `<hex>.links`). |
| Schema versioning | None; a line format. |
| Tooling reuse | None. |

Trade-offs. The most "put it where the bin is" shape, and the only one where a target's WAD is
explicit. It leaks into every format and provider (a new file class the modpkg TOC has no
column for; `ModpkgChunk` has `layer_index`, `wad_index`, `path_index` and nothing else,
`chunk.rs:11-25`), is invisible to `mod_project()`, and cannot express a target the mod does not
place in a WAD directory. It is the shape Fantome's `RAW/` folder took for a different problem
and that Fantome's own wiki warns against for new files.

### <a id="s3.4"></a>3.4 Candidate D: a binary artifact the build applies

For links: the author ships the bin with the links already added, as any override. For 191: a
`.ptch` file plus a target list. This is the status quo for links and the literal reading of 191.

| Question | Answer |
| --- | --- |
| Packing | No change; the file is a chunk. A `PTCH` needs a target list from somewhere - which is candidate A or B again. |
| Layer and mod order | For a shipped bin: first mod wins, whole chunk (`overlay-builder-design.md:213-216`); under ADR-0012's merge, links union ([section 2.4](#s2.4)). For a `PTCH`: `join` reports collisions; order is the build's (`ptch-property-patches.md` [section 13](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s13)). |
| Pack-time validation | `BinKind::identify_from_bytes` says which kind a file is ([section 5.4](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s5.4)). |
| Build-time validation | `BinOverride::check` against the base ([section 9.5](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s9.5)). |
| Addressing | The base is whatever chunk the file sits at. |
| Schema versioning | The wire format's. |
| Tooling reuse | All of `ltk_meta`'s patch surface. |

Trade-offs. For links it is the defect ADR-0012 measured: shipping a copy of a game bin replaces
what the mod did not carry forward and goes stale on every game patch. For 191 it is the payload,
not the declaration, and needs one of the other candidates to name targets.

### <a id="s3.5"></a>3.5 Candidate E: a ritobin-text delta

A `.py`-style text file per target, using ritobin's `linked:` root entry and, for 191, the
`patches:` entry of [section 15](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s15):

```text
#PROP_text
type: string = "PROP"
version: u32 = 3
linked: list[string] = { "data/characters/jade_teemo/skins/skin0.bin" }
entries: map[hash,embed] = {}
```

| Question | Answer |
| --- | --- |
| Packing | A text chunk or meta chunk; the packer needs `ltk_ritobin` to validate. |
| Layer and mod order | Union of `linked`; `patches` as PTCH. |
| Pack-time validation | Full typecheck via `ltk_ritobin`. |
| Build-time validation | As D. |
| Addressing | Target named outside the file (again A or B). |
| Schema versioning | ritobin's. |
| Tooling reuse | `ltk_ritobin` parse and print; the LSP. |

Trade-offs. A ritobin file describes a whole bin, not a delta; `entries` and `version` are
mandatory roots, and a "delta" reading of it is a new dialect with no consumer. It pulls
`ltk_ritobin` into `ltk_mod_project` for a list of strings. Right for authoring a `PTCH` by hand
(that is what [section 15](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s15)
exists for); wrong as the declaration of targets.

## <a id="s4"></a>4. Namespacing hardcoded data points declaratively

The question on top of `links` and `overrides`: how a project declares (a) that its own content
lives in a namespace no Riot name reaches, and (b) that a hardcoded data point of
[section 2.9.3](#s2.9.3) resolves into that namespace - without shipping a copy of the Riot bin
the data point lives in. Vocabulary for this section, all of it provisional:

- A **namespace** is a root the mod owns: a chunk-path prefix (`mods/<modid>/`) and an
  entry-name prefix (`Mods/<modid>/`), both from the zero-shipped set of
  [section 2.9.1](#s2.9.1).
- A **binding** is one edit that makes a Riot-side data point point into the namespace. A
  binding is expressed as a rule against the installed base and is materialised by the build.
  This is the `[design]` rule 3 of [section 2.9.4](#s2.9.4) restated in this repository's terms.
- A **declared delta** is the family of bindings the project spells inline; a `PTCH` file in
  `overrides` is the same delta authored by hand. Both are overrides applied to a target bin; the
  noun for the artifact stays "override", never "layer".

### <a id="s4.1"></a>4.1 What is reachable, and by which declaration

Column "declaration" names the `game_data` sub-key of [section 4.3](#s4.3) that carries it.

| data point | goal | reachable by | declaration | not reachable |
| --- | --- | --- | --- | --- |
| H1 skin bin file | put a mod skin at `Skin<N>` | an override chunk at the templated path, or H13 | a layer file; or `properties` (`SkinParent`) | redirecting the template |
| H2 skin properties | swap meshes, textures, VFX for one skin | PTCH on the `SkinCharacterDataProperties` entry | `properties`, `resolvers` | an entry the base bin lacks (scoped lookup) |
| H3 alias | render one character as another | PTCH String on the chosen `CharacterRecord` | `properties` on H4's record | - |
| H4 record chain | a per-map or per-mode record | add a whole `CharacterRecord` entry to the character's bin, cloned from `Root` | `objects` (clone + set) | a record in a foreign bin; a token nothing sets (`modeTok`) |
| H5 override token | a mode-scoped token | PTCH whole-list replacement on `GameMutatorExpansions.mMutators` | `properties` (`append` on a list) | server-evaluated effects |
| H6 skin resolver entry | redirect one effect slot for one skin | PTCH on `resourceMap` of `Skin<N>/Resources` | `resolvers` | a `Resources` entry the base lacks (no Skin0 fallback for it) |
| H7 resolver key | suppress or redirect an effect key | a null link or a repoint in the owning map | `resolvers` | inventing a key nothing asks for |
| H8 owners | give an item, perk, map skin, spell a mod resolver | PTCH Link / Hash repoint, or whole-list replacement | `properties` | growing a `List<Link>` in place |
| H9 global resolver | a process-wide last-resort entry | PTCH on chunk `shared` | `resolvers` keyed by the extensionless chunk path | knowing the client reads it (open) |
| H10 mode bins | load a mod bin on one `(map, mode)` | PTCH whole-list replacement on `AdditionalPropertyDataPaths` | `properties` (`append`) | - |
| H11 UI bins | add a view controller | PTCH whole-list replacement on `ViewControllers`; the chunk name is derived | `properties` (`append`) plus a chunk named `<state>.<leaf>.bin` | a VC class the exe lacks |
| H12 map-skin skin table | force a unit skin on a map skin | whole `MapSkin` object only | none until the property name is cracked or a hash token is admitted | named PTCH path |
| H13 `SkinParent` | make skin X inherit skin Y | PTCH I32 | `properties` | verifying the consumer (untraced) |
| H14 `PropertyOverrideLoadable` | a conditional override | a `UiPropertyOverrideLoadable` object plus a `PTCH` chunk | `objects` + a layer file | a new condition |
| H15 `objectPath` | keep a cloned entry consistent | set by the build on every cloned or renamed object | implicit in `objects` | - |
| H16, H17 | - | - | - | not data |
| H18 constants | flip a mode constant | PTCH map value | `properties` | a key with no reader |

Three things fall out. First, every reachable row is one of four record shapes: **set** a scalar
or link, **replace** a whole container, **add** a whole object, or **merge** keys into a map.
Second, "replace a whole container" and "merge keys into a map" both need the base's current
value, which only the build has (`read_game_chunk`, [section 2.6](#s2.6)); the declaration can
carry the delta and nothing else. Third, the string every row hashes is one of the three in
[section 2.9.2](#s2.9.2), and the declaration writes the string, never the hash - with a
hex escape for the uncracked (H12's `0x2D3285EB`, resolver keys such as `0x2777b32e`).

### <a id="s4.2"></a>4.2 The project-level namespace

The standard has no `namespace` key in version 1 ([section 5](#s5)). This section records
the lint that was considered and the facts it rests on.

A single declaration beside `layers`, not under any of them: the namespace is a property of the
mod, and every layer's content sits under it.

```toml
name = "teemo-classic"
version = "1.2.0"

[game_data]
namespace = "mods/teemo-classic"        # chunk-path root; entry-name root is the same string in
                                      # the mod's own casing, "Mods/TeemoClassic"
```

What it buys, each a rule the pack step enforces or reports:

- **A chunk path under `content/<layer>/<Wad>.wad.client/` that is neither under the namespace
  nor a declared target is reported.** A path outside the namespace is a Riot path by
  construction ([section 2.9.1](#s2.9.1): the 20 real roots), and a Riot path in a layer is a
  deliberate override that the `game_data` block names, or a mistake. A raw hash file name
  (16 hex digits, [section 2.5](#s2.5)) is exempt: it is by definition a Riot chunk.
- **A backslash in a chunk path is an error** (`AssetNamespacing_Repathing.md` line 270-271).
- **Every entry name in a mod-shipped `PROP` under the namespace is expected to start with the
  entry-name root.** This check needs a bin parse and is a build-time report, not a pack-time
  error ([section 4.5](#s4.5)).
- A `links` value under the namespace is the mod's own bin; a `links` value outside it is a
  Riot bin, which the existing pre-flight already verifies exists.

The namespace is a lint and a contract, not a mechanism: nothing in the client reads it
([section 2.9.4](#s2.9.4) rule 7). Whether it is enforced or advisory is
[section 5.1](#s5.1) question 15.

### <a id="s4.3"></a>4.3 Bindings in a module body

A module body in [section 3.2](#s3.2) holds the bindings materialised into its explicit
target by the build. The examples below are external source files referenced from a layer
manifest. Their names describe their content; only the manifest's `target` selects the bin.
The same bindings can appear directly beside `target` in a manifest item.

| sub-key | payload | record shape | precedent |
| --- | --- | --- | --- |
| `links` | list of chunk paths | header rewrite | issue 190 |
| `overrides` | list of `PTCH` files in the layer | records as authored | issue 191 |
| entries (the entry name at target level) | entry -> signed property path -> value | set / add / remove | ritobin `patches:` |
| `resolvers` | entry -> key -> entry name or null | merge into a `resourceMap` | `string_overrides` |
| `objects` | entry -> clone source + sets | add a whole object | none |

entries, `resolvers` and `objects` are three spellings of a declared delta, and each one
is representable as a `PTCH` the build could also have read from `overrides`
(`LeagueModding.md` section 4.3). They exist as separate keys for one reason: their values are
computed against the installed base and cannot be written down as a `PTCH` in the project.

#### Entries: set, add, remove

```yaml
# content/base/teemo-skin0.patch.yaml
version: 1
Characters/Teemo/Skins/Skin0:
  # H13: base Teemo renders as skin 0 (a no-op here; a chroma would name its parent)
  SkinParent: !i32 0 # a type pin; a bare 0 coerces against the schema
  # H8: hang a mod resolver off the skin, in front of Riot's; the string coerces to the
  # list's link element type
  +mAdditionalResourceResolvers: [Characters/Teemo/Skins/Skin0/ClassicResources]
  # Jade_Teemo ships iconAvatar and base Teemo does not: the schema types it
  iconAvatar: assets/characters/jade_teemo/hud/jade_teemo_circle_0.tex
```

```yaml
# content/base/map11-classic.patch.yaml
# Manifest target: the chunk holding Maps/Shipping/Map11/Modes/CLASSIC.
version: 1
# H10: load a mod bin on Summoner's Rift Classic and nowhere else
Maps/Shipping/Map11/Modes/CLASSIC:
  +AdditionalPropertyDataPaths: [data/characters/teemo/classic]
```

Every key under an entry is a full path with an optional leading sign, so an edit reaches a
nested field, a list element or a map entry without replacing its container. `+` adds to the
list or map the path ends on, `-` removes from it, an unsigned key sets; there is no operation
table. An entry sits in the module body, keyed by its name; there is no `properties`
keyword. The two vocabularies stay apart lexically: the standard's words (`target`, `path`,
`hash`, `source`, `steps`, `links`, `overrides`, `objects`, `clone`, `class`, `set`, `remove`,
the signs, the type names)
are bare words, an entry name carries a slash or is in hash form (every one of the 359,477
names in CDragon's `hashes.binentries.txt` carries a slash), and the game's field names sit
only inside an entry, a `set` or a map value. Riot has fields named `Add`,
`Path`, `Links`, `Scope` and `Entries` (`hashes.binfields.txt`), and names hash
case-insensitively, so a word-keyed operation table inside a field mapping would collide. Values are from base Teemo and League Classic
(Jade) Teemo, `data/characters/{teemo,jade_teemo}/skins/skin0.bin` on CommunityDragon:

```yaml
# content/base/teemo-skin0.patch.yaml
version: 1
Characters/Teemo/Skins/Skin0:
  skinMeshProperties.materialOverride[0].texture: assets/characters/jade_teemo/skins/base/jade_teemo_base_mushroom_tx_cm.tex
  skinMeshProperties.overrideBoundingBox: [50.0, 180.0, 150.0]
  healthBarData.unitHealthBarStyle: 12
  +skinAudioProperties.bankUnits[0].events: [Play_sfx_Teemo_R_Pickup]
Characters/Teemo/Skins/Skin0/Resources:
  'resourceMap{"Teemo_R_Mis"}': Characters/Jade_Teemo/Skins/Skin0/Particles/Jade_Teemo_Base_R_Mis # a string key hashed to the map's key kind
```

- A value is a plain value, coerced at build time to the type the schema gives the property:
  an integer to any integer width with a range check, a string to `string`, `hash`, `link` or
  `file` by hashing it the way that type hashes, a list to a list or a fixed-size vector, a
  mapping to a map, `null` to a null hash, link, file, pointer or option. A mapping on an
  `embed` or `pointer` property descends into the struct and merges field by field, so an
  edit can be blocked out (`skinMeshProperties: {texture: ..., selfIllumination: 1.0}`) or
  dotted (`skinMeshProperties.texture: ...`), and the two mix; an index stays a path segment.
  A coercion that loses information is a report and the edit is skipped. A retype
  by Riot between patches (`string` to `file`) keeps working, which is what a build-time
  coercion buys over an authoring-time pin. A property the base does not serialise (H13: base
  skins omit `SkinParent`) and the schema does not know has no type to coerce against, and the
  value is spelled with a type pin: a YAML local tag, `!i32 0`, `!link ...`, `!file ...`,
  `!hash ...`, `!string ...`, `!vec3 [...]`, `!rgba [...]`, `!option null`, or in TOML and
  JSON a one-key mapping keyed by the same ritobin type name, `{ i32 = 0 }`. A `pointer` or
  `embed` pin carries `class` and `set`, and `set` is the only place the struct's fields
  appear: `!pointer {class: VfxEmitterDefinitionData, set: {emitterName: Classic_Trail}}`.
  On a signed key the pin names the element type: `+tagEventList: !hash [Jade_Teemo]`. A pin
  that disagrees with the schema is a report and a skip, never a coercion. The one-key
  mapping is read by the property's type: a pin on a scalar or container, a descent on a
  struct unless its only key is `pointer` or `embed`. Riot has fields named `Flag`, `Hash`,
  `Map`, `Option` and `String` and none named `Pointer` or `Embed`
  (`hashes.binfields.txt`); a YAML tag is never subject to that reading. ADR-0006 keeps
  schemas out of `ltk_meta` ([section 2.4](#s2.4)); the build types from the manager's dump,
  then the base bin, then the pin. The coercion table and the pin rules are on the wiki page.
- `+` on a `List` / `List2` is materialised as a whole-container replacement record over
  the base's current elements, less `-` removals, plus the additions; the client cannot grow a list through `[i]`
  (`LeagueModding.md` section 4.3, lines 415-417). A `set` on `[i]` is bounds-checked at load
  against the live size.
- A property key is Riot's own property path (`.`, `[i]` in decimal, hex or octal, `{k}` with
  a JSON scalar key; `PTCH_PropertyPatches.md` section 1, lines 38-70;
  `ptch-property-patches.md` section 8.1 is the normative grammar and
  `ltk_meta::path::PropertyPath` implements it). A segment name hashes FNV-1a32 lowercased;
  a `0x` name is text to the client, so the standard's hash-form escape is resolved by the
  build and never reaches an override record. `{k}` has zero shipped examples and is
  `[inferred]`; the build can avoid emitting it by always replacing the whole map
  ([section 5.1](#s5.1) question 3).
- A `link` value is an entry name; the build hashes it FNV-1a32 lowercased. A `file` value is a
  chunk path; the build hashes it XXH64 lowercased. A value that must name something uncracked
  is spelled `!link "0x2088be81"` and passes through.

#### `resolvers`: key-level merge into a `resourceMap`

The standard expresses this as signed keys under the entry
(`+resourceMap: {...}` and `-resourceMap: [...]`, [section 5](#s5)). The facts below hold for
that form unchanged.

The shape of `string_overrides` - a map of key overrides applied over the game's own map - fits
a resolver exactly: the key is the skin-independent effect name, the value is the entry the
skin answers with, and the build has the base map ([section 2.6](#s2.6), `StringPatchPlan`).

```yaml
# content/base/teemo-skin0.patch.yaml
version: 1
# H6, H7: point the ult at League Classic's missile, add its pickup effect, mute two effects
Characters/Teemo/Skins/Skin0/Resources:
  -resourceMap: [Teemo_R_Firefly_GroundLight]
  +resourceMap:
    Teemo_R_Mis: Characters/Jade_Teemo/Skins/Skin0/Particles/Jade_Teemo_Base_R_Mis
    Teemo_R_Pickup: Characters/Jade_Teemo/Skins/Skin0/Particles/Jade_Teemo_Base_R_Pickup
    "0x6ecc5fac": Characters/Jade_Teemo/Skins/Skin0/Particles/Jade_Teemo_Base_E_Poison # an uncracked key CDragon prints as {6ecc5fac}
    Teemo_R_Debuff: null # a present key with a null link is a hit that suppresses;
                         # an absent key falls through
```

```yaml
# content/base/shared.patch.yaml
# Manifest target.path: shared
version: 1
# H9: one entry in the process-wide last-resort tier, chunk path extensionless
"0xd2343ed1":
  +resourceMap:
    Teemo_Classic_Ambient: Characters/Teemo/Skins/Skin0/Particles/Teemo_Classic_Ambient
```

- Keys and values are strings; the build lowercases and hashes both (FNV-1a32, never FNV-1:
  `ResourceResolvers_VfxEffectKeys.md` section 9.4, lines 812-814). A key or value already in
  hash form (`0x...`) passes through.
- The build reads the base entry's `resourceMap`, applies the overrides (a declared key
  replaces or adds; `null` writes a null link, `""` in TOML), sorts by key and writes one whole-map replacement
  record. Duplicate keys collapse in the client after `finalize`, first-merged wins
  (`ResourceResolvers_VfxEffectKeys.md` lines 100-106); the build de-duplicates on the
  lowercased key and reports a collision across mods.
- A redirect target that is not a `VfxSystemDefinitionData` is the hazard of
  `ResourceResolvers_VfxEffectKeys.md` section 7.6 (lines 663-667). The build can classify the
  target's class when the target is in a mod bin or the game index; a target in neither is the
  same "dangling" outcome the census counts (117 shipped).
- The mod's own particles live in a mod bin under the namespace, reached through `links` on the
  same target, and their entries carry `objectPath` equal to their own name (H15).

#### `objects`: add a whole object, cloned from the current target

```yaml
# content/base/braum.patch.yaml
# Manifest target.path: data/characters/braum/braum.bin
version: 1
# H4: a per-map CharacterRecord for Braum on Summoner's Rift, no mutator needed
objects:
  Characters/Braum/CharacterRecords/Map11:
    clone: Characters/Braum/CharacterRecords/Root
    set:
      mFallbackCharacterName: Mods_TeemoClassic_Braum # H3: skin lookups retry under this name
```

```yaml
# content/base/ui.patch.yaml
# Manifest target: the UI chunk holding the edited entries.
version: 1
# H14: a conditional override riding an existing trigger
objects:
  Mods/TeemoClassic/UI/FlipOverride:
    class: UiPropertyOverrideLoadable
    set:
      OverrideSrcFolder: clientstates/gameplay/ux/lol/minimap/uibase # coerces to file
      FilepathHash: mods/teemo-classic/ui/minimap-flip.bin # coerces to file
```

- `clone` names an entry at the start of the batch's object-creation phase; the build copies
  it, applies `set`, rewrites
  `objectPath` to the new name where the class binds it (H15), and emits it as a layer object.
  `class` without `clone` constructs a default object of that class; the build cannot verify the
  class exists in the exe (no schema, and a class cannot be introduced); an unknown class is
  a build-time report from the game index's own bins at best.
- The new object's name is under the mod's entry namespace unless the data point demands a
  Riot-formatted name (H4's `Characters/<char>/CharacterRecords/<token>`), which is the ladder's
  rung 1: an identifier the client formats itself.

### <a id="s4.4"></a>4.4 How it maps onto the overlay builder

The build step of [section 5](#s5) generalises without a new pass:

1. **Pass 1** collects the ordered program of every active layer of every enabled mod.
   Source expansion belongs to loading or packing; the plan retains module and step
   boundaries, target selectors and diagnostic origins. It does not flatten edits into maps
   that discard repeated writes. One synthetic `OverrideMeta` represents each target,
   `OverrideSource::BinEdits { chunk_path }` (working name), with `fallback_wad` from the
   game index. The target fingerprint includes the ordered operations and referenced override
   bytes, alongside the base, schema and build inputs required by the cache. Declared links
   feed pre-flight; the final materialised dependencies are checked against mounted content.
2. **Pass 2** resolves the base (highest-priority mod override of the chunk, else
   `read_game_chunk`), refuses a `PTCH` base (D3) and a version-1 base (the client rejects it,
   [section 2.1](#s2.1)), then materialises with `ltk_meta`. Modules and steps execute in the
   order defined in [section 3.2](#s3.2), against the evolving target state. Each batch applies
   its authored overrides before its declared edits; `check` and `apply` observe that batch's
   input state. A global `join` across batches cannot erase those dependencies. The output is
   one `PROP` per target. Untouched objects copy byte for byte under the streaming rewrite
   ([section 2.3](#s2.3)).
3. **Reports.** `check` gives each record the outcome the client never reports
   ([section 2.9.1](#s2.9.1), silent failure table): a property missing from the expected
   class, a type tag that differs, an entry absent from the target, a `links` path nothing
   mounts, a resolver value in no bin, a namespace violation. The namespacing note's rule 6 ("fail
   closed with a report naming what moved") and the manager's advisory posture
   ([section 2.7](#s2.7)) meet in the same place the linked-bin offenders do: `overlay.json`.

This is the build of [section 5](#s5) with `ltk_meta` on the pass-2 path, which settles
[section 5.1](#s5.1) question 1 in favour of the crate for anything beyond `links`: a header
rewrite cannot merge a map.

### <a id="s4.5"></a>4.5 Constraints carried over

- **Hash versus path.** `target.path` and dependency `links` are literal game paths.
  `target.hash` is an explicit 16-hex string without a prefix. Entry names and property-path
  segments use the separate `0x` plus 8-hex escape. Typed `file` values use their coercion
  grammar. The build hashes each literal with the function its position demands
  ([section 2.9.2](#s2.9.2)); these forms are not interchangeable.
- **Case.** All three client hashes lowercase before hashing; a declaration keeps the author's
  casing for readability and the build compares on the lowercased form (chunk paths already do,
  [section 2.5](#s2.5)). Shipped dependency strings are PascalCase, none lowercase
  (`AssetNamespacing_Repathing.md` lines 85-89); a declared `links` value is written in the
  casing the author chooses and hashes the same either way.
- **Extensionless chunk paths are legal targets.** `shared`, `characters/petbunny`, and 3,447 of
  5,404 distinct shipped dependency strings have no extension
  (`AssetNamespacing_Repathing.md` lines 54-59, 85-89). `target.path` is a chunk path;
  tooling that appends `.bin` changes the lookup.
- **Ordering.** Front of the enabled list wins, with lower-precedence mods evaluated first;
  layers use ascending priority, then modules and steps use array order ([section 5](#s5)).
  Mapping-key order has no execution meaning. Within a list addition, values retain their
  written order after the existing elements.
- **`PTCH` cannot target `PTCH`, and a `PTCH` declares zero links** ([section 2.1](#s2.1)). A
  declared delta is materialised into the target's `PROP`, never shipped as a `PTCH` chunk, so
  the client never sees a `PTCH` from this build; a mod's `overrides` file is an input only.
- **Version-1 bases.** The client accepts a base at version 2 or 3 only
  (`BinLoadPoints_DataOnly.md` line 35). A version-1 chunk is not a bin the client loads through
  this parser; a declaration targeting one is a report.
- **Scoped lookups.** H2, H4 and H6 search one container. A declared delta on a target adds
  entries to that target, never to a sibling; an `objects` entry meant for the skin lookup lives
  in the skin's own bin.
- **A `List<Link>` grows only by whole replacement**, and a stale replacement on a retyped list
  empties it ([section 2.9.3](#s2.9.3), second bullet after the table). The build's `check` runs
  against the installed base; a retype is caught at build time, not at load time - the one
  advantage a build-time materialisation has over a shipped `PTCH`.
- **Removal is materialised by the build.** Signed keys remove dependency paths, map keys
  or list elements; object removal omits the object from the resulting `PROP`. A null link
  value is not a removal.

## <a id="s5"></a>5. Recommendation

The planned standard is the wiki page `reference/mod-packages/game-data.mdx`. This section
records the decisions and their grounds in [section 2](#s2) to [section 4](#s4).
A rule lives on the wiki page; a row here names the choice and the ground.

**Candidate B, one layer manifest with direct or external module bodies**, explicit targets
and ordered applications, the build step modelled on `StringPatchPlan`, and `ltk_meta` on
pass 2 from the first slice.

| Decision | Choice | Ground |
| --- | --- | --- |
| Declaration surface | One `content/<layer>/game_data.{yaml,yml,toml,json}` manifest with an ordered `modules` array; each item has an explicit `target` and direct bindings, `steps`, or `source`; no `inline` key or required directory | [section 3.2](#s3.2); small edits share one file, larger edits have external sources, and source file moves never retarget edits |
| Formats | YAML, TOML and JSON all accepted in version 1, one schema; YAML preferred, read as 1.2 core schema with hashes quoted; TOML read as 1.1 (multi-line inline tables, trailing commas) so a struct pin is one field per line; the Workshop writes YAML | nesting is the grammar in YAML and the TOML header ladder is the awkward translation; `0x6ecc5fac` is a YAML integer |
| Bindings | `links`, `overrides`, `objects`, and entry names in a module body; direct bodies place these beside `target`; `resolvers` folded into the entry as signed map keys | [section 4.3](#s4.3); one map-merge rule; entry names carry a slash or use hash form, separate from structural metadata |
| Operations | signed keys: `+key` adds, `-key` removes, an unsigned key sets, a bare `links` list adds; no operation table; the value is always the literal | the standard's words and the game's never share a mapping: Riot has fields named `Add`, `Path`, `Links`, `Scope`, `Entries` and names hash case-insensitively; a sign can never start a Riot name |
| List removal | by value for scalar-like elements, by index for `embed` and `pointer` | embeds have no natural value |
| Object removal | `remove = true`, materialised by omission; the client's `PTCH` carries a deletion record | `BinOverride.deleted` in `ltk_meta` |
| Override files | `.ptch` or `.rito` of type `PTCH`, relative to the external source file or the layer directory for a direct body; converted to `.ptch` at pack and stored as build resources | source resolution preserves the body location; override inputs never become game chunks |
| Role split | `source` selects an authored module body; `overrides` applies PTCH records verbatim; entry edits coerce against schema and base | file organisation and game-data operations are distinct parts of the interface |
| Type source | the meta class dump for the installed patch, base bin as fallback with a report | the manager holds a dump per patch; ADR-0006 keeps schemas out of `ltk_meta`, not out of the build |
| Type pins | optional; a YAML local tag `!f32 1.0`, or in TOML and JSON a one-key mapping `{ f32 = 1.0 }`; on a signed key the pin names the element type; `pointer` and `embed` pins carry `class` and `set`; the one-key mapping is read by the property's type (pin on a scalar or container, descent on a struct unless the key is `pointer` or `embed`); a pin that disagrees with the schema is a report and a skip | a pin is the author's "tell me when this drifts"; a tag can never collide with a key, and Riot has fields named `Flag`, `Hash`, `Map`, `Option`, `String` but none named `Pointer` or `Embed`; `set` keeps the game's field names out of the pin's mapping |
| Block nesting | a mapping on an `embed` or `pointer` property descends and merges; dotted and block forms are one edit and mix; an index stays a path segment; a mapping on a `map` is a set | a cluster of edits on one struct reads as a block; `bankUnits: {0: ...}` would read as a set of a map |
| `objects` | `clone` or `class`, exclusive; `class` typechecked against the dump | schema is available at build |
| Entry names and property paths | Explicit `target.path` or a quoted 16-hex `target.hash` without a prefix; entry names are absolute and use the separate 32-bit `0x` escape, as do property-path segments; dependency links are literal paths | target identity does not depend on file names; raw target hashes and literal dependency paths have separate grammars |
| Namespace | no `namespace` key in version 1; the game index decides Riot versus mod paths; an undeclared Riot path in a layer is a pack warning | [section 4.2](#s4.2) is a lint the client never reads |
| Precedence | Mod order, ascending layer priority, module array order, step array order; compact bodies are one batch with fixed phases; later batches may set the same property; duplicate or conflicting writes inside one batch are errors | source expansion and direct bodies have identical execution order; mapping order is not portable across formats |
| Shipped plus declared | a mod may ship a target in one layer and declare on it in another; the copy is the base | [section 2.6](#s2.6) base selection |
| Build posture | report and skip, never fail; reports in `overlay.json`, shown per mod by the manager | ADR-0001 |
| Unknown keys | pack error; the manager refuses that layer's declarations, warns, suggests updating | never a half-applied mod |
| Artifacts | Store ordered modules and steps with compiled override-resource references; `origin` carries diagnostic provenance; source text preservation is optional, execution boundaries are required; fantome uses `Layers.<name>.GameData` and resources under `META/game_data/` | packing resolves source files once; filesystem and archive providers expose the same executable program |
| Editor | Add direct module items to the layer manifest by default; edit referenced source files in place with comments preserved; extraction can reconstruct direct bodies without original source text | the manifest owns discovery and targeting; source preserves file organisation |
| Roadmap | `links` and `overrides` as one slice, then `properties`, then `objects` | shared loader, plan entry and pass-2 rewrite |

What the reversing notes settled ([section 2.9](#s2.9), [section 4](#s4)):

- **`links` semantics on the client side.** Lowercase then XXH64 seed 0, no prefix or suffix, a
  miss dropped silently, declaration order in the parse loop. De-duplication is the build's own
  rule.
- **The build reports what the client never does.** Every failure on the load path is silent
  ([section 2.9.1](#s2.9.1)); `check` against the installed base at build time is the only
  place a stale binding is caught.
- **`ltk_meta` is on the pass-2 path.** The `join` / `check` / `apply` surface of
  `ptch-property-patches.md` is the materialiser; `BinOverride::to_writer` exists at
  `origin/main` ([section 2.9.5](#s2.9.5)).

### <a id="s5.1"></a>5.1 Open questions for the maintainer

Questions the standard leaves open. Everything else that was open in earlier drafts of this
note is a row in the table above.

1. **Relation to ADR-0012's merge.** Under merge, a shipped bin's links union with the game's.
   Whether declared links are a second mechanism for the same effect, or the only mechanism a
   mod that ships no bin has, decides whether the declaration is sugar or a feature.
2. **Version-1 bases.** A v1 header has no link list and the client rejects a v1 base. The
   standard reports and skips; whether the build should instead rewrite the base as v3
   (`ltk_meta`'s rule for every write, D15) is open.
3. **Map subscript versus whole-map replacement.** `{k}` in a property path has zero shipped
   examples and is `[inferred]`. The standard materialises a map edit as a whole-map
   replacement; whether `{k}` records are ever emitted is open.
4. **Wildcard targets.** `ptch-property-patches.md` [section 9.5](https://github.com/LeagueToolkit/league-toolkit/blob/main/docs/design/ptch-property-patches.md#s9.5)
   asks for a distinct outcome if targeting grows a wildcard. `target.path` is exact;
   whether it ever admits a pattern is open.
5. **`objectPath` on cloned objects.** The build rewrites `objectPath` on an `objects` clone
   (H15). ADR-0002 places normalisation at import; whether a build-time rewrite of a property
   is normalisation or materialisation is a vocabulary question for the spec.
6. **H9 and H12 are unconfirmed as consumers.** Whether the client loads the shipped
   `GlobalResourceResolver` at all, and what reads `MapSkin`'s character skin override list
   outside preload, are reversing questions that decide whether two rows of
   [section 2.9.3](#s2.9.3) are reachable in practice.
7. **Conflict report granularity.** Per target or per key.
8. **Editor rewriting.** A comment-preserving YAML editor for the Workshop is a requirement
   with no crate chosen.
9. **`ltk_ritobin` `PTCH` support.** The crate rejects `type: string = "PTCH"`
   (`crates/ltk_ritobin/src/typecheck/collect.rs:167-172`); the `.rito` override form needs it.
10. **Where the spec lives.** `docs/design/` has no spec for the mod project format. The wiki
    page is the standard for the declaration modules; the crate-level spec (`write-spec`),
    with the requirement recorded first (`write-prd`), cites it.

## <a id="s6"></a>6. Sources

- Standard for the declaration file: `ltk-wiki` `src/content/docs/reference/mod-packages/game-data.mdx`
  (proposal, status `planned`), alongside `hashtables.md` in the same directory.

Issues and pull requests (read with `gh api` on 2026-09-02):

- https://github.com/LeagueToolkit/league-mod/issues/190 - body, labels, comments (none)
- https://github.com/LeagueToolkit/league-mod/issues/191 - body, comments (none)
- https://github.com/LeagueToolkit/league-toolkit/pull/227 - metadata, files, reviews, review
  comments; diff via `git diff origin/main...HEAD` in the local checkout at `0bc9d0e`

`league-toolkit` at `origin/main` (`11bb8ba`):

- `docs/design/bin-streaming.md` - sections 1, 3, 4, 5, 7, 10, 11, 13
- `docs/design/ptch-property-patches.md` - sections 1, 2, 4, 5, 8, 9, 10, 13, 14, 15, 17
- `docs/design/value-walk.md` (PR head `0bc9d0e`) - section 4
- `docs/prd/001-ptch-property-patches.md` - sections 3, 5
- `crates/ltk_meta/src/tree.rs:15-100, 150-170`; `crates/ltk_meta/src/tree/write.rs:25-60`
- CommunityDragon `game/data/characters/teemo/skins/skin0.bin.json` and
  `game/data/characters/jade_teemo/skins/skin0.bin.json` (`latest`, fetched 2026-09-03) - the
  Teemo values in every example
- `crates/ltk_meta/src/path.rs:1-16, 268-330`; `crates/ltk_meta/src/path/parse.rs:107-109`;
  `crates/ltk_meta/src/path/resolve.rs:228-280`
- `crates/ltk_meta/src/walk.rs:1-140` (PR head); `crates/ltk_meta/src/lib.rs` (PR diff)
- `.scratch/value-walk/issues/01-walk.md` (PR head)
- `crates/ltk_ritobin/src/print.rs:180`; `crates/ltk_ritobin/src/typecheck/state.rs:89`

`league-mod` at `049ab95`:

- `CLAUDE.md`, `CONTEXT.md`, `crates/CLAUDE.md`
- `docs/overlay-builder-design.md`; `docs/adr/0001-*.md`; `docs/adr/0002-*.md`
- `crates/ltk_mod_project/src/lib.rs`; `src/config_format.rs`; `src/pack/mod.rs`;
  `src/pack/plan.rs`; `src/modpkg/format.rs`; `src/fantome/import.rs`;
  `test-data/mod.config.json`; `test-data/mod.config.toml`
- `crates/ltk_modpkg/src/metadata.rs`; `src/chunk.rs`; `src/lib.rs`; `src/hashtable.rs`;
  `src/readme.rs`; `src/license.rs`
- `crates/ltk_overlay/Cargo.toml`; `README.md`; `src/lib.rs`; `src/linked_bins.rs`;
  `src/builder/mod.rs`; `src/builder/metadata.rs`; `src/builder/resolve.rs`; `src/content.rs`;
  `src/strings.rs`; `src/meta_cache.rs`; `src/state.rs`; `src/modpkg_content.rs`;
  `src/wad_builder.rs:68`
- `crates/league-mod/src/commands/pack.rs`; `src/commands/init.rs`
- `crates/ltk_fantome/DESIGN.md`

`ltk-manager` at `main`, fetched from `raw.githubusercontent.com` and the GitHub contents API:

- `docs/adr/0012-the-overlay-merges-a-mod-over-the-games-copy.md`
- `docs/adr/0015-the-pass-reads-bins-before-files.md` (header only)
- `CONTEXT.md` (lines matching "merge")
- `specs/015-game-as-parts-source/issues/004-build-the-overlay-merge.md`
- `crates/ltk-manager-core/src/overlay/build.rs` (lines matching the overlay API)
- `crates/ltk-manager-core/src/patcher/events.rs:22-28`

Other:

- Fantome wiki, *Mod File Format*: https://github.com/LeagueToolkit/Fantome/wiki/Mod-File-Format
- cslol-manager `README.md` at `main` (GitHub readme API)
- `lol-meta-classes/db/database.py` (local checkout), for the `UiPropertyOverrideLoadable` link
  properties PRD-001 route 2 refers to

`league_structs` at `53c0809` (`origin/main`, commit "resolvers: all 28 owner properties with
shipped counts, and why the CAC entries stay unread", 2026-09-02, touching only
`docs/reversing/ResourceResolvers_VfxEffectKeys.md`), read for [section 2.9](#s2.9) and
[section 4](#s4):

- `docs/reversing/AssetNamespacing_Repathing.md` - all sections (1-8)
- `docs/reversing/ResourceResolvers_VfxEffectKeys.md` - sections 1-4.3, 5, 6.1-6.4, 7.1-7.6,
  9, verification notes
- `docs/reversing/CharacterRecords_OverrideChain.md` - sections 1-7, verification notes
- `docs/reversing/SkinResolution_Skin0Fallback.md` - sections 1-7, verification notes
- `docs/reversing/MapSkin_CharacterSkinOverride.md` - sections 1-5
- `docs/reversing/SkinParent_ChromaSystem.md` - sections 1-5
- `docs/reversing/LoadFromDefinitionSentinel.md` - sections 1-10
- `docs/reversing/BinLoadPoints_StringPaths.md` - sections 1-3
- `docs/reversing/BinLoadPoints_DataOnly.md` - sections 1-5
- `docs/reversing/BinFileCache_DataOverrides.md` - sections 1-6
- `docs/reversing/PTCH_PropertyPatches.md` - sections 1, 2, 7
- `docs/reversing/LTK_NoSkinsMode.md` - sections 1-6
- `docs/reversing/GameMutators_LevelProperties.md` - section 7
- `docs/reversing/PropertyOverrideLoadable.md` - section 8
- `docs/LeagueModding.md` - sections 2.3, 2.4, 4.2, 4.3, 6.1-6.4
- `include/league_structs.hpp` (umbrella, lines 43, 51, 80); `include/Vfx/ResourceResolver.hpp`;
  `include/Character/CharacterRecord.hpp`; `include/Environment/MapSkin.hpp`;
  `include/Spell/SpellDataResource.hpp:196`
- The other 18 markdown files (and two `data/*.tsv`) a `grep -rli resolver docs` lists (`AllLuaFilesManifest*.md`,
  `BinPropertyTypes_TypeRule.md`, `BuildingBlocks_FreeformMath.md`,
  `CustomAnnouncer_VoicePacks.md`, `GameModeCodenames.md`, `IX3dShadingModel.md`,
  `LeagueLogCodes.md`, `LoadingScreen_StepInventory.md`, `LocaleResolution_LocalizedAssets.md`,
  `MapgeoLoader.md`, `MapgeoSamplerOverrides.md`, `MetaDelta_16_14.md`,
  `RiotClient_ProductLaunch.md`, `TmeshGmesh_ParticleEmitterMesh.md`, `UIElement_RuntimeTree.md`,
  `VfxSystemDefinitionData.md`, `README.md`) were not read; the word matches there were taken
  as incidental.

`league-toolkit` at `origin/main`, for [section 2.9.5](#s2.9.5):

- `crates/ltk_meta/src/data_override/write.rs:14-35`; the file list of
  `crates/ltk_meta/src/data_override/`

Not consulted: the decompiled client itself (every client claim above is second-hand through
the `league_structs` notes, at the builds those notes name), cslol-manager source, the
`ltk-wiki` beyond a grep for "linked". Not verified by running the client: any of the
`[inferred]` items - the `{k}` path subscript, the backslash key split, a null `SkinData+112`,
the `SkinParent` consumer, the data-only wildcard `UiPropertyOverrideLoadable`.
