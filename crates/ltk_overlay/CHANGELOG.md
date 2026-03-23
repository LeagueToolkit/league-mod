# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


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
