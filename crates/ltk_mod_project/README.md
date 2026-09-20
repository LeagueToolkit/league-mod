# ltk_mod_project

The mod project layer of the [League Mod Toolkit](https://github.com/LeagueToolkit/league-mod):
the `mod.config.json` schema, the on-disk project layout, and the format-neutral drivers that
pack a project into an archive and import one back.

## Overview

A *mod project* is a directory a mod author edits: a config file, content organized by layer and
WAD target, and optional metadata files. A *package* is the single archive that directory packs
into - `.modpkg`, or the legacy `.fantome`.

This crate sits between the format crates and the tools:

```
   league-mod (CLI)          ltk-manager (desktop app)
              \                 /
            ltk_mod_project (this crate)
              /                 \
        ltk_modpkg           ltk_fantome
```

It owns:

- **Config**: `ModProject` and its nested types, loaded from and saved to JSON or TOML
- **Layout**: the `content/<layer>/<WAD>/...` and `hashes/` conventions, plus layer ordering and
  normalization
- **Filtering**: `.modignore`, gitignore semantics over `content/`
- **Packing**: `ProjectPacker`, one driver that scans, filters and validates a project into a
  `PackPlan`, and the `PackFormat` trait a backend implements
- **Importing**: `ProjectImporter`, the mirror driver, and the `ImportFormat` trait
- **Backends**: `ModpkgFormat` / `ModpkgImporter` and `FantomeFormat` / `FantomeImporter`, one
  cargo feature each

The archive formats themselves are not here. `ltk_modpkg` and `ltk_fantome` know their own bytes;
this crate knows what a project is and drives them.

## Cargo features

| Feature   | Default | Adds                                                                                       |
| --------- | ------- | ------------------------------------------------------------------------------------------ |
| *(none)*  | yes     | config types, layout constants, `.modignore`, the pack and import drivers and their traits |
| `modpkg`  | no      | `modpkg::{ModpkgFormat, ModpkgImporter, read_project}` and `modpkg::thumbnail`              |
| `fantome` | no      | `fantome::{FantomeFormat, FantomeImporter}` and the `preserve` module                       |

The features are independent; a tool that handles both formats enables both.

```toml
[dependencies]
ltk_mod_project = { version = "0.9", features = ["modpkg", "fantome"] }
```

A crate that only reads and writes `mod.config.json` - a validator, an editor, a scaffolder -
takes the default build and pulls in neither format crate nor the image codecs.

## Project layout

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
|   |   |-- Map11.wad.client
|   |   |-- raw               # files named by game asset path, routed when an overlay builds
|   |-- high_res              # an optional layer
|       |-- Aatrox.wad.client
|-- build                     # packed archives
```

- A directory directly under a layer names the WAD its contents patch. `raw/` is the exception:
  its files are named by game asset path, and the WAD each belongs to is resolved when
  `ltk_overlay` builds an overlay. Only the base layer has one.
- A layer's directory has to exist for every layer the config declares. Packing fails with
  `PackError::LayerDirMissing` otherwise.
- `hashes/` sits outside `content/`. A table is never a packing candidate and never meets
  `.modignore`. The manifest is authoritative: a file under `hashes/` that no entry declares does
  not exist for lookup.

## Configuration

```json
{
  "name": "old-summoners-rift",
  "display_name": "Old Summoners Rift",
  "version": "0.1.0-beta.5",
  "description": "Changes the map to the old Summoners Rift",
  "authors": ["TheKillerey", { "name": "Crauzer", "role": "Contributor" }],
  "license": "MIT",
  "tags": ["map-skin"],
  "maps": ["summoners-rift"],
  "thumbnail": "thumbnail.webp",
  "hashtables": [
    {
      "path": "hashes/game.hashes.txt",
      "category": "game",
      "algorithm": "xxh64",
      "bits": 64
    }
  ],
  "layers": [
    { "name": "base", "priority": 0, "description": "Base layer of the mod" },
    { "name": "chroma1", "display_name": "Chroma 1", "priority": 20 }
  ]
}
```

The same document in TOML is `mod.config.toml`. `ModProject::load` searches a directory for
`mod.config.json` first, then `mod.config.toml`; `ConfigFormat` carries the mapping between an
extension and a parser.

| Field          | Type             | Notes                                                                     |
| -------------- | ---------------- | ------------------------------------------------------------------------- |
| `name`         | string           | Directory-safe identifier: letters, digits, `_` and `-`                    |
| `display_name` | string           | Shown to a person                                                          |
| `version`      | string           | SemVer                                                                     |
| `description`  | string           |                                                                            |
| `authors`      | array            | A bare name, or `{ "name": ..., "role": ... }`                             |
| `license`      | string or object | An SPDX id, or `{ "name": ..., "url": ... }` with `url` optional           |
| `tags`         | array of string  | `WellKnownModTag::ALL` lists the recognized ones; anything else is custom  |
| `champions`    | array of string  | Champion names the mod targets                                             |
| `maps`         | array of string  | `WellKnownMap::ALL` lists the recognized ones; anything else is custom     |
| `thumbnail`    | string           | Path relative to the project root, at most 5 MB, converted to WebP on pack |
| `hashtables`   | array of object  | `path`, `category`, `algorithm`, `bits`; see `ltk_hashtable`               |
| `layers`       | array of object  | Omitted means the base layer alone                                         |
| `transformers` | array of object  | Declared in the schema; no backend applies them                            |

A layer carries `name`, an optional `display_name`, `priority`, an optional `description`, and
`string_overrides`: a locale to field-name to replacement-string map applied to `lol.stringtable`
when an overlay builds.

Higher priority wins where two layers write the same file. The base layer is named `base` and its
priority is 0; any other value fails a pack with `PackError::InvalidBaseLayerPriority`. A layer
table decoded from an archive runs through `ModProjectLayer::normalize_table`, which sorts base
first, then by ascending priority, then by name the way a person reads it - `layer9` before
`layer10`. A hand-written config is left alone, so an author's mistake is reported rather than
silently rewritten.

## Usage

### Read and write a config

```rust
use camino::Utf8Path;
use ltk_mod_project::{ConfigFormat, ModProject};

let mut project = ModProject::load(Utf8Path::new("my-mod"))?;
project.version = "1.1.0".to_owned();
project.save(Utf8Path::new("my-mod/mod.config.json"))?;

// Or render it without touching the filesystem.
let toml = project.to_config_string(ConfigFormat::Toml)?;
```

### Pack a project

`ProjectPacker` loads the config, validates the layer layout, walks `content/` once through the
project's `.modignore`, resolves the metadata files, and hands the resulting `PackPlan` to a
format backend.

```rust
use ltk_mod_project::modpkg::ModpkgFormat;
use ltk_mod_project::{PackageFormat, ProjectPacker};

let packer = ProjectPacker::from_dir("my-mod")?;
let name = packer.project().package_file_name(None, PackageFormat::Modpkg);
let file = std::fs::File::create(format!("my-mod/build/{name}"))?;

let report = packer.pack(ModpkgFormat::new(file))?;
println!("{} files ignored", report.ignored_count());
```

Fantome is the same call with the other backend. It stores only the base layer, so a caller warns
about what a pack drops:

```rust
use ltk_mod_project::fantome::FantomeFormat;
use ltk_mod_project::ProjectPacker;

let packer = ProjectPacker::from_dir("my-mod")?;
for layer in packer.project().non_base_layers() {
    eprintln!("layer {} is not stored in a Fantome archive", layer.name);
}
packer.pack(FantomeFormat::new(std::fs::File::create("my-mod.fantome")?))?;
```

`PackOptions::with_ignore` picks the filter: `IgnoreMode::FromProject` (the default),
`IgnoreMode::Disabled`, or `IgnoreMode::Explicit` with a `ModIgnore` the caller already built for
the same project root.

### Import a package

`ProjectImporter` creates the output directory and the base layer's content directory, runs the
backend, offers the decoded project to a config hook, gives every declared layer a directory, and
writes `mod.config.json`.

```rust
use ltk_mod_project::fantome::FantomeImporter;
use ltk_mod_project::{ImportStage, ProjectImporter};

let file = std::fs::File::open("my-mod.fantome")?;
let project = ProjectImporter::new("imported/my-mod")
    .with_config(|project| project.name = "my-mod".to_owned())
    .import_with_progress(FantomeImporter::new(file), &mut |progress| {
        let (done, total) = (progress.current, progress.total);
        match progress.stage {
            ImportStage::Extracting { item } => println!("{done}/{total}: {item}"),
            ImportStage::WritingMetadata => println!("writing metadata"),
            ImportStage::Complete => println!("done"),
        }
    })?;
println!("imported {}", project.name);
```

`try_with_config` is the fallible hook: returning a `ConfigRefusal` aborts the import - a name
collision only the user can resolve, say.

### Read a package's config without unpacking it

```rust
use ltk_modpkg::Modpkg;

let mut modpkg = Modpkg::mount_from_reader(std::fs::File::open("my-mod.modpkg")?)?;
let project = ltk_mod_project::modpkg::read_project(&mut modpkg)?;
println!("{} v{}", project.display_name, project.version);
```

Only the metadata chunk is decompressed. The content chunks stay where they are.

### Preflight an import

`ProjectPaths` reports every path an import writes, relative to the project directory, before one
byte lands. A `FantomeReader` and a modpkg `ExtractionPlan` both answer it, so a caller checking
the Windows path length limit reads the layout instead of restating it. A Fantome archive's answer
is incomplete for a packed WAD; `ProjectPath::is_unpacked_wad` marks those.

### Progress and cancellation

Both drivers take a progress callback (`pack_with_progress`, `import_with_progress`), and the
importer takes a `Cancellation`. A cancellation is checked between items, so it lands between
files rather than part-way through one, and fails with a cancelled error.

```rust
use std::sync::atomic::AtomicBool;
use ltk_mod_project::ProjectImporter;

let flag = AtomicBool::new(false); // flipped by whatever drives the UI
let importer = ProjectImporter::new("imported/my-mod").with_cancellation(&flag);
```

### Filter content

```rust
use camino::Utf8Path;
use ltk_mod_project::ModIgnore;

let root = Utf8Path::new("my-mod");
let ignore = ModIgnore::parse(root, "*.psd\ncache/\n!base/keep.psd\n")?;

assert!(ignore.is_ignored(Utf8Path::new("base/splash.psd"), false));
assert!(!ignore.is_ignored(Utf8Path::new("base/keep.psd"), false));
```

Ignore files cascade the way git's do: `<project_root>/.modignore` anchors its patterns at
`content/`, and any directory beneath `content/` may hold its own. Patterns match
case-insensitively on every platform. The game resolves packed paths case-insensitively, and a
case-sensitive filter would let a `thumbs.db` rule ship a `Thumbs.db`.

## Writing a format backend

`PackFormat` and `ImportFormat` are public API. A backend outside this crate reads the plan
through its accessors and is driven exactly like the built-in ones.

```rust
use ltk_mod_project::{PackFormat, PackFormatReport, PackPlan, PackReporter, ProjectPacker};

/// A toy format: counts the files a pack would write.
struct EntryCount<'a>(&'a mut usize);

