# Game index: consumer call sites

Question: which functions and fields of the three existing indexes do their consumers reach
for, and with what arguments. Ticket: `../issues/02-consumer-call-sites.md`. Boundary as
settled on `../map.md`: flat chunk table (hash to size and every holder WAD), object rows,
fingerprint, versioned cache, resolver trait. Folded tree, `read_dir`, ranked `search`,
exhaustive `find`, generations, state slots and wire derives stay with the manager.

## Summary

- A minimal crate boundary that satisfies every consumer offers: `holders(WadHash) ->
  &[archive]` (every holder, in a stable archive order), `size(WadHash)`, an archive list
  in merge order with a `DATA/FINAL`-relative name and a game-directory-relative path for
  each, a case-insensitive `archive_by_filename(&str)` that distinguishes "absent" from
  "ambiguous", a per-chunk visitor `for_each_chunk(|hash, size, holder|)` that the manager's
  tree fold and the object-index build feed from, `fingerprint() -> u64`, `load_or_build(game_dir,
  cache_path)` / `save(cache_path)` / `build(game_dir)` / `build_from_archives(&[path])`, and,
  behind the object feature, `ObjectIndex::build(&chunk_index, workers, called_off)`,
  `declared(BinHash) -> rows in archive order`, `declares(BinHash)`, `for_each_declaration(reader)`.
- Two overlay helpers sit on the fence and are decided by the surface ticket: `find_best_matching_wad(&[WadHash])`
  (the archive with the most hits, a pure fold over `holders`) and `compute_content_hashes_batch(game_dir,
  &HashSet<WadHash>)` (reads chunk bytes from the first holder, rayon over archives). Both are
  derivable from the flat table plus archive access. `localized_global_wads()` and
  `subchunktoc_blocked()` are filename-pattern folds over the archive list and stay overlay
  concerns unless the block-list question on the map resolves the other way.
- In this repo the overlay is the sole consumer of `ltk_overlay::GameIndex`. `league-mod` and
  `ltk_game_data` never name it. `ltk_game_data` exposes `Target::chunk_hash()` and nothing that
  resolves a target to a holder or an entry to a declaring chunk; that resolution lives in
  `ltk_overlay/src/builder/game_data.rs` (`find_wads_with_hash(hash).min()`), which is the
  declaration engine's one index call.
- The manager never names `ltk_overlay::GameIndex`. It reaches it through
  `OverlayBuilder::build` and `OverlayBuilder::analyze_single_mod`, reads
  `ModWadReport.game_index_fingerprint` as a staleness key, maps `Error::GameDir` to a wire
  category, and deletes `game_index.bin` by file name. Every `GameIndex` token in the manager
  refers to its own `game_index::GameIndex`.
- Manager modules that move wholesale: `object_index/build.rs` (rows, `Declaration`,
  `for_each_declaration`, the bin sniff), the `ObjectNames` trait half of
  `object_index/names.rs`, and the enumeration half of `game_wads.rs` (`GameArchives::list`,
  `for_each_chunk`, `archive_path`). `problems/game.rs::HeldChunks` is a third, private flat
  chunk table (hash to first archive) and is deleted in favour of the crate's. Modules that
  stay: `game_index.rs` fold, `read_dir`, `search`, `find`, `stats`, `GameIndexState`,
  `SearchGeneration`, `FindGeneration`, all wire structs; `object_index/{browse,find,search,
  references,spells,state,walk,wire}.rs`; `WadCache`; every `src-tauri` command. Modules that
  split: `game_index.rs` (build feed to the crate, tree fold stays), `game_wads.rs`
  (`GameArchives` to the crate, `WadCache` and `chunk_head` stay or go to `ltk_wad`),
  `object_index.rs` root (`ObjectIndex` rows and `declared`/`declares` to the crate;
  `Names`, `parse_hash`, `UNNAMED_PREFIX` stay), `object_index/names.rs` (trait to the crate,
  `CacheNames` impl stays).
- Two identity conventions collide at the boundary. The overlay keys an archive by its
  game-directory-relative `Utf8PathBuf` (`DATA/FINAL/Champions/Aatrox.wad.client`) and every
  holder is kept. The manager keys an archive by its `DATA/FINAL`-relative forward-slash name
  (`Champions/Aatrox.wad.client`), plus an ordinal into `wads()`, and keeps only the first
  holder. The crate row carries both spellings or one plus a `game_dir` to join against, and
  the manager's fold takes holder `[0]` of a stable archive order.

