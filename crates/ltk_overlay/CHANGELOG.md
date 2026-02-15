# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [0.1.0](https://github.com/LeagueToolkit/league-mod/releases/tag/ltk_overlay-v0.1.0) - 2026-02-15

### Added

- *(ltk-manager)* implement overlay invalidation after mod operations
- overlay optimizations
- *(ltk_overlay)* use camino for paths
- implement mod content providers for Fantome and Modpkg archives
- start using overlay crate
- add ltk_overlay crate for WAD overlay/profile building

### Fixed

- *(ltk_overlay)* handle non-UTF-8 paths gracefully with warnings
- *(ltk-manager)* patcher threading and overlay wad building

### Other

- documentation for overlay builder and mod content provider
