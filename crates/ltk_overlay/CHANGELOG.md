# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


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