## A. `ltk_overlay::GameIndex`

Definition: `/home/crauzer/dev/lol/league-mod/crates/ltk_overlay/src/game_index.rs`. Public
fields `wad_index: HashMap<String, Vec<Utf8PathBuf>>` (lowercased file name to paths),
`hash_index: HashMap<WadHash, Vec<Utf8PathBuf>>`, `game_fingerprint: u64`,
`subchunktoc_blocked: HashSet<WadHash>`. Methods `new`, `build(game_dir)`,
`load_or_build(game_dir, cache_path)`, `save(cache_path)`, `find_wad(filename)`,
`find_wads_with_hash(WadHash)`, `game_fingerprint()`, `localized_global_wads()`,
`subchunktoc_blocked()`, `find_best_matching_wad(&[WadHash])`,
`compute_content_hashes_batch(game_dir, &HashSet<WadHash>)`. Cache: MessagePack, `CACHE_VERSION = 3`,
fingerprint-checked on load. Errors: `Error::WadNotFound(path)`, `Error::AmbiguousWad { name, count }`,
`GameDirError::MissingDataFinal { path }` (from `build`), `Error::cache_write` / `Error::read`
(from `save` / `load_cache`).

### A.1 `crates/ltk_overlay` (non-test)

Paths are relative to `/home/crauzer/dev/lol/league-mod/crates/ltk_overlay/src/`.

