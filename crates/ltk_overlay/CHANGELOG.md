# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [0.9.4](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.9.3...ltk_overlay-v0.9.4) - 2026-08-29

### Other

- updated the following local packages: ltk_fantome, ltk_mod_project

## [0.9.3](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.9.2...ltk_overlay-v0.9.3) - 2026-08-29

### Other

- updated the following local packages: ltk_fantome, ltk_mod_project

## [0.9.2](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.9.1...ltk_overlay-v0.9.2) - 2026-08-29

### Added

- *(fantome)* normalize fantome archives
- *(overlay)* pass mod chunks through already compressed

### Other

- Merge pull request #206 from LeagueToolkit/perf/overlay-chunk-passthrough

## [0.9.1](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.9.0...ltk_overlay-v0.9.1) - 2026-08-29

### Added

- *(bench)* report peak commit and peak working set
- *(overlay)* stream override reads through a per-chunk visitor
- *(bench)* benchmark several .fantome mods at once
- *(bench)* benchmark a .fantome archive, packed or exploded

### Fixed

- overlay bench harness timer

### Other

- *(overlay)* compress overrides in bounded batches, deduped before reading
- *(overlay)* mount packed fantome WADs lazily

## [0.9.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.8.0...ltk_overlay-v0.9.0) - 2026-08-28

### Added

- add embedded hashtable support

### Fixed

- [**breaking**] harden overlay layout records and type its content hashes
- fall back to a full rebuild when a tail rewrite fails
- rebuild WADs a killed build left mid-rewrite
- write overlay.json atomically

### Other

- [**breaking**] give the overlay API matchable errors and typed path hashes
- correct the ltk_overlay README architecture and status
- rewrite the overlay builder design to match the crate
- [**breaking**] rebuild an unchanged-shape WAD by rewriting only its tail
- [**breaking**] write patched WADs as a copied source region plus an override tail
- [**breaking**] compress each override once per build

## [0.8.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.7.0...ltk_overlay-v0.8.0) - 2026-08-27

### Added

- [**breaking**] drive an archive import and let it say where it writes

## [0.7.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.6.0...ltk_overlay-v0.7.0) - 2026-08-26

### Other

- Merge pull request #194 from LeagueToolkit/feat/ltk-wad-0.5
- [**breaking**] name chunks through ltk_wad 0.5's PathResolver
- *(overlay)* read hex chunk names through ltk_wad

## [0.6.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.5.2...ltk_overlay-v0.6.0) - 2026-08-25

### Added

- [**breaking**] read RAW overrides from unpacked mod projects
- better mod archive and overlay API surface
- implement modignore file
- [**breaking**] add support for license files

### Fixed

- [**breaking**] stop hiding error causes behind category messages
- [**breaking**] store readme and license as bytes, not lossy UTF-8

### Other

- name the file in ltk_overlay IO errors
- replace stringly ltk_overlay errors with typed variants
- [**breaking**] delete dead ltk_overlay error API
- [**breaking**] validate layer names through a Slug newtype
- [**breaking**] add ChunkPath, replacing hash_chunk_name

## [0.5.2](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.5.1...ltk_overlay-v0.5.2) - 2026-07-06

### Fixed

- *(ltk_overlay)* cross-wad chunk distribution
- *(ltk_overlay)* cross-wad lazy override filtering

## [0.5.1](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.5.0...ltk_overlay-v0.5.1) - 2026-07-06

### Added

- *(ltk_overlay)* preserve original WAD signature when patching

### Fixed

- *(ltk_overlay)* respect overrides filter in all passes

## [0.5.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.4.0...ltk_overlay-v0.5.0) - 2026-07-03

### Fixed

- *(ltk_overlay)* [**breaking**] prevent serving of stale overlay WADs

## [0.4.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.3.1...ltk_overlay-v0.4.0) - 2026-07-03

### Added

- *(ltk_overlay)* [**breaking**] apply string overrides to localized stringtables

## [0.3.1](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.3.0...ltk_overlay-v0.3.1) - 2026-06-29

### Added

- *(ltk_overlay)* mmap source wad during patching
- *(ltk_overlay)* optimize buffering during wad patching

### Fixed

- *(ltk_overlay)* parallelize game index content hash computation
- *(ltk_overlay)* enforce 4 GiB limit for patched WAD
- *(ltk_overlay)* improve handling of ZIP entry reads
- *(ltk_overlay)* use highest progress value
- *(ltk_overlay)* safer rebuild
- *(ltk_overlay)* handle edge cases around WAD routing

## [0.3.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.2.9...ltk_overlay-v0.3.0) - 2026-06-25

### Added

- *(ltk_overlay)* check for missing bin dependencies

## [0.2.9](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.2.8...ltk_overlay-v0.2.9) - 2026-06-24

### Fixed

- *(ltk_overlay)* support non-conventional fantome folder casings

