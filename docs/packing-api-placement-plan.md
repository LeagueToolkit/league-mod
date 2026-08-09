# Plan: move project packing into ltk_mod_project

Status: proposed, not started. Target: its own branch/PR (suggested name
`refactor/packing-api`), after `feat/modignore` lands - it moves code that
branch created (`IgnoreMode`, the ignore plumbing, their tests).

## Problem

Project-level orchestration lives in the format crates, and the dependency
arrows point the wrong way. `ltk_modpkg` and `ltk_fantome` should be format
crates (read/write their archive format); instead each one depends on
`ltk_mod_project` and hosts its own copy of "walk a mod project and pack it".
The two copies have also drifted into different API shapes: modpkg packing is
a builder (`ProjectPacker` + `PackOptions`), fantome packing is free
functions with no options.

## Inventory of misplaced APIs

In `ltk_modpkg` (all behind the `project` feature, `src/project/`):

- `ProjectPacker`, `PackOptions`, `IgnoreMode`, `PackError`, `PackResult`,
  `pack_from_project`, `pack_from_project_with_config` - project
  orchestration in the format crate. The headline move.
- `project::create_file_name` - a one-line wrapper around
  `ModProject::package_file_name`, which already lives in `ltk_mod_project`.
- `project::thumbnail::{load_thumbnail, ThumbnailError, MAX_THUMBNAIL_SIZE}` -
  reads the project's thumbnail file and encodes it for embedding. Project
  operation. (Distinct from `src/thumbnail.rs`, which reads a thumbnail out
  of an archive and stays.)

In `ltk_fantome` (`src/lib.rs`, `src/extractor.rs`):

- `pack_to_fantome`, `pack_to_fantome_with_ignore` and the private
  `pack_base_layer` / `pack_wad_directory` / `pack_metadata` /
  `pack_image` - the whole project-to-archive path.
- `create_file_name` - the same wrapper, duplicated per format.
- `get_unsupported_layers` - a pure `ModProject` query (filter on
  `is_base()`); even `ModProjectLayer::is_base`'s rustdoc talks about
  Fantome. Belongs on `ModProject`.
- `From` conversions between `FantomeLicense` and `ModProjectLicense` -
  conversion glue; once the dependency flips, the orphan rule forces these
  into `ltk_mod_project` anyway.
- `FantomeExtractor::extract_to` - materializes a mod *project*: maps
  `FantomeInfo` to `ModProject`, writes `mod.config.json`, places
  `LICENSE`/`README.md`/`thumbnail.webp` under project naming, lays out
  `content/base/`. `FantomeExtractResult` carries a `ModProject`, and
  `FantomeExtractError::Config` wraps `ModProjectError`. The format-level
  parts (zip reading, `META/info.json` parsing, WAD unpacking via
  hashtable) are correctly placed; the project materialization is not.
- `FantomePackError::Ignore` wraps `ltk_mod_project::ModIgnoreError` -
  moves with the packing code.

Correctly placed already (no change): `FantomeInfo` / `FantomeLicense` /
`FantomeLayerInfo` (format metadata shapes; `ltk_overlay::fantome_content`
uses exactly these), `WadHashtable`, `ModpkgBuilder` and friends,
`ModIgnore` + `ContentWalk` in `ltk_mod_project`, `FsModContent` in
`ltk_overlay`.

## Target architecture

Invert the dependencies. `ltk_mod_project` becomes the crate that operates
on projects; the format crates stop knowing projects exist.

```
before:  ltk_modpkg --(project feature)--> ltk_mod_project
         ltk_fantome -------------------> ltk_mod_project

after:   ltk_mod_project --(feature "modpkg")--> ltk_modpkg
         ltk_mod_project --(feature "fantome")-> ltk_fantome
```

`ltk_overlay` and `league-mod` already depend on all three, so the flip
creates no cycle for them. The base `ltk_mod_project` (no features) stays as
light as today - types, config, modignore - so `ltk_overlay` and ltk-manager
keep their small dependency surface.

New layout in `ltk_mod_project`:

```
src/pack.rs          PackOptions, IgnoreMode  (shared, no format deps)
src/modpkg/          feature "modpkg", dep ltk_modpkg
  packer.rs          ProjectPacker, PackError, PackResult, pack_from_project*
  thumbnail.rs       load_thumbnail, ThumbnailError, MAX_THUMBNAIL_SIZE
src/fantome/         feature "fantome", dep ltk_fantome
  pack.rs            pack_to_fantome(writer, project, root, &PackOptions)
  import.rs          import_project(reader, dest, hashtable) -> ModProject
  convert.rs         FantomeInfo <- ModProject, license From impls
```

Type names keep their current spelling where possible (`ProjectPacker`,
`PackOptions`, `FantomePackError`); only the paths change.

