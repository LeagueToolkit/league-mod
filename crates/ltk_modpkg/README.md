# ltk_modpkg

A Rust library for reading, writing, and packing `.modpkg` archives — the binary mod distribution format for League of Legends mods in the [League Mod Toolkit](https://github.com/LeagueToolkit/league-mod).

## Overview

A `.modpkg` file is a binary container that stores mod content organized by layers and WAD targets, with per-chunk zstd compression, xxhash checksums, and embedded metadata (name, version, authors, license, thumbnail, etc.).

This crate provides:

- **Reading** — mount a modpkg from any `Read + Seek` source and access chunks by path hash
- **Writing** — build a modpkg from scratch using `ModpkgBuilder`
- **Project packing** — scan a mod project directory and produce a modpkg in one call (requires `project` feature)
- **Extraction** — extract modpkg contents back to disk
- **Metadata** — read/write msgpack-encoded mod metadata

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

### Packing a mod project (requires `project` feature)

```rust
use ltk_modpkg::project::ProjectPacker;
use camino::Utf8PathBuf;

// Loads mod.config.json/toml automatically from the project directory
let packer = ProjectPacker::new(Utf8PathBuf::from("my-mod"))?;
packer.pack("build/my-mod_1.0.0.modpkg".into())?;
```

Or pack to an in-memory buffer:

```rust
use ltk_modpkg::project::ProjectPacker;
use camino::Utf8PathBuf;

let mut buffer = std::io::Cursor::new(Vec::new());
ProjectPacker::new(Utf8PathBuf::from("my-mod"))?
    .pack_to_writer(&mut buffer)?;
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

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `project` | no | Enables `ProjectPacker` and mod project packing from disk. Adds `ltk_mod_project` dependency. |

## Project structure

The expected mod project layout (used by `ProjectPacker`):

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