| Call site | Call | Needs from the index | Boundary |
| --- | --- | --- | --- |
| `builder/mod.rs:641` | `GameIndex::load_or_build(game_dir, &state_dir.join("game_index.bin"))` in `analyze_single_mod` | cached build keyed by fingerprint; caller picks the file | inside (cache, load-or-build) |
| `builder/mod.rs:731` | `GameIndex::load_or_build(&self.game_dir, &cache_path)` in `build` | same | inside |
| `builder/mod.rs:632`, `:719` | `GameDirError::MissingDataFinal` raised by the builder before the index | error type shared with `build` | inside (error) |
| `builder/mod.rs:141-142` | `find_wads_with_hash(path_hash)` in `OverrideMeta::is_cross_wad_import` | every holder of a hash, compared against `fallback_wad` | inside (flat table) |
| `builder/mod.rs:168-169` | `find_wads_with_hash(path_hash)` in `OverrideMeta::route_targets` | every holder; empty means "new entry" | inside |
| `builder/mod.rs:454`, `:470` | `route_targets(.., game_index)`, `game_fingerprint()` in `ModWadReport::from_meta` | holders; fingerprint copied onto the report | inside |
| `builder/mod.rs:735` | `string_override_mode.resolve_locales(&game_index)` | `localized_global_wads()` | fence: archive-name fold |
| `builder/mod.rs:763`, `:789`, `:800`, `:889` | `game_fingerprint()` into `OverlayState::new`, `try_exact_match_skip`, `supports_incremental` | fingerprint as `u64` | inside (fingerprint) |
| `builder/mod.rs:812`, `:816`, `:817`, `:819`, `:835` | passes `&game_index` to metadata, strings, game data, distribution, linked-bin check | reference passing | n/a |
| `builder/mod.rs:1051` | `find_wad(&format!("Global.{locale}.wad.client"))`, matches `Err(Error::WadNotFound(_))` to skip | case-insensitive file-name lookup; absent vs ambiguous distinguished by error variant | inside (archive by filename) |
| `builder/mod.rs:1065` | `GameDirError::WadOutsideGameDir` when `strip_prefix(game_dir)` fails | the returned path is absolute and joined under `game_dir` | overlay's own error |
| `builder/metadata.rs:214` | `find_wad(wad_name)` in `resolve_fallback_wad`; `Err(WadNotFound)` falls through, other errors propagate | file-name lookup, absolute path, `AmbiguousWad` propagated | inside |
| `builder/metadata.rs:236` | `find_best_matching_wad(path_hashes)` | archive with most holder hits over a hash slice | fence: fold over flat table |
| `builder/metadata.rs:279-280` | `find_wad(&sibling).ok()?` in `resolve_unlocalized_wad` | file-name lookup; ambiguity treated as absent | inside |
| `builder/metadata.rs:377` | `find_best_matching_wad(&all_hashes)` in `route_unroutable_to_dominant_wad` | same as `:236` | fence |
| `builder/metadata.rs:420` | `subchunktoc_blocked()` in `filter_override_metadata` | `HashSet<WadHash>` of `.wad.SubChunkTOC` hashes | fence: open on the map |
| `builder/metadata.rs:421` | `strings::blocked_stringtable_hashes(game_index)` | `localized_global_wads()` | fence |
| `builder/metadata.rs:458` | `compute_content_hashes_batch(game_dir, &override_hashes)` | first holder per hash, chunk bytes read and hashed, rayon per archive | fence: needs archive access |
| `builder/metadata.rs:470` | `meta.is_cross_wad_import(path_hash, game_index)` | holders | inside |
| `builder/metadata.rs:556` | `game_fingerprint()` keys `OverrideMetaCache::load/new` | fingerprint | inside |
| `builder/metadata.rs:26-60`, `:92`, `:136`, `:165`, `:507-526`, `:549-577`, `:612-619` | `&GameIndex` threaded through `collect_*_metadata`, `build_mod_wad_reports` | reference passing | n/a |
| `builder/resolve.rs:309` | `meta.route_targets(path_hash, game_index)` | holders | inside |
| `builder/resolve.rs:322`, `:324` | `find_wads_with_hash(path_hash).is_none()`, `is_cross_wad_import` | presence test, holders | inside |
| `builder/game_data.rs:124` | `find_wads_with_hash(hash).and_then(|p| p.iter().min())` | the lexicographically first holder of a declaration target, read as the base via `strings::read_game_chunk(game_dir, wad_rel_path, hash)` | inside (holders); the byte read is archive access |
| `builder/game_data.rs:141` | `subchunktoc_blocked().contains(&hash)` | block list | fence |
| `strings.rs:79-80` | `localized_global_wads()` in `StringOverrideMode::resolve_locales` | `(locale, path)` pairs from `global.<locale>.wad.client` names with exactly one path | fence: archive-name fold |
| `strings.rs:114-115` | `localized_global_wads()` in `blocked_stringtable_hashes` | same | fence |
| `linked_bins.rs:72-73` | `find_wads_with_hash(path_hash)` in `PresentSet::holds`, filtered by `is_wad_blocked` | holders as paths whose file name is matched against a blocklist | inside |
| `meta_cache.rs:178-223` | `OverrideMetaCache` stores and compares `game_fingerprint: u64` | fingerprint value | inside (fingerprint) |
| `state.rs:121`, `:203`, `:302`, `:329` | `OverlayState.game_fingerprint`, `supports_incremental(game_fingerprint)` | fingerprint value persisted in `overlay.json` | inside (fingerprint) |
| `lib.rs:117`, `:141` | `pub mod game_index; pub use game_index::GameIndex;` | re-export | removed per the map |
| `error.rs:94`, `:109`, `:169` | `Error::AmbiguousWad`, `Error::GameDir(GameDirError)`, `enum GameDirError` | error surface | inside for `AmbiguousWad`/`MissingDataFinal`; `WadOutsideGameDir`, `StringtableChunkMissing` stay overlay |
| `content.rs:113`, `:156` | doc links to `find_wad` and "hash lookup" | docs only | n/a |

### A.2 `crates/ltk_overlay` (tests)

Tests construct `GameIndex` as a struct literal or through `GameIndex::new()` and insert into
`hash_index` / `wad_index` directly: `builder/mod.rs:1336-1341`, `builder/metadata.rs:709`
(`GameIndex::build(game_dir)` over an empty `DATA/FINAL`), `:810-814`, `:865-869`, `:909-913`,
`:962-966`, `:1012-1016`, `:1109-1113`, `:1232-1240`; `strings.rs:704-708`; `linked_bins.rs:312`,
`:334`, `:365-366`, `:390-391`, `:417-418`, `:444`, `:469-470`. This is the one dependency on the
fields being `pub`. The crate serves it with a builder or a `from_rows` constructor for tests;
`Default`/`new()` plus an insert API is the equivalent shape.

### A.3 `crates/league-mod`, `crates/ltk_game_data`, other crates

No use. `grep -rn "GameIndex\|find_wads_with_hash\|game_fingerprint\|load_or_build"` over
`crates/league-mod`, `crates/ltk_game_data`, `crates/ltk_modpkg`, `crates/ltk_fantome`,
`crates/ltk_mod_project`, `crates/ltk_hashtable`, `crates/ltk_mod_core` returns only
`Modpkg::wad_index` (a modpkg table position, unrelated) and `FantomeReader::packed_wad_index`.
`ltk_game_data` (`lib.rs:106-157`) exposes `Target::chunk_hash() -> u64` and `path_hash(&str)`;
holder resolution is the overlay's (`builder/game_data.rs:124`).