The guiding rule after the split: **format crates expose builders/readers,
`ltk_mod_project` decides what goes in and where it lands.** Modpkg already
has this shape (`ModpkgBuilder` in the format crate, the packer
orchestrating). Fantome gets the same: a small `FantomeWriter` in
`ltk_fantome` owning the zip flavor (compression options, `WAD/` / `META/`
entry conventions) so `zip` need not become a `ltk_mod_project` dependency,
plus format-level extraction primitives (`read_metadata` -> `FantomeInfo`,
WAD/RAW extraction into a caller-chosen directory, META entry access with
the `META/LICENSE*` canonical-name mapping). `ltk_mod_project::fantome`
composes them.

## Unifications that fall out

- Delete both `create_file_name` wrappers; callers use
  `ModProject::package_file_name` (the CLI already imports
  `ltk_mod_project`).
- Replace `get_unsupported_layers` with `ModProject::non_base_layers()` in
  core (no feature gate needed).
- Fantome packing gains `PackOptions`/`IgnoreMode` parity: `Disabled` and
  `Explicit` (with the root-mismatch guard) come for free instead of the
  current bespoke `_with_ignore` variant. One ignore story across formats.
- Optional, recommended: fold the CLI's `validate_mod_name` /
  `validate_version_format` checks into `validate_project` so GUIs get the
  same backend validation (per the project rule: never rely on frontend
  validation). The CLI keeps its pretty errors by matching `PackError`.

## What this does NOT include (follow-ups)

- Modpkg-to-project import symmetry. `ModpkgExtractor::extract_all` writes
  `output_dir/<layer>/...` with hardcoded project meta names but no
  `mod.config.json`, no `content/` prefix, and (from a read of the
  extractor) no WAD directory level - so despite its doc comment, the
  output is not something `ProjectPacker` can read back. A proper
  `ltk_mod_project::modpkg::import_project` mirroring the fantome one is a
  separate PR; this one only notes the gap.
- ltk-manager migration happens in its own repo when it bumps these deps.

## Mechanics and risks

- **No deprecation shims are possible.** Once `ltk_mod_project` depends on
  the format crates, they cannot re-export the moved items (cycle). This is
  a hard break: `feat!` on `ltk_modpkg` (module `project` removed, plus the
  already-pending removal of `PackError::InvalidGlobPattern` from
  `feat/modignore`) and `feat!` on `ltk_fantome` (packing functions,
  `extract_to`, conversions removed). `ltk_mod_project` is additive:
  `feat`.
- Everything the packer uses from `ltk_modpkg` is already public
  (`builder`, `ModpkgChunkBuilder.path` and accessors, `hash_layer_name`,
  `Utf8PathExt`/`PathBufExt`, `Slug`, metadata types, error types), so the
  modpkg move is mechanical. The fantome move is the design work (writer +
  extraction primitives).
- Tests move with their code (packer tests, fantome pack/round-trip tests,
  `project/tests.rs`). No dev-dependency from a format crate back to
  `ltk_mod_project` may remain - a dev-dep cycle would break publishing.
- Dependency deltas: `ltk_mod_project` gains optional `ltk_modpkg`,
  `ltk_fantome`, `image` (thumbnails, both features), `semver`
  (`PackError::InvalidVersion`), `slug` (fantome import name slugging).
  `ltk_fantome` likely *drops* `image` (both image users move out) and the
  now-unused `walkdir`; verify with `cargo udeps` or by grep.
- `ltk_mod_project` is edition 2021, the moving fantome code is edition
  2024; syntax is compatible but check `cargo fmt`/clippy after the move.

## Sequencing (workspace green at every commit)

1. `feat!: move modpkg project packing into ltk_mod_project` - add the
   `modpkg` feature and `pack`/`modpkg` modules, delete
   `ltk_modpkg::project`, update `league-mod` pack imports in the same
   commit.
2. `feat!: move fantome packing and project import into ltk_mod_project` -
   add `FantomeWriter` + extraction primitives to `ltk_fantome`, add the
   `fantome` feature and modules, delete the moved fantome APIs, update the
   CLI's pack/extract commands.
3. `refactor: unify packing options across formats` - `PackOptions` for
   fantome, `non_base_layers`, delete the `create_file_name` wrappers,
   (optional) backend name/version validation.
4. `docs: update crate READMEs for the new packing API` - `ltk_modpkg` and
   `ltk_fantome` READMEs show moved examples; root README crate
   descriptions; rustdoc module examples.

Verification per commit: `cargo test --workspace`, `cargo clippy
--workspace --all-targets --all-features`, `cargo fmt --check`, and a
feature-matrix build of `ltk_mod_project` (none / `modpkg` / `fantome` /
both). Before opening the PR: grep for stale `ltk_modpkg::project` and
`ltk_fantome::pack_to_fantome` references, including the wiki.
