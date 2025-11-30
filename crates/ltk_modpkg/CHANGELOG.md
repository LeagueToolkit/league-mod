# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/LeagueToolkit/league-mod/compare/ltk_modpkg-v0.1.3...ltk_modpkg-v0.1.4) - 2025-11-30

### Added

- implement global configuration management with TOML

### Other

- update licenses across multiple crates to MIT or Apache-2.0

## [0.1.3](https://github.com/LeagueToolkit/league-mod/compare/ltk_modpkg-v0.1.2...ltk_modpkg-v0.1.3) - 2025-11-21

### Other

- update release-plz configuration and add changelogs for new crates

## [0.1.2](https://github.com/LeagueToolkit/league-mod/compare/ltk_modpkg-v0.1.1...ltk_modpkg-v0.1.2) - 2025-11-21

### Added

- add layers metadata structure and update ModpkgMetadata to include layers
- update version handling in metadata to use semver::Version
- add schema version to metadata
- add layers to metadata
- better meta handling
- add distributor info to metadata
- use metadata chunk
- *(modpkg)* msgpack metadata

### Fixed

- minor clone stuff

### Other

- update c# modpkg metadata object for consistency
- clean up builder code

## [0.1.1](https://github.com/LeagueToolkit/league-mod/compare/ltk_modpkg-v0.1.0...ltk_modpkg-v0.1.1) - 2025-09-29

### Added

- add check for update

### Other

- add quick install instructions for league-mod using PowerShell

## [0.1.0](https://github.com/LeagueToolkit/league-mod/releases/tag/ltk_modpkg-v0.1.0) - 2025-09-28

### Added

- add initial winget stuff

### Fixed

- typo

### Other

- update Cargo.toml files for pkgs with metadata
- prepare repo for crates releases
- add ci workflow
- add release-plz
- add readme

