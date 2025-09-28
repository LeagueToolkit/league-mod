# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/LeagueToolkit/league-mod/releases/tag/league-mod-v0.1.1) - 2025-09-28

### Added

- improve cli command and use miette
- add initial winget stuff
- add support for packing to fantome
- add option to specify thumbnail in mod project config

### Fixed

- pack readme and thumbnail into modpkg
- fmt
- pad println output
- skip base layer conditionally
- layer presence lookup
- base skip
- error if explicit base layer
- typo

### Other

- bump league-mod version to 0.1.1
- prepare repo for crates releases
- remove comments
- fix checks
- fix deny licenses
- add ci workflow
- add release-plz
- add readme
- move existing mod crates

## [0.1.0](https://github.com/LeagueToolkit/league-mod/releases/tag/v0.1.0) - 2025-09-21

### Added

- improve cli command and use miette
- add initial winget stuff
- add support for packing to fantome
- add option to specify thumbnail in mod project config

### Fixed

- pack readme and thumbnail into modpkg
- fmt
- pad println output
- skip base layer conditionally
- layer presence lookup
- base skip
- error if explicit base layer
- typo

### Other

- remove comments
- fix checks
- fix deny licenses
- add ci workflow
- add release-plz
- add readme
- move existing mod crates

### Added
- Initial release of League Mod toolkit
- Project initialization and management with interactive prompts
- Mod packaging into distributable `.modpkg` files  
- Mod extraction for inspection and modification
- Detailed mod package information display
- Layer-based mod organization with priority system
- File transformation system for asset processing
- Cross-format configuration support (JSON and TOML)
- Comprehensive CLI interface for mod developers
- Support for multiple mod layers with override behavior
- Rich metadata management including authors and licenses
- Windows distribution via winget package manager
- Automated release pipeline with changelog generation

### Changed
- N/A - Initial release

### Deprecated
- N/A - Initial release

### Removed
- N/A - Initial release

### Fixed
- N/A - Initial release

### Security
- N/A - Initial release