### A.4 LTK Manager, uses of `ltk_overlay`

Paths relative to `/mnt/c/dev/ltk/ltk-manager/`. None names `ltk_overlay::GameIndex`.

| Call site | Call | Needs from the index | Boundary |
| --- | --- | --- | --- |
| `crates/ltk-manager-core/src/overlay/build.rs:64-70` | `ltk_overlay::OverlayBuilder::new(game_dir, overlay_root, state_dir)...build()` | the builder loads or builds `state_dir/game_index.bin` itself | inside, via the overlay |
| `crates/ltk-manager-core/src/mods/analysis/scan.rs:115` and `src-tauri/src/commands/mods.rs:365` | `OverlayBuilder::analyze_single_mod(&game_dir, &state_dir, &mut enabled_mod)` | same load-or-build; returns `ModWadReport` | inside, via the overlay |
| `crates/ltk-manager-core/src/mods/analysis/wad_reports.rs:36`, `:76`, `:113`, `:148`, `:224-264` | reads `ModWadReport.game_index_fingerprint`, keeps `current_game_index_fp`, marks a cached report stale on mismatch | fingerprint as an opaque `u64` | inside (fingerprint) |
| `crates/ltk-manager-core/src/error.rs:89-96`, `:226` | `From<&ltk_overlay::Error> for OverlayErrorCategory`, `ltk_overlay::Error::GameDir(_)` | error category | inside (error variant reachable from the overlay error) |
| `crates/ltk-manager-core/src/overlay/artifacts.rs:86-106` | `purge_overlay_artifacts(profile_dir, include_game_index)` deletes `game_index.bin` | the cache file's name and location | caller-owned per the map |
| `crates/ltk-manager-core/src/mods/overlay_content.rs:31` | doc: profile directory shared "for `game_index.bin`" | same | caller-owned |
| `crates/ltk-manager-core/src/overlay/mod.rs:76`, `overlay/resolve.rs:35`, `utils/game.rs:61` | `GameDir::wads()` lowercased file names for blocklist regex expansion | a separate `DATA` walk, not the index | outside; a candidate to read from the crate's archive list |

`BinNames::invalidate_game_index` (`problems/names.rs:124`, `mods/health/sweep.rs:281`,
`src-tauri/src/commands/hashtables.rs:131`) names the process-wide Mimir FNV table, not any
game index.

## B. LTK Manager's own indexes

Crates: `crates/ltk-manager-core` (library) and `src-tauri` (the Tauri shell, package
`ltk-manager`). Paths relative to `/mnt/c/dev/ltk/ltk-manager/`.

Definitions:

- `crates/ltk-manager-core/src/game_index.rs`: `GameIndex { dirs: Vec<Dir>, unknown: Vec<File>,
  wads: Vec<String> }`, one `File` per chunk hash (first archive wins), `File { name, path_hash:
  u64, size_bytes: u64, wad: u32, mask }`. `build(&GameArchives, &LayeredHashDb)`, `read_dir`,
  `search`, `find`, `stats`, `files_under`, `file_at`, `unnamed_at`, `for_each_named_file`,
  `for_each_unnamed_file`, `wads()`. `GameIndexState(Mutex<Option<Arc<GameIndex>>>)` with
  `get_or_build`/`clear`. `SearchGeneration`, `FindGeneration`, `SEARCH_LIMIT = 100`,
  `FIND_LIMIT = 20_000`, `UNKNOWN_DIR = "?"`, wire structs `GameDirListing`, `GameDirEntry`,
  `GameFileEntry`, `GameIndexStats`, `GameSearchHit/Result`, `GameFindHit/Result`.
- `crates/ltk-manager-core/src/game_wads.rs`: `GameArchives { final_dir }` with `resolve(&Config)`,
  `at(game_dir)`, `list() -> Vec<GameWadSummary>`, `read(wad_name, &LayeredHashDb) ->
  Vec<GameWadEntry>`, `for_each_chunk(wad_name, &LayeredHashDb, |hash, Option<&str>, size|)`,
  `archive_path(wad_name)`. `WadCache` (LRU of mounted `Wad`s) with `read_chunk(&GameArchives,
  wad_name, WadHash)`, `mounted()`, `clear()`. Free fn `chunk_head(&mut Wad, &WadChunk, &mut
  ChunkDecoder, want)`.
