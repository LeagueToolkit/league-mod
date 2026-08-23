# ltk_modpkg

A Rust library for reading, writing, and packing `.modpkg` archives, the binary mod distribution format for League of Legends mods in the [League Mod Toolkit](https://github.com/LeagueToolkit/league-mod).

## Overview

A `.modpkg` file is a binary container that stores mod content organized by layers and WAD targets, with per-chunk zstd compression, xxhash checksums, and embedded metadata (name, version, authors, license, thumbnail, etc.).

This crate provides:

- **Reading**: mount a modpkg from any `Read + Seek` source and access chunks by path hash
- **Writing**: build a modpkg from scratch using `ModpkgBuilder`
- **Extraction**: extract modpkg contents back to disk
- **Metadata**: read/write msgpack-encoded mod metadata

Packing a mod *project* directory into a modpkg lives in the `ltk_mod_project`
crate (its `modpkg` cargo feature), whose `ModpkgFormat` backend drives
`ModpkgBuilder` from here.

## Usage

### Reading a modpkg

```rust
use ltk_modpkg::Modpkg;
use std::fs::File;

let file = File::open("my-mod_1.0.0.modpkg")?;
let mut modpkg = Modpkg::mount_from_reader(file)?;

// Read metadata
let metadata = modpkg.load_metadata()?;
println!("{} v{}", metadata.name, metadata.version);

// List WADs
for wad_name in modpkg.wads.values() {
    println!("WAD: {wad_name}");
}
```

### Packing a mod project

Project packing lives in `ltk_mod_project` behind its `modpkg` feature. The
format-neutral `ProjectPacker` scans the project; `ModpkgFormat` encodes it:

```rust
use ltk_mod_project::modpkg::ModpkgFormat;
use ltk_mod_project::ProjectPacker;

// Loads mod.config.json/toml automatically from the project directory
let packer = ProjectPacker::from_dir("my-mod")?;
let file = std::fs::File::create("build/my-mod_1.0.0.modpkg")?;
packer.pack(ModpkgFormat::new(file))?;
```

### Building a modpkg programmatically

```rust
use ltk_modpkg::builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder};
use ltk_modpkg::ModpkgCompression;

let builder = ModpkgBuilder::default()
    .with_layer(ModpkgLayerBuilder::base())
    .with_chunk(
        ModpkgChunkBuilder::new()
            .with_path("data/characters/graves/skin0.bin")
            .unwrap()
            .with_compression(ModpkgCompression::Zstd)
            .with_layer("base")
            .with_wad("Graves.wad.client"),
    );

let mut output = std::fs::File::create("out.modpkg")?;
builder.build_to_writer(&mut output, |chunk, cursor| {
    // provide raw chunk data here
    Ok(())
})?;
```

## Project structure

The expected mod project layout (used by `ltk_mod_project`'s `ProjectPacker`):

```
my-mod/
├── mod.config.json             # or mod.config.toml
├── README.md                   # optional, embedded in modpkg
├── thumbnail.webp              # optional, embedded in modpkg
├── content/
│   ├── base/                   # base layer (priority 0)
│   │   ├── Graves.wad.client/  # WAD target directory
│   │   │   ├── data/
│   │   │   └── assets/
│   │   └── Map11.wad.client/
│   │       └── data/
│   └── high-res/               # additional layer
│       └── Graves.wad.client/
│           └── assets/
└── build/                      # output directory
```

## License

MIT OR Apache-2.0
