# .modignore behavior notes

Design decisions, edge cases, and deliberate deviations of the `.modignore`
system. Audience: maintainers, and source material for the wiki page. Items
marked *(pinned)* have a test asserting the behavior.

## Behavior mod authors must know (wiki material)

- **Always use `/` in patterns.** In gitignore syntax a backslash escapes
  the next character; it is not a path separator. A Windows author writing
  `base\scratch` gets a pattern for a file literally named `basescratch`,
  which matches nothing they meant. *(pinned:
  `backslash_in_a_pattern_is_an_escape_not_a_separator`)*
- **Keeping one file inside an ignored folder** requires ignoring the
  *contents*, not the folder: `scratch/*` plus `!scratch/keep.bin` works;
  `scratch/` plus `!scratch/keep.bin` does not, because a negation can
  never re-include anything under an excluded directory (git parity).
  *(pinned: `negation_cannot_reinclude_under_an_excluded_directory`)*
- **Matching is case-insensitive** on every platform, deviating from git,
  because the game resolves packed paths case-insensitively. `thumbs.db`
  matches `Thumbs.db`. *(pinned: `matching_is_case_insensitive`)*
- **Ignore files nest.** Any directory under `content/` may hold its own
  `.modignore`, anchored at that directory; deeper files override
  shallower ones; a nested file beneath an ignored directory is never
  read. The root project file and `content/.modignore` share the same
  anchor, and the in-tree one wins on conflict.
- **`.modignore` files are never packed**, in any format, even with
  `IgnoreMode::Disabled` ("disabled" means "do not filter", not "pack
  every byte on disk"). Consequence: archives are distributions, not
  project backups; extracting an archive does not recreate ignore files.
- **Nothing is excluded by default.** `Thumbs.db`, `.DS_Store`,
  `desktop.ini` all ship unless listed. Project templates (ltk-manager)
  should write a starter `.modignore`.
- **`dir/` and `dir/*` differ beyond negation.** `dir/` prunes: the walk
  never descends and the skipped report records one entry. `dir/*` still
  descends and records each child individually.
- **Skipped counts are entries, not files.** A pruned directory is one
  entry in `ignored_files()` however many files sit beneath it. UIs must
  phrase accordingly.
- **Over-ignoring succeeds silently.** A filter that hides an entire layer
  still produces a valid, empty archive. The data to warn exists
  (`PackResult::ignored_files`, `ContentWalk::skipped`) but no shipped
  consumer surfaces it yet; ltk-manager should.

## Implementation semantics (rustdoc-level)

- **`ModIgnore::parse` reads the disk.** It discovers nested files under
  `content/`, so parsing valid text can fail because of a broken
  `.modignore` elsewhere in the tree. Editor integrations validating a
  buffer should expect that.
- **A `ModIgnore` is a point-in-time snapshot.** Files created or edited
  after `load` are not seen. `FsModContent` caches one snapshot per
  provider instance, so a build's fingerprint and reads agree with each
  other; create a provider per build to pick up edits.
- **Silent walk categories.** Broken symlinks and special files are
  dropped without a record; a link cycle is entered once and cut when it
  would revisit an ancestor *(pinned: `symlink_cycle_terminates`)*; two
  sibling links to the same directory pack the content twice, once under
  each spelling (matches the old walker).
- **Mis-cased ignore files.** Nested files are discovered from the
  directory listing with a case-insensitive name match and read by their
  actual name, so `.MODIGNORE` behaves identically on Windows and Linux
  *(pinned: `mis_cased_nested_ignore_file_is_still_applied`)*. If several
  casings coexist in one directory, the exact `.modignore` spelling wins.
  The root project file is still probed by exact name; a mis-cased root
  file is simply not loaded (it was never a packing candidate, so nothing
  is silently dropped).
- **Three definitions of case-insensitive.** Matching uses the regex
  crate's Unicode case folding, which is neither NTFS's upcase table nor
  the game's own path lowercasing. They agree on ASCII, which covers all
  real game paths. macOS NFD normalization can also make visually
  identical names differ byte-wise. Guidance: keep content paths ASCII.
- **The pattern dialect is globset's, not C git's.** The overwhelmingly
  common cases match git exactly; dark corners differ. An unclosed `[`
  compiles as a literal instead of erroring; `a**b` behaves as globset
  defines rather than git's "two stars equal one star" rule; `a{b` is an
  error here (globset alternates) though git would treat it literally.
- **Fingerprint granularity is mtime seconds + size.** Two edits to any
  file (including a `.modignore`) within the same second that keep its
  length can produce an unchanged fingerprint and a stale overlay cache.
  Long-standing property of the fingerprint scheme, not specific to
  ignore files.
- **`PackError::IgnoreRootMismatch` compares spelling, not identity.**
  `D:\mods\x` vs `d:\mods\x`, or the same directory reached through a
  junction, fails the guard. A false positive is loud and harmless where
  the old behavior was silent mis-filtering; canonicalizing both sides is
  the upgrade path if it ever annoys.
- **`ignored_files()` returns absolute paths.** ltk-manager will likely
  want project-relative display; decide before the API ships in a
  release.

## Performance notes

- Discovery makes exactly one `read_dir` pass per directory; the
  `.modignore` probe rides the same pass (no extra stat per directory).
- Cycle detection costs zero canonicalize syscalls unless a symlinked or
  junction directory is actually descended into; ancestor canonical forms
  are computed lazily and memoized on the chain.
- `FsModContent` loads the filter once per provider; before this, every
  `read_wad_overrides` call re-walked the whole content tree for
  discovery (O(WADs x directories) per overlay build).

## Open items

- ltk-manager: surface the ignored-files report, write a starter
  `.modignore` in new projects, decide relative-vs-absolute reporting.
- Wiki page: fold in the author-facing section above.