- `crates/ltk-manager-core/src/object_index.rs` and `object_index/`: `ObjectIndex { declared:
  Arc<Declarations>, names: Names }`, rows `Row { object: BinHash, class: BinHash, file: WadHash }`
  plus `DeclaringFile { path_hash, path: Option<Box<str>>, wad: u32 }`. `pub use`:
  `build::{Declaration, for_each_declaration}`, `names::{CacheNames, ObjectNames}`,
  `spells::{CharacterSpell, SpellCatalog}`, `state::{BuildTicket, ObjectFindGeneration,
  ObjectIndexSnapshot, ObjectIndexState, ObjectReferenceGeneration, ObjectSearchGeneration}`,
  `walk::{LayerBin, WalkRequest, WalkTarget, layer_bins}`, `wire::{...}`. Methods `build(&GameIndex,
  &GameArchives, workers, called_off)`, `named(&impl ObjectNames)`, `stats`, `declares`,
  `declared`, `search`, `find`, `object_dir`, `class_references`, `walk`, `character_spells`.
  `parse_hash(&str) -> Option<BinHash>`, `UNNAMED_PREFIX = "?"`.

Classification key: **chunk-table** (hash to size/holder), **object-rows**, **archive**
(enumerate, mount, read bytes), **browse/search** (tree, ranked or exhaustive scan),
**lifecycle** (state slots, tickets, generations, clears), **IPC** (wire structs, commands).

### B.1 `crates/ltk-manager-core/src/game_index.rs` consumers

| Call site | Call | Needs | Class | Boundary |
| --- | --- | --- | --- | --- |
| `game_index.rs:282`, `:296` | `archives.list()`, `archives.for_each_chunk(&wad.name, resolver, ..)`; `seen.insert(path_hash)` keeps the first holder | archive list in name order; per-chunk `(hash, resolved path, size)`; resolver `LayeredHashDb` | archive + chunk-table build | inside: this is the crate's build feed; the fold on top is outside |
| `object_index/build.rs:303` | `game.wads().to_vec()` | archive names in merge order, ordinal-indexed | chunk-table | inside |
| `object_index/build.rs:308` | `game.for_each_named_file(|hash, path, wad| ..)` filtered by `is_bin(path)` / `is_bare(path)` | every named chunk with its resolved path and holder ordinal | chunk-table + resolver | inside |
| `object_index/build.rs:316` | `game.for_each_unnamed_file(|hash, wad| ..)` | every unnamed chunk and holder ordinal | chunk-table | inside |
| `object_index/references.rs:11`, `find.rs:8`, `walk/run.rs:18`, `search.rs:11` | `FIND_LIMIT`, `SEARCH_LIMIT` | result caps | browse/search | outside |
| `object_index/state.rs:7`, `:16`, `:36`, `:56` | wraps `SearchGeneration` in three object generations | cancel tickets | lifecycle | outside |
| `game_extract.rs:285` | `index.files_under(path)` in `ExtractJob::plan` | every chunk below a tree path with `path_hash`, `path`, `size_bytes`, `wad` | browse (tree) reading chunk rows | outside (tree); the rows it yields are inside |
| `src-tauri/src/commands/game_index.rs:22` | `index.stats()` | counts | IPC | outside |
| `src-tauri/src/commands/game_index.rs:32-34` | `index.read_dir(&path)` | folded listing | browse | outside |
| `src-tauri/src/commands/game_index.rs:47-55` | `index.file_at(&path)` per path in `locate_game_files` | one chunk by resolved path: hash, size, one holder | chunk-table (by path) | inside, as `hash(path)` then row lookup; the tree walk is the manager's |
| `src-tauri/src/commands/game_index.rs:73` | `index.search(&query, overtaken)` with `SearchGeneration::claim/overtook` | ranked scan | browse/search + lifecycle | outside |
| `src-tauri/src/commands/game_index.rs:108` | `index.find(&FindQuery, overtaken)` with `FindGeneration` | exhaustive scan | browse/search + lifecycle | outside |
| `src-tauri/src/commands/game_index.rs:172-175` | `GameIndexState::clear()`, `WadCache::clear()`, `ObjectIndexState::clear()` | drop everything derived from the install | lifecycle | outside |
| `src-tauri/src/commands/game_index.rs:207-215` | `GameArchives::resolve(config)`, `GameIndexState::get_or_build(&archives, resolver.tables())` | build once per app, share as `Arc` | lifecycle + archive | outside (slot); the build it calls is inside |
| `src-tauri/src/commands/game_extract.rs:165-176` | `GameIndexState::get_or_build`, hands `(&GameIndex, &GameArchives, &WadPathResolver)` to the extract | same | lifecycle | outside |
| `src-tauri/src/commands/document_assets.rs:63`, `:91-92`, `:100` | `built_game_index`, `index.file_at(&path.to_lowercase())`, `index.unnamed_at(WadHash::hash_str(path).0)`, `unnamed_at(hash.0)` | a chunk by path or by hash: `wad` name and `path_hash` for `AssetRef::GameChunk` | chunk-table | inside |
| `src-tauri/src/commands/hashtables.rs:130` | `GameIndexState::clear()` after a hashtable sync | names changed, rows did not | lifecycle | outside; a crate table keyed by hash survives a sync and only the fold rebuilds |
| `src-tauri/src/setup.rs:122-125` | `app.manage(GameIndexState::default())`, `SearchGeneration`, `FindGeneration` | Tauri state | lifecycle | outside |
| `src-tauri/src/commands/object_index.rs:82` | `built_game_index(app, config)` feeds `ObjectIndex::build` | the game index and archives | lifecycle | outside (slot); build inside |

