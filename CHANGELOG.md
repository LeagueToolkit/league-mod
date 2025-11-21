# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/LeagueToolkit/league-mod/releases/tag/ltk_pki-v0.1.0) - 2025-11-21

### Added

- initial PKI implementation
- add check for update
- add initial winget stuff

### Fixed

- typo

### Other

- add quick install instructions for league-mod using PowerShell
- add ci workflow
- add release-plz
- add readme

## [0.2.0](https://github.com/LeagueToolkit/league-mod/releases/tag/league-mod-v0.2.0) - 2025-11-21

### Added

- update version handling in metadata to use semver::Version
- add layers to metadata
- better meta handling
- use metadata chunk
- add support for signing mod packages (argument only)
- add check for update
- add color styling to clap output
- improve cli command and use miette
- add initial winget stuff
- add support for packing to fantome
- add option to specify thumbnail in mod project config

### Fixed

- minor clone stuff
- convert version to string format for consistent display in info_mod_package
- pack readme and thumbnail into modpkg
- fmt
- pad println output
- skip base layer conditionally
- layer presence lookup
- base skip
- error if explicit base layer
- typo

### Other

- include schema version when building metadata
- mark 'sign' field as dead code in PackModProjectArgs
- release
- bump version to 0.2.0
- add quick install instructions for league-mod using PowerShell
- bump league-mod version to 0.1.1
- prepare repo for crates releases
- remove comments
- fix checks
- fix deny licenses
- add ci workflow
- add release-plz
- add readme
- move existing mod crates

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

## [0.2.0](https://github.com/LeagueToolkit/league-mod/releases/tag/league-mod-v0.2.0) - 2025-09-29

### Added

- add check for update
- add color styling to clap output
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

- bump version to 0.2.0
- add quick install instructions for league-mod using PowerShell
- bump league-mod version to 0.1.1
- prepare repo for crates releases
- remove comments
- fix checks
- fix deny licenses
- add ci workflow
- add release-plz
- add readme
- move existing mod crates

## [0.1.1](https://github.com/LeagueToolkit/league-mod/compare/ltk_modpkg-v0.1.0...ltk_modpkg-v0.1.1) - 2025-09-29

### Added

- add check for update

### Other

- add quick install instructions for league-mod using PowerShell

## [0.1.1](https://github.com/LeagueToolkit/league-mod/compare/ltk_fantome-v0.1.0...ltk_fantome-v0.1.1) - 2025-09-29

### Added

- add check for update

### Other

- add quick install instructions for league-mod using PowerShell

## [0.1.1](https://github.com/LeagueToolkit/league-mod/compare/ltk_mod_project-v0.1.0...ltk_mod_project-v0.1.1) - 2025-09-29

### Added

- add check for update

### Other

- add quick install instructions for league-mod using PowerShell

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

## [0.1.0](https://github.com/LeagueToolkit/league-mod/releases/tag/ltk_fantome-v0.1.0) - 2025-09-28

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

## [0.1.0](https://github.com/LeagueToolkit/league-mod/releases/tag/ltk_mod_project-v0.1.0) - 2025-09-28

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

## [0.1.0](https://github.com/LeagueToolkit/league-mod/releases/tag/ltk_modpkg-v0.1.0) - 2025-09-28

### Other

- prepare repo for crates releases

## [0.1.0](https://github.com/LeagueToolkit/league-mod/releases/tag/ltk_fantome-v0.1.0) - 2025-09-28

### Other

- prepare repo for crates releases

## [0.1.0](https://github.com/LeagueToolkit/league-mod/releases/tag/ltk_mod_project-v0.1.0) - 2025-09-28

### Other

- prepare repo for crates releases

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
