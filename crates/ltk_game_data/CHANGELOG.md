# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [0.7.2](https://github.com/LeagueToolkit/league-mod/compare/ltk_game_data-v0.7.1...ltk_game_data-v0.7.2) - 2026-09-25

### Added

- *(game_data)* load empty entries module

## [0.7.1](https://github.com/LeagueToolkit/league-mod/compare/ltk_game_data-v0.7.0...ltk_game_data-v0.7.1) - 2026-09-24

### Added

- *(game_data)* add schema fallback shape

## [0.7.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_game_data-v0.6.0...ltk_game_data-v0.7.0) - 2026-09-24

### Added

- *(game_data)* [**breaking**] support module names

### Other

- Merge pull request #276 from LeagueToolkit/feat/module-names

## [0.6.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_game_data-v0.5.0...ltk_game_data-v0.6.0) - 2026-09-23

### Added

- *(game_data)* [**breaking**] create and remove objects

### Fixed

- *(game_data)* [**breaking**] resolve hash-form field names

### Other

- Merge pull request #272 from LeagueToolkit/release-plz-2026-09-23T06-57-00Z

## [0.5.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_game_data-v0.4.0...ltk_game_data-v0.5.0) - 2026-09-23

### Added

- *(game_data)* [**breaking**] spell struct pins as struct tags

## [0.4.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_game_data-v0.3.0...ltk_game_data-v0.4.0) - 2026-09-21

### Added

- *(game_data)* render a bin value and write it as YAML
- *(game_data)* [**breaking**] a value can reference the installed game's copy

### Fixed

- *(game_data)* read a struct pin on an option of structs
- *(game_data)* [**breaking**] refuse a pinned reference and report an entry that cannot be read

### Other

- *(deps)* bump ltk_meta to 0.8.4
- *(overlay)* decode a referenced entry with ltk_meta
- *(game_data)* cover references in every format, position, and failure

## [0.3.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_game_data-v0.2.0...ltk_game_data-v0.3.0) - 2026-09-20

### Fixed

- *(game_data)* [**breaking**] report whether an application changed anything
- *(game_data)* [**breaking**] refuse a binding keyword where an entry name is built
- *(game_data)* attest a pinned class by the base value
- *(game_data)* [**breaking**] refuse declarations the document form cannot carry
- *(game_data)* [**breaking**] group edits by property and refuse unattested classes
- *(game_data)* close discovery, span and identifier gaps

### Other

- rewrite the new comments in simplified technical english
- *(overlay)* give the entry fan-out a name
- *(game_data)* one binding-keyword table

## [0.2.0](https://github.com/LeagueToolkit/league-mod/compare/ltk_game_data-v0.1.0...ltk_game_data-v0.2.0) - 2026-09-16

### Added

- *(game_data)* [**breaking**] apply property edits
- *(game_data)* [**breaking**] add entry bodies
- *(game_data)* [**breaking**] add overrides binding
- *(game_data)* [**breaking**] replace module target with selector

### Fixed

- *(game_data)* uniform type pins
- *(game_data)* settle property edit review findings

### Other

- *(game_data)* [**breaking**] code errors
- *(workspace)* unify toml
- *(workspace)* share ltk and common dependencies