### B.2 `crates/ltk-manager-core/src/game_wads.rs` consumers

| Call site | Call | Needs | Class | Boundary |
| --- | --- | --- | --- | --- |
| `game_index.rs:282`, `:296` | `list()`, `for_each_chunk(name, resolver, visit)` | see B.1 row 1 | archive | inside (build feed) |
| `game_extract.rs:255`, `:303`, `:371`, `:411` | `archives.archive_path(wad)` then `Wad::mount` per archive; `WadCache` avoided on purpose (`:353`) | archive path by `DATA/FINAL`-relative name; bytes | archive | archive path lookup inside; byte reads outside |
| `object_index/build.rs:65`, `:122` | `archives.archive_path(self.name)`, `Wad::mount`, `chunk_head(wad, &chunk, decoder, MAX_MAGIC_SIZE)` to sniff bins | archive path; bounded chunk prefix | archive | inside for the object build's own reads; `chunk_head` itself is an `ltk_wad` shape |
| `object_index/walk/run.rs:111`, `:322` | `WalkRequest.archives`, `archives.archive_path(name)` then `Wad::mount` | archive path | archive | outside (reference walk) |
| `problems/game.rs:51-53`, `:106`, `:139`, `:170` | `GameArchives::resolve/at`, `list()`, `archive_path(wad_name)` + `Wad::mount` to collect every `path_hash` into `HeldChunks { names, at: HashMap<WadHash, usize> }`; `WadCache::read_chunk(&archives, name, path)` | hash to first holder, then bytes | archive + a private chunk-table | the table duplicates the crate's flat table; the read stays |
| `problems/engine/archive.rs:393`, `:436` | `chunk_head(wad, chunk, decoder, limit)` on a mod archive | bounded chunk prefix | archive | outside (mod content, not the install) |
| `preview/source.rs:72` | `wads.read_chunk(&GameArchives::resolve(config)?, wad, path_hash)` for `AssetRef::GameChunk` | bytes of one chunk of one archive | archive | outside |
| `ritobin.rs:73`, `preview/mod.rs:171`, `:209` | `&WadCache` threaded to `AssetRef::read/info/to_disk_path` | same | archive | outside |
| `src-tauri/src/commands/game_wads.rs:14`, `:32` | `GameArchives::resolve(&config)?.list()`, `archives.read(&wad_name, resolver.tables())` | archive list; one archive's chunk rows with resolved paths | archive + IPC | list inside; `read` is `for_each_chunk` collected into a wire struct, outside |
| `src-tauri/src/protocol.rs:67`, `commands/{bin.rs:52,461, preview.rs:26,46, ritobin.rs:46, skin.rs:36,93,117}` | `app.state::<WadCache>()` handed to asset reads | mounted-archive cache | archive | outside |
| `src-tauri/src/setup.rs:126` | `app.manage(WadCache::default())` | Tauri state | lifecycle | outside |
| `src-tauri/src/commands/object_index.rs:418` | `GameArchives::resolve(&config)` for `WalkRequest` | archive view | archive | outside |

### B.3 `crates/ltk-manager-core/src/object_index/` consumers

