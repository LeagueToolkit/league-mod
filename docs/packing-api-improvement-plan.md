# Improvement plan for the packing API

This plan lists API changes for the `ltk_mod_project` crate on the `refactor/packing-api` branch (PR #189).
The findings come from a review of the `pack`, `modpkg`, and `fantome` modules, and of the CLI code in `league-mod`.
The PR is marked `feat!`. Thus a breaking change is acceptable in this cycle.

## Order of work

1. Do item 2 and item 3 first. They are small and safe.
2. Do item 1 next. It is the largest improvement, and it changes `PackError`.
3. Do item 4 after item 1, because item 1 supplies the data for it.
4. Do item 5 at any time. It is independent of the other items.
5. Do item 6 in a separate PR. It is a new feature, not polish.

## Item 1 - Split `ProjectPacker::pack` into `plan()` and `pack()`

**Problem.** A caller cannot see what a pack will do before the pack writes the archive.
The module docs use a toy `EntryCount` format only to count files. That is a workaround for a missing API.

**Changes.**

1. Add `ProjectPacker::plan(&self) -> Result<PackPlan<'_>, PlanError>`.
2. Add a non-generic `PlanError` enum. Move the driver variants of `PackError` into it: `Scan`, `NonUtf8Path`, `LayerDirMissing`, `InvalidBaseLayerPriority`, `Ignore`, `Walk`, and `IgnoreRootMismatch`.
3. Reduce `PackError<E>` to two variants: a transparent `Plan(#[from] PlanError)` and `Format(E)`.
4. Move the list of ignored entries into `PackPlan`. Then `plan()` and `PackReport` can both supply it.
5. Keep `pack()` as the usual path. It calls `plan()`, then it calls the format.

**Results.**

- The CLI can show the file count of each layer, and the ignored entries, before it writes the archive. This is a dry run.
- A caller can pack one plan into a modpkg archive and a Fantome archive with one walk of `content/`, not two.
- The CLI error mapping becomes simpler. It matches `PlanError` once, not one `PackError<E>` for each format.

**Breaking?** Yes. The driver variants of `PackError` move one level down.
The `Display` output does not change, because the `Plan` variant is transparent.

## Item 2 - Convert the Fantome conversion functions to `From` impls

**Problem.** `fantome/convert.rs` keeps two free conversion functions.
The modpkg module replaced the same shape of code with `From` and `TryFrom` impls on this branch. The two modules must agree.

**Changes.**

1. Replace `fantome_info_from_project` with `impl From<&ModProject> for FantomeInfo`.
2. Replace `project_from_fantome_info` with `impl From<FantomeInfo> for ModProject`.
3. Keep the doc text about the slug and the layer reset on the new impl.
4. Remove the two functions from the `pub use` list in `fantome/mod.rs`.

Both functions are infallible. Thus `From` is the correct trait, not `TryFrom`.

**Breaking?** Yes. `fantome/mod.rs` exports both functions today.

## Item 3 - Add `ProjectPacker::from_config_file`

**Problem.** The CLI loads a config file, then it calls `ProjectPacker::new(project, config_path.parent().unwrap())`.
The parent computation and the `unwrap` belong in the library.

**Changes.**

1. Add `ProjectPacker::from_config_file(path) -> Result<Self, ModProjectError>` as the sibling of `from_dir`.
2. The constructor loads the config from the file. It uses the parent directory of the file as the project root.
3. Change the CLI pack command to use the new constructor.

**Breaking?** No. This is an addition.

## Item 4 - Show the file count of each layer in the CLI

**Problem.** `PackReport` only carries the ignored entries.
The CLI prints its "Building layer" lines from the config, with no file counts.

**Changes.**

- After item 1, change the CLI to read the counts from the plan. No new report API is necessary.
- Without item 1, add `packed_count()` and a count for each layer to `PackReport` instead.

**Breaking?** No.

## Item 5 - Add file constructors to the formats

**Problem.** The two CLI pack paths repeat the same code: `File::create`, then `BufWriter::new`, then `Format::new`.

**Changes.**

1. Add `ModpkgFormat::create(path) -> io::Result<ModpkgFormat<BufWriter<File>>>`.
2. Add the same constructor to `FantomeFormat`.
3. Change the CLI to use the new constructors.

**Breaking?** No.

## Item 6 - Add a `ModpkgImporter` (separate PR)

**Gap.** `ImportFormat` is a public trait, but only Fantome implements it.
For a `.modpkg` file, the extract command of the CLI uses `ModpkgExtractor::extract_all`. That call writes raw chunks and no `mod.config.json`.

**Change.** Add a `ModpkgImporter` that implements `ImportFormat`.
It must materialize a `.modpkg` archive as a project directory with a config file.
This closes the pack-import roundtrip for the native format.

**Scope.** This is a feature with its own design work. Put it in its own PR.

## Doc note - pack to memory

A pack to memory is possible today, without an API change, because `Write` and `Seek` have impls for `&mut W`.
`ModpkgFormat::new(&mut cursor)` lets the caller keep the buffer.
Add an example to the docs to make this visible.

## Acceptance for each item

1. `cargo fmt` shows no changes.
2. `cargo clippy --workspace --all-targets` reports no warnings.
3. `cargo test -p ltk_mod_project --all-features` passes.
4. `cargo doc --no-deps -p ltk_mod_project --all-features` builds without warnings.
