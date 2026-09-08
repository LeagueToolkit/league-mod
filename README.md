# League Mod Toolkit

[![CI](https://github.com/LeagueToolkit/league-mod/actions/workflows/ci.yml/badge.svg)](https://github.com/LeagueToolkit/league-mod/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](#license)

Rust libraries for creating, packaging, and installing League of Legends mods - the code behind
the `.modpkg` format and the overlay the game loads.

This workspace is the toolkit layer, not the app. [LTK Manager](https://github.com/LeagueToolkit/ltk-manager)
is where an end user clicks buttons; everything it does to a mod happens in the crates here.

## The pipeline

```
   author's project        distributable            user's library         what the game loads
   my-mod/           ->    my-mod_1.0.0.modpkg ->   profile/mods/     ->   profile/overlay/
   mod.config.json         (or .fantome)            + mod.config.json      *.wad.client

   ltk_mod_project         ltk_modpkg               ltk_mod_project        ltk_overlay
   (pack)                  ltk_fantome              (import)               (build)
```

A mod project is a directory of loose files organized by layer and WAD target. Packing turns it
into one archive. Installing imports that archive back into a project directory in the user's
library. Enabling a mod rebuilds an overlay: copies of the game's WADs with the mod's chunks
written into them, which a patcher loads in place of the originals.

## Crates

| Crate                                          | Version                                                                                                       | What it is                                                                                  |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| [`ltk_mod_project`](crates/ltk_mod_project)    | [![crates.io](https://img.shields.io/crates/v/ltk_mod_project.svg)](https://crates.io/crates/ltk_mod_project)  | The `mod.config.json` schema, the project layout, and the pack and import drivers            |
| [`ltk_modpkg`](crates/ltk_modpkg)              | [![crates.io](https://img.shields.io/crates/v/ltk_modpkg.svg)](https://crates.io/crates/ltk_modpkg)            | The `.modpkg` binary container: read, write, extract                                          |
| [`ltk_fantome`](crates/ltk_fantome)            | [![crates.io](https://img.shields.io/crates/v/ltk_fantome.svg)](https://crates.io/crates/ltk_fantome)          | The legacy `.fantome` archive: read, write, rewrite in place                                  |
| [`ltk_overlay`](crates/ltk_overlay)            | [![crates.io](https://img.shields.io/crates/v/ltk_overlay.svg)](https://crates.io/crates/ltk_overlay)          | Builds the WAD overlay from the enabled mods, incrementally                                   |
| [`ltk_hashtable`](crates/ltk_hashtable)        | [![crates.io](https://img.shields.io/crates/v/ltk_hashtable.svg)](https://crates.io/crates/ltk_hashtable)      | The hashtables a mod embeds: file grammar, keys, merging, collision detection                 |
| [`ltk_mod_core`](crates/ltk_mod_core)          | [![crates.io](https://img.shields.io/crates/v/ltk_mod_core.svg)](https://crates.io/crates/ltk_mod_core)        | League installation detection and cross-platform path helpers                                 |
| [`league-mod`](crates/league-mod)              | -                                                                                                              | The CLI for mod authors. Deprecated; distributed through GitHub Releases rather than crates.io |

Each crate's README is the reference for its own surface. `ltk_mod_project`'s covers the config
schema and the project layout in full.

## For mod authors

A mod project is a directory:

```
my-mod
|-- mod.config.json           # or mod.config.toml
|-- README.md                 # optional, embedded in the package
|-- LICENSE                   # optional; LICENSE.md and LICENSE.txt also recognized
|-- thumbnail.webp            # optional, named by the config's thumbnail field
|-- .modignore                # optional, patterns anchored at content/
|-- hashes
|   |-- game.hashes.txt       # declared by the config's hashtables manifest
|-- content
|   |-- base                  # the base layer, priority 0, always present
|   |   |-- Aatrox.wad.client # one directory per WAD target
|   |   |   |-- assets
|   |   |   |-- data
|   |   |-- raw               # files named by game asset path, routed when an overlay builds
|   |-- high_res              # an optional layer
|       |-- Aatrox.wad.client
|-- build                     # packed archives
```

```json
{
  "name": "aatrox-rework",
  "display_name": "Aatrox Visual Rework",
  "version": "1.0.0",
  "description": "A complete visual overhaul for Aatrox",
  "authors": ["Your Name"],
  "license": "MIT",
  "tags": ["champion-skin"],
  "champions": ["Aatrox"],
  "layers": [
    { "name": "base", "priority": 0, "description": "Core modifications" },
    { "name": "high_res", "priority": 10, "description": "High resolution textures" }
  ]
}
```

Full field reference: [`ltk_mod_project`'s README](crates/ltk_mod_project/README.md#configuration).
Guides and walkthroughs: [wiki.leaguetoolkit.dev](https://wiki.leaguetoolkit.dev/making-mods/mod-projects/).

### Layers

A layer is a named, prioritized set of overrides. Every project has `base` at priority 0. Where
two layers write the same file, the higher priority wins. A manager can enable a subset of a
mod's layers, so one package ships a base skin plus optional chromas, high-res textures, or sound
replacements.

### Ignoring files

`.modignore` at the project root lists gitignore-style patterns for files under `content/` that
are not part of the mod - `.psd` sources, scratch directories, OS junk. Comments (`#`), negation
(`!`), directory-only patterns (`cache/`) and last-match-wins all behave as git's do. Any
directory under `content/` may hold its own `.modignore` governing its subtree. Matching is
case-insensitive on every platform, and the same filter runs when a live overlay builds, so what
you test is what you ship.

Two traps:

- Always write `/` in a pattern, even on Windows. A backslash is gitignore's escape character, so
  `base\scratch` matches nothing you meant.
- To keep one file inside an otherwise excluded folder, ignore the folder's *contents*:
  `scratch/*` then `!scratch/keep.bin`. `scratch/` alone cannot work - nothing under an excluded
  directory is re-includable.

### Licensing a mod

The `license` field names the terms; a `LICENSE` file carries the text. They are independent, and
either, both or neither may be present. `.modpkg` stores the text in a compressed `_meta_/license`
chunk, `.fantome` in a `META/LICENSE` entry keeping the source extension. Extraction writes the
file back under the name it was packed with.

### The CLI

> **Deprecated.** `league-mod` receives no new features. The library crates are the supported way
> to build mod tooling, and [LTK Manager](https://github.com/LeagueToolkit/ltk-manager) is the
> supported way to author and install mods.

Windows, no admin:

```powershell
irm https://raw.githubusercontent.com/LeagueToolkit/league-mod/main/scripts/install-league-mod.ps1 | iex
```

The script installs the latest release to `%LOCALAPPDATA%\LeagueToolkit\league-mod` and adds it to
your user `PATH`. The binaries are also on the [releases page](https://github.com/LeagueToolkit/league-mod/releases).

```bash
league-mod init                          # scaffold a project, interactively
league-mod pack                          # -> build/my-mod_1.0.0.modpkg
league-mod pack --format fantome         # legacy container, base layer only
league-mod info my-mod_1.0.0.modpkg      # metadata, layers, chunk counts
league-mod extract my-mod_1.0.0.modpkg   # back to a project directory
league-mod config auto-detect            # find the League installation
```

## For tool developers

```toml
[dependencies]
ltk_mod_project = { version = "0.9", features = ["modpkg", "fantome"] }
ltk_overlay = "0.9"
```

Pack a project:

```rust
use ltk_mod_project::modpkg::ModpkgFormat;
use ltk_mod_project::ProjectPacker;

let packer = ProjectPacker::from_dir("my-mod")?;
let file = std::fs::File::create("build/my-mod_1.0.0.modpkg")?;
let report = packer.pack(ModpkgFormat::new(file))?;
```

Import a package into a library:

```rust
use ltk_mod_project::modpkg::ModpkgImporter;
use ltk_mod_project::ProjectImporter;

let reader = std::fs::File::open("my-mod.modpkg")?;
let project = ProjectImporter::new("library/my-mod").import(ModpkgImporter::new(reader))?;
```

Build the overlay from the enabled mods:

```rust
use camino::Utf8PathBuf;
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuilder};

let game_dir = Utf8PathBuf::from("C:/Riot Games/League of Legends/Game");
let profile_dir = Utf8PathBuf::from("profiles/default");

let mut builder = OverlayBuilder::new(game_dir, profile_dir.join("overlay"), profile_dir.clone());
builder.set_enabled_mods(vec![EnabledMod {
    id: "my-mod".to_owned(),
    content: Box::new(FsModContent::new(Utf8PathBuf::from("library/my-mod"))),
    enabled_layers: None,
}]);
let result = builder.build()?;
```

A rebuild touches only the WADs whose contents changed, and rewrites those by appending the mod's
bytes rather than recopying the WAD. `docs/overlay-builder-design.md` covers the file layout, the
trust rules behind that fast path, and the state files.

## Formats

**`.modpkg`** is the toolkit's own container: a binary file storing chunks by path hash, with
per-chunk zstd compression, xxhash checksums, and layers and WAD targets in the header. Msgpack
metadata carries the name, version, authors, license, tags, per-layer string overrides and the
hashtable manifest. The readme, license text, thumbnail and hashtable files are chunks of their
own under `_meta_/`, so reading the metadata never decompresses them.

**`.fantome`** is the legacy format - a renamed ZIP with `META/info.json`, `WAD/` and `RAW/`
entries. Reading and writing it is supported for compatibility with the existing mod ecosystem. It
carries only a project's base layer, and a pack to Fantome warns about the layers it drops.

## Building from source

Stable Rust, 2021 edition. No pinned MSRV; CI builds on the current stable toolchain.

```bash
git clone https://github.com/LeagueToolkit/league-mod.git
cd league-mod
cargo build --release        # binary at target/release/league-mod
cargo test                   # whole workspace
cargo test -p ltk_modpkg     # one crate
cargo clippy --all-targets
cargo fmt
```

## Documentation

- [wiki.leaguetoolkit.dev](https://wiki.leaguetoolkit.dev) - guides for mod authors
- Per-crate READMEs, linked from the table above
- `docs/adr/` - architectural decision records, one per decision with real alternatives
- `docs/overlay-builder-design.md` - the overlay build, its incremental path and its state files
- `docs/modignore-behavior-notes.md` - `.modignore` edge cases

## Contributing

Issues and pull requests are welcome. For a large change, open an issue first.

Commits follow [Conventional Commits](https://www.conventionalcommits.org/); release-plz reads the
type and the `!` marker to pick each crate's version bump. The scope is the crate without its
`ltk_` prefix (`modpkg`, `fantome`, `overlay`), `cli` for `league-mod`, or the area (`docs`, `ci`,
`workspace`) for work outside one.

```bash
git commit -m "feat(overlay): pass through container chunks"
git commit -m "fix(modpkg): reject chunk path with a drive prefix"
git commit -m "feat(project)!: require a layer directory per declared layer"
```

Every PR runs: build and test on Linux, Windows and macOS; clippy; `cargo fmt --check`; a package
check that every crate publishes as it will on crates.io; and `cargo deny` for advisories,
licenses and duplicate dependencies.

Releases are automated. Push a conventional commit to `main`, release-plz opens a release PR with
the version bumps and changelogs, and merging it publishes the crates and the Windows binaries.

## Related

- [ltk-manager](https://github.com/LeagueToolkit/ltk-manager) - the desktop mod manager, and the
  main consumer of these crates
- [wadtools](https://github.com/LeagueToolkit/wadtools) - CLI for extracting, listing and
  comparing `.wad` archives
- [Mimir](https://github.com/LeagueToolkit/Mimir) - hash-to-path tables as compact memory-mapped
  `.hashdb` files
- [lol-meta-wiki](https://github.com/LeagueToolkit/lol-meta-wiki) - documentation and a JSON API
  for `.bin` meta classes and properties
- [awesome-league](https://github.com/LeagueToolkit/awesome-league) - a curated list of tools,
  libraries and resources for League of Legends files, assets and mods

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
http://www.apache.org/licenses/LICENSE-2.0).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
this work by you, as defined in the Apache-2.0 license, shall be licensed as above, without any
additional terms or conditions.