| Call site | Call | Needs | Class | Boundary |
| --- | --- | --- | --- | --- |
| `src-tauri/src/commands/object_index.rs:83` | `ObjectIndex::build(&game, &archives, files_at_once(), &|| !state.is_current(ticket))` | rows from every bin chunk of the install; a cancel probe | object-rows | inside (feature) |
| `src-tauri/src/commands/object_index.rs:95`, `:643` | `index.named(&CacheNames::new(&bin_tables, &wad_resolver))` at warm and after a hashtable sync | resolve object, class and file names through `ObjectNames` | object-rows + resolver | trait inside; `CacheNames` impl outside |
| `src-tauri/src/commands/object_index.rs:535` | `index.declared(parse_hash(text)?)` in `declared_objects` | every declaration of an object in archive order: file path, wad, class | object-rows | inside |
| `src-tauri/src/commands/object_index.rs:133` | `index.search(&query, overtaken)` with `ObjectSearchGeneration` | ranked scan | browse/search + lifecycle | outside |
| `src-tauri/src/commands/object_index.rs:210` | `index.object_dir(&prefix)` | folded prefix tree | browse | outside |
| `src-tauri/src/commands/object_index.rs:271` | `index.find(query, class_term, overtaken)` with `ObjectFindGeneration` | exhaustive scan | browse/search + lifecycle | outside |
| `src-tauri/src/commands/object_index.rs:391` | `index.class_references(hash, overtaken)` with `ObjectReferenceGeneration` | rows grouped by declaring file | object-rows grouped for the wire | outside as shaped; the row scan underneath is inside |
| `src-tauri/src/commands/object_index.rs:445` | `index.walk(&WalkRequest { target, layers, archives, budget, workers }, &names, schema, overtaken, progress)` | full bin walk of layers and install | archive + browse/search | outside |
| `src-tauri/src/commands/object_index.rs:190` | `index.character_spells(&character)` | rows filtered by class `SpellObject` and path prefix | object-rows shaped for the wire | outside |
| `src-tauri/src/commands/object_index.rs:62`, `:66`, `:102`, `:126`, `:184`, `:204`, `:264`, `:378`, `:526`, `:643` | `ObjectIndexState::{begin, finish, clear, snapshot, rename}` | four-slot state with tickets | lifecycle | outside |
| `src-tauri/src/setup.rs:127-130` | `app.manage(ObjectIndexState::default())`, three generations | Tauri state | lifecycle | outside |
| `src-tauri/src/commands/game_index.rs:175`, `hashtables.rs:132` | `ObjectIndexState::clear()`, `rename_after_sync(app)` | drop or rename with the install or the tables | lifecycle | outside |
| `workshop/content.rs:474` | `for_each_declaration(BufReader<File>, |Declaration| ..)` on a project layer's bin | objects a bin declares, no install | object-rows (parser) | inside (the one parser both the build and the workshop use) |
| `bin_document.rs:42`, `:1385` | `impl RowNames for CacheNames<'_>`; `ObjectDeclaration` in `declared_in` | name resolution shape shared with the bin editor | resolver | outside (`CacheNames` stays); the trait it implements is inside |
| `events.rs:30` | `pub use object_index::ReferenceWalkProgress` | progress event | IPC | outside |
| `skin/mod.rs:1135`, `src-tauri/src/commands/{bin.rs, document_assets.rs}` (many) | `object_index::parse_hash(text)`, `CacheNames::new(&bin, &wad)` | eight-hex-digit `BinHash` parse; names | helper + resolver | outside |

## Moves / stays / splits

Manager files, decided against the boundary on the map. The migration itself belongs to the
manager's tracker.

Moves (replaced by the crate):

- `crates/ltk-manager-core/src/object_index/build.rs`: `Declaration`, `for_each_declaration`,
  `ArchiveJob::read`, `sniffs_as_bin`, `ObjectIndex::build`, `Declarations`, `Row`,
  `DeclaringFile`. The build's input becomes the crate's chunk table instead of the folded tree.
- `crates/ltk-manager-core/src/object_index/names.rs`: the `ObjectNames` trait. The crate's
  resolver trait covers object and class names by `BinHash` and file names by `WadHash`.
- `crates/ltk-manager-core/src/game_wads.rs`, the `GameArchives` half: `list`,
  `for_each_chunk`, `archive_path`, `at(game_dir)`. `resolve(&Config)` stays as a one-line
  adapter over the crate's `at`.