## [0.2.8](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.2.7...ltk_overlay-v0.2.8) - 2026-06-10

### Added

- *(ltk_overlay)* add AffectedWad struct to track mod overrides per WAD

### Fixed

- *(ltk_overlay)* bypass ZIP CRC32 check to handle bad checksums in Fantome archives

## [0.2.7](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.2.6...ltk_overlay-v0.2.7) - 2026-04-14

### Other

- updated the following local packages: ltk_mod_project, ltk_modpkg, ltk_fantome

## [0.2.6](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.2.5...ltk_overlay-v0.2.6) - 2026-04-14

### Added

- *(ltk_mod_project, ltk_fantome, ltk_modpkg, ltk_overlay)* add display_name to mod project layer struct

## [0.2.5](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.2.4...ltk_overlay-v0.2.5) - 2026-04-08

### Added

- *(ltk_overlay)* add per mod wad report stuff

### Other

- Merge pull request #148 from LeagueToolkit/feat/per-mod-wad-reports

## [0.2.4](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.2.3...ltk_overlay-v0.2.4) - 2026-03-28

### Added

- *(ltk_modpkg)* optimize modpkg chunk processing

### Other

- *(ltk_fantome, ltk_overlay)* ltk_wad as workspace dependency

## [0.2.3](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.2.2...ltk_overlay-v0.2.3) - 2026-03-23

### Fixed

- *(ltk_overlay)* remove scripts wad from always blocklist

## [0.2.2](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.2.1...ltk_overlay-v0.2.2) - 2026-03-21

### Fixed

- *(ltk_overlay)* use refs when counting wad overlaps
- use overlap detection fallback for unknown WAD names

### Other

- *(ltk_overlay)* add tests for overlap matching

## [0.2.1](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.2.0...ltk_overlay-v0.2.1) - 2026-03-18

### Fixed

- *(ltk_modpkg)* normalize backslashes to forward slashes in chunk path handling

### Other

- *(ltk_modpkg)* add tests for path normalization and backslash handling
- *(ltk_modpkg)* introduce normalize_chunk_path utility for consistent path handling

## [0.2.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.1.4...ltk_overlay-v0.2.0) - 2026-03-18

### Added

- *(ltk_overlay)* add support for layer filtering in mod metadata collection

### Fixed

- *(ltk_overlay)* exclude BASE_LAYER_NAME from layer fingerprinting to ensure consistent hashing

### Other

- *(ltk_overlay)* enhance documentation for ModContentProvider trait regarding thread safety and read-only operations

## [0.1.4](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.1.3...ltk_overlay-v0.1.4) - 2026-03-13

### Added

- *(ltk_overlay)* introduce FantomeIndex for efficient archive content lookups
- *(ltk_overlay)* add content fingerprinting for archive metadata caching
- *(ltk_overlay)* implement mod meta cache and archive content providers

### Fixed

- *(ltk_overlay)* cache packed wad files during content provider creation
- *(ltk_overlay)* improve error handling for override meta cache deserialization

### Other

- *(ltk_overlay)* optimize file retrieval in FantomeContent by replacing index-based lookups with direct name-based access

## [0.1.3](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.1.2...ltk_overlay-v0.1.3) - 2026-03-12

### Added

- *(ltk_overlay)* support fantome raw folder fully

## [0.1.2](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.1.1...ltk_overlay-v0.1.2) - 2026-02-21

### Added

- *(ltk-mod-project)* add support for tags, champions, and maps in mod project configuration

## [0.1.1](https://github.com/LeagueToolkit/league-mod/compare/ltk_overlay-v0.1.0...ltk_overlay-v0.1.1) - 2026-02-21

### Fixed

- *(ltk-overlay)* route overrides for unknown wad files

## [0.1.0](https://github.com/LeagueToolkit/league-mod/releases/tag/ltk_overlay-v0.1.0) - 2026-02-18

### Added

- *(ltk-overlay)* integrate rmp-serde for MessagePack serialization
- *(ltk-overlay)* add state_dir to OverlayBuilder for improved file management
- *(ltk-manager)* add wad blocklist for scripts and tft wads
- *(ltk-overlay)* optimize WAD override processing
- *(ltk-overlay)* implement parallel processing for WAD patching
- *(ltk_overlay)* detect and skip lazy mod overrides via content hashing
- *(ltk_overlay)* implement incremental overlay rebuild
- *(ltk-manager)* implement overlay invalidation after mod operations
- overlay optimizations
- *(ltk_overlay)* use camino for paths
- implement mod content providers for Fantome and Modpkg archives
- start using overlay crate
- add ltk_overlay crate for WAD overlay/profile building

### Fixed

- *(ltk-manager)* non-blocking patcher stop and overlay log visibility
- *(ltk_overlay)* handle non-UTF-8 paths gracefully with warnings
- *(ltk-manager)* patcher threading and overlay wad building

### Other

- remove comments
- documentation for overlay builder and mod content provider
