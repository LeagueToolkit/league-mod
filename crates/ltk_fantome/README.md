# ltk_fantome

A Rust library for reading and writing the legacy `.fantome` archive format (renamed ZIP files) used by League of Legends mod managers before the introduction of the newer `.modpkg` format.

## Overview

This is a format crate: it knows the archive layout (`META/info.json`, `WAD/` and `RAW/` entries, license and thumbnail conventions) and nothing about mod projects. It provides:

- **Metadata types**: `FantomeInfo`, `FantomeLicense`, `FantomeLayerInfo`
- **Writing**: `FantomeWriter` writes an archive entry by entry, owning the zip flavor and entry naming conventions
- **Reading**: `FantomeReader` parses `META/info.json` and extracts `WAD/` and `RAW/` contents into caller-chosen directories, unpacking packed WADs through a `WadHashtable`

Packing a mod *project* into a Fantome archive, and importing an archive back into a project directory, live in the `ltk_mod_project` crate (its `fantome` cargo feature): its `FantomeFormat` and `FantomeImporter` backends compose the writer and reader from here.

```rust
use ltk_mod_project::fantome::FantomeFormat;
use ltk_mod_project::ProjectPacker;

// Loads mod.config.json/toml automatically from the project directory
let packer = ProjectPacker::from_dir("my-mod")?;
let file = std::fs::File::create("build/my-mod_1.0.0.fantome")?;
packer.pack(FantomeFormat::new(file))?;
```

## Integration with League Mod Toolkit

The format is also reachable through the `league-mod` CLI tool:

```bash
# Pack to Fantome format
league-mod pack --format fantome

# Pack with custom filename
league-mod pack --format fantome --file-name "my-mod.fantome"
```

Fantome stores only a project's base layer; the CLI warns when a project contains additional layers that will not be included.

## Contributing

This crate is part of the larger League Mod Toolkit project. See the main project README for contribution guidelines.

## License

Licensed under the same terms as the parent project.