- `crates/ltk-manager-core/src/problems/game.rs` `HeldChunks` and `InstalledContent::index/hashes_in`:
  deleted; `holds` and `read` query the crate table for the first holder.

Stays (application-side):

- `crates/ltk-manager-core/src/game_index.rs`: `Dir`, `File`, the fold, `finalize`, `read_dir`,
  `search`, `find`, `stats`, `files_under`, `file_at`, `unnamed_at`, `GameIndexState`,
  `SearchGeneration`, `FindGeneration`, `SEARCH_LIMIT`, `FIND_LIMIT`, `UNKNOWN_DIR`, every wire
  struct. `build` reads its rows from the crate's `for_each_chunk` instead of `GameArchives`.
- `crates/ltk-manager-core/src/object_index.rs` root: `Names`, `parse_hash`, `UNNAMED_PREFIX`,
  `STALE_CHECK_INTERVAL`, `declaration()` (wire shaping).
- `crates/ltk-manager-core/src/object_index/{browse,find,search,references,spells,state,walk,wire}.rs`
  and `walk/`.
- `crates/ltk-manager-core/src/game_wads.rs`, the `WadCache` half, and `chunk_head` (a
  candidate for `ltk_wad`, not for the index crate).
- `crates/ltk-manager-core/src/game_extract.rs`, `preview/`, `ritobin.rs`, `problems/engine/`,
  `workshop/content.rs` (its `for_each_declaration` import repoints to the crate).
- Every file under `src-tauri/src/`.

Splits:

- `crates/ltk-manager-core/src/game_index.rs`: build feed and first-holder dedup to the crate;
  tree fold and everything below it stays.
- `crates/ltk-manager-core/src/game_wads.rs`: `GameArchives` moves, `WadCache` and `chunk_head` stay.
- `crates/ltk-manager-core/src/object_index.rs`: `ObjectIndex.declared` rows, `declares`,
  `declared`, `stats` move; `names`, browse and wire shaping stay.
- `crates/ltk-manager-core/src/object_index/names.rs`: `ObjectNames` moves, `CacheNames` stays.

## Sources

- `/home/crauzer/dev/lol/league-mod/crates/ltk_overlay/src/game_index.rs` (definition; read whole)
- `/home/crauzer/dev/lol/league-mod/crates/ltk_overlay/src/{builder/mod.rs, builder/metadata.rs, builder/resolve.rs, builder/game_data.rs, strings.rs, linked_bins.rs, meta_cache.rs, state.rs, error.rs, lib.rs, content.rs}`
- `/home/crauzer/dev/lol/league-mod/crates/ltk_game_data/src/lib.rs`
- `grep -rn --include='*.rs' -E "GameIndex|find_wad|find_wads_with_hash|hash_index|wad_index|game_fingerprint|subchunktoc_blocked|load_or_build|GameDirError|AmbiguousWad|game_index" /home/crauzer/dev/lol/league-mod/crates`
- `/mnt/c/dev/ltk/ltk-manager/crates/ltk-manager-core/src/{game_index.rs, game_wads.rs, object_index.rs, object_index/build.rs, object_index/names.rs, object_index/state.rs, object_index/find.rs, object_index/references.rs, object_index/browse.rs, object_index/spells.rs, object_index/walk/run.rs, object_index/wire.rs, game_extract.rs, problems/game.rs, problems/engine/archive.rs, preview/source.rs, ritobin.rs, workshop/content.rs, bin_document.rs, error.rs, overlay/build.rs, overlay/mod.rs, overlay/artifacts.rs, mods/analysis/scan.rs, mods/analysis/wad_reports.rs, utils/game.rs, problems/names.rs}`
- `/mnt/c/dev/ltk/ltk-manager/src-tauri/src/{setup.rs, protocol.rs, commands/game_index.rs, commands/object_index.rs, commands/game_wads.rs, commands/game_extract.rs, commands/document_assets.rs, commands/hashtables.rs, commands/mods.rs}`
- `grep -rn --include='*.rs' "GameIndex\|game_index" /mnt/c/dev/ltk/ltk-manager/crates /mnt/c/dev/ltk/ltk-manager/src-tauri`
- `/mnt/c/dev/ltk/ltk-manager/Cargo.toml` (pins: `ltk_overlay = "0.9.8"`, `ltk_wad = "0.5.4"`, `ltk_hash` and `ltk_meta` from `league-toolkit` rev `b800ad5`)
