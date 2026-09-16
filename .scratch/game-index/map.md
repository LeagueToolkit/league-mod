# Map: game index crate

Status: route clear; implementation tickets 10 to 16 ready for /implement

## Destination

A spec `docs/design/game-index.md`, one ADR for the new crate, and an implementation-ready
ticket set under `.scratch/game-index/issues/` for a published `ltk_game_index` crate that both
`ltk_overlay` and the game data declaration engine consume, with nothing left to decide before
`/implement` picks the tickets up.

## Notes

- Domain: League of Legends installs under `Game/DATA/FINAL`, `.wad.client` archives, chunk path
  hashes (XXH64 of the lowercased path, `ltk_wad::WadHash`), bin objects (`ltk_hash::BinHash`),
  the declaring chunk of an object, holder WADs of a chunk.
- Reference implementation: `C:\dev\ltk\ltk-manager\crates\ltk-manager-core\src` (WSL
  `/mnt/c/dev/ltk/ltk-manager/crates/ltk-manager-core/src`), modules `game_index.rs`,
  `game_wads.rs`, `object_index/`. Seed for the chunk index here:
  `crates/ltk_overlay/src/game_index.rs`.
- Consumers the spec must serve: `ltk_overlay` (holder fan-out, WAD filename lookup, fingerprint,
  SubChunkTOC block list), `ltk_game_data` (`target` to holder chunks, `entries` to declaring
  chunks), LTK Manager (object index rows; its own browse tree and search stay with it).
- Skills every session consults: `rust-skills`, `write-spec`, `write-adr`, `write-ticket`,
  `codebase-design`. Vocabulary lives in the spec's vocabulary section, never in a CONTEXT.md
  (repo rule).
- Repo rules: one ADR before adding a crate. Prose declarative, free of time and cause. Commits
  are one conventional subject line and happen only when asked. Unrelated staged files in the
  working tree are never touched.
- Research findings land in `.scratch/game-index/research/<name>.md` in the working tree, not on
  a branch. The working tree carries unrelated staged work that a branch switch would disturb.

### Settled at charting

- Planning only. Implementation is a separate effort worked from the tickets.
- Manager migration: which manager code moves and which stays is decided here. The migration
  work itself belongs to the manager's tracker.
- One crate, `ltk_game_index` as the working name. The object index sits behind a feature flag.
- The chunk index core is a flat table: chunk hash to size and every holder WAD. No folded
  directory tree, no `read_dir`, no ranked `search`, no `find`, no cancellation generations,
  no wire-type derives. Those stay in the manager.
- Name resolution enters through a resolver trait the crate defines. `ltk_hashtable` implements
  it here. A direct `ltk_hashtable` dependency is the fallback if the trait costs more churn than
  it saves (see the resolver ticket).
- The crate owns the game fingerprint and a versioned serialized cache with save and
  load-or-build. The cache file's location is the caller's.
- `ltk_overlay::GameIndex` is removed. `ltk_overlay` depends on the new crate. The one ADR for the
  new crate records the removal in a line.
- Two build entry points: from a game directory, and from a caller-supplied archive list. Rayon
  is a default-on feature.
- No performance baseline measurement in this effort.

## Decisions so far

<!-- one line per resolved ticket: [title](issues/NN-slug.md): gist -->

- [Game index: ltk_meta reader available on crates.io](issues/01-ltk-meta-pin.md): the object index build compiles against published `ltk_meta` 0.8.2 unchanged (`BinStream::mount`/`entries`, `ObjectEntry`, `BinOverride`); only the manager's property walk needs the unreleased `feat/value-walk` rev; `ltk_wad` 0.5.4, `ltk_hash` 0.4.0, `ltk_file` 0.2.11 are shared by both sides with a single hash type per graph. Findings: [research/ltk-meta-pin.md](research/ltk-meta-pin.md).
- [Game index: every call site the crate must serve](issues/02-consumer-call-sites.md): the minimal boundary is holders-of-hash, chunk size, an ordered archive list with both `DATA/FINAL`-relative and game-dir-relative spellings, case-insensitive archive-by-filename distinguishing absent from ambiguous, a per-chunk visitor, fingerprint, cache, two build entries, and behind the object feature `ObjectIndex::build`/`declared`/`declares`/`for_each_declaration`; the overlay is the only consumer of `ltk_overlay::GameIndex` and the declaration engine's one index call is `find_wads_with_hash(hash).min()` in `builder/game_data.rs`; manager `object_index/build.rs`, the `ObjectNames` trait and `GameArchives` enumeration move, the tree and everything UI stays. Findings: [research/consumer-call-sites.md](research/consumer-call-sites.md).
- [Game index: chunk index surface](issues/03-chunk-index-surface.md): `ltk_game_index::GameIndex`; archives are `ArchiveId` ordinals into a sorted list carrying name and path; rows hold size and per-holder checksum; lookups by hash and by path, archive by filename with absent/ambiguous, dominant holder, per-chunk visitor, fingerprint newtype; unreadable archives skip and are recorded; MessagePack cache with version and fingerprint; private fields; content hashing, locale and SubChunkTOC folds stay in the overlay.
- [Game index: name resolver boundary](issues/04-name-resolver.md): a batch-shaped WAD-path resolver trait at the crate root, implemented for `ltk_hashtable::GameResolver` behind a `hashtable` feature; only the object build consumes it and it is optional (absent means sniff every chunk); the chunk index and object rows are hash-only, object display names stay in the manager.
- [Game index: object index surface](issues/05-object-index-surface.md): feature `objects` on published `ltk_meta` 0.8.2; `ObjectIndex::build(&GameIndex)` plus `build_with(&BuildOptions)` (optional resolver, workers, called_off); rows `(object, class, chunk, archive)`; `declares`, `declarations` in archive order, per-chunk reverse lookup, stats, free `for_each_declaration(reader)`; skip-and-count failures; rows cached beside the chunk table on the same fingerprint; rayon behind the default feature.
- [Game index: resolving declaration selectors](issues/06-declaration-resolution.md): holder means archive, a chunk containing an entry is a declaring chunk (wiki reworded); resolution stays in `ltk_overlay`, an `entries` module lowers to one application per declaring chunk; `ltk_game_data::Module` gains a `Selector` enum (`Target` | `Entries`); every declaring chunk is edited with an informational diagnostic; new kinds `EntryUnresolved`, `EntryFanOut`, `IndexUnavailable`; the object index is built lazily only when `entries` is declared, with an overlay build stage and a warning on failure.
- [Game index: write spec, ADR and implementation tickets](issues/07-write-documents.md): `docs/design/game-index.md`, ADR-0009, game-data spec sections 2 to 6 and 8, umbrella plus implementation tickets 10 to 16; GitHub sync pending the maintainer's word.

## Not yet specified



## Out of scope

- The manager's folded directory tree, `read_dir`, ranked `search`, exhaustive `find`, search
  generations, `ObjectIndexState` slots, `ts_rs`/`specta` derives. Application-side, stay in the
  manager.
- Performing the manager's migration onto the published crate.
- `Game.db` as an index source. Ruled out by the manager's research
  `docs/research/game-db-as-precomputed-index.md`.
- Build-time performance measurement.