impl PackFormat for EntryCount<'_> {
    type Error = std::convert::Infallible;

    fn pack(
        self,
        plan: &PackPlan<'_>,
        progress: &mut PackReporter<'_>,
    ) -> Result<PackFormatReport, Self::Error> {
        for layer in plan.layers() {
            for file in layer.files() {
                progress.report_file(file.rel_path());
                *self.0 += 1;
            }
        }
        Ok(PackFormatReport::default())
    }
}

let packer = ProjectPacker::from_dir("my-mod")?;
let mut count = 0;
packer.pack(EntryCount(&mut count))?;
```

## Errors

Driver failures and format failures stay separate. `PackError<E>` and `ImportError<E>` carry the
shared variants once, and a transparent `Format` variant carries the backend's own error, so
matching on a concrete format's failure is one level deep: `PackError::Format(inner)`.

`ModProjectError` covers config access alone: no config file in a directory, an unreadable file, a
parse failure naming the path, an unsupported extension.

## Preserving Fantome names

With the `fantome` feature, `preserve::preserve_archive_names` reads an archive, harvests the
names its entries still carry, and writes an archive that declares them. It is import-shaped -
source in, destination out - so the way a mod enters a library is the preserve, and a repair that
runs on library mods cannot run before the harvest. `HarvestReport` counts the chunks whose names
are unrecoverable; it never guesses at one.

## Related

In this workspace:

- [`ltk_modpkg`](../ltk_modpkg) - the `.modpkg` binary container
- [`ltk_fantome`](../ltk_fantome) - the legacy `.fantome` archive
- [`ltk_hashtable`](../ltk_hashtable) - hashtable manifests and lookup
- [`ltk_overlay`](../ltk_overlay) - builds the WAD overlay the game loads
- [`league-mod`](../league-mod) - the CLI, deprecated in favor of the libraries

Elsewhere in League Toolkit:

- [ltk-manager](https://github.com/LeagueToolkit/ltk-manager) - the desktop mod manager, and the
  main consumer of this crate
- [wadtools](https://github.com/LeagueToolkit/wadtools) - CLI for extracting, listing and
  comparing `.wad` archives
- [Mimir](https://github.com/LeagueToolkit/Mimir) - hash-to-path tables as compact memory-mapped
  `.hashdb` files
- [lol-meta-wiki](https://github.com/LeagueToolkit/lol-meta-wiki) - documentation and a JSON API
  for `.bin` meta classes and properties
- [awesome-league](https://github.com/LeagueToolkit/awesome-league) - a curated list of tools,
  libraries and resources for League of Legends files, assets and mods

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or
http://www.apache.org/licenses/LICENSE-2.0).
