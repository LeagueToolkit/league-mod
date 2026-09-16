---
title: "Game index: every call site the crate must serve"
labels: area:research
---

Part of the game index map.
Type: research
Status: resolved

## Question

Which functions and fields of the three existing indexes do their consumers reach for, and with
what arguments? Cover:

- `ltk_overlay::GameIndex` inside this repo (`crates/ltk_overlay`, `crates/league-mod`,
  `crates/ltk_game_data`): every use of `find_wad`, `find_wads_with_hash`, `hash_index`,
  `wad_index`, `game_fingerprint`, `subchunktoc_blocked`, `load_or_build`, `save`.
- The manager's `game_index.rs`, `game_wads.rs` (`GameArchives`, `WadCache`, `chunk_head`) and
  `object_index/` from everywhere else in `ltk-manager-core` and the Tauri shell: which calls are
  index queries (chunk table, object rows) and which are browse, search, lifecycle or IPC.

Output a table per consumer: call, what it needs from the index, and whether that need is inside
the crate's boundary as settled on the map (flat chunk table, object rows, fingerprint, cache,
resolver) or stays with the consumer.

## Answer

- A minimal crate boundary that satisfies every consumer offers: `holders(WadHash)` (every
  holder, stable archive order), `size(WadHash)`, an archive list in merge order carrying both
  the `DATA/FINAL`-relative name and the game-directory-relative path, a case-insensitive
  `archive_by_filename` that distinguishes absent from ambiguous, a per-chunk visitor the
  manager's tree fold and the object build feed from, `fingerprint()`, `load_or_build` / `save`
  / `build(game_dir)` / `build_from_archives`, and, behind the object feature,
  `ObjectIndex::build`, `declared(BinHash)`, `declares(BinHash)`, `for_each_declaration`.
- On the fence, for the surface ticket: `find_best_matching_wad(&[WadHash])` and
  `compute_content_hashes_batch(game_dir, &HashSet<WadHash>)` are derivable from the flat table
  plus archive access. `localized_global_wads()` and `subchunktoc_blocked()` are file-name folds
  over the archive list.
- In this repo the overlay is the sole consumer of `ltk_overlay::GameIndex`. `league-mod` and
  `ltk_game_data` never name it; the declaration engine's one index call is
  `find_wads_with_hash(hash).min()` in `ltk_overlay/src/builder/game_data.rs`.
- The manager never names `ltk_overlay::GameIndex`. It reaches it through `OverlayBuilder::build`
  and `analyze_single_mod`, reads `ModWadReport.game_index_fingerprint`, maps `Error::GameDir`,
  and deletes `game_index.bin` by name.
- Manager files that move: `object_index/build.rs`, the `ObjectNames` trait, the `GameArchives`
  half of `game_wads.rs`; `problems/game.rs::HeldChunks` is a third private chunk table and is
  deleted. Stays: the folded tree, `read_dir`, `search`, `find`, `stats`, state slots,
  generations, wire structs, `WadCache`, `object_index/{browse,find,search,references,spells,
  state,walk,wire}.rs`, every `src-tauri` command. Splits: `game_index.rs`, `game_wads.rs`,
  `object_index.rs` root, `object_index/names.rs`.
- Two archive identities collide at the boundary: the overlay keys by game-directory-relative
  `Utf8PathBuf` and keeps every holder; the manager keys by `DATA/FINAL`-relative name plus an
  ordinal and keeps the first holder. The crate row carries both spellings.

Full tables and the moves / stays / splits list: [findings](../research/consumer-call-sites.md).
