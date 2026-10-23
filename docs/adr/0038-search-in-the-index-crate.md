# ADR-0038: Search in the index crate

- **Status:** Accepted
- **Date:** 2026-10-23
- **Crates:** `ltk_game_index`
- **Related:** [ADR-0009](0009-game-index-crate.md), and the spec that states the rules:
  `docs/design/game-search.md` [section 4](../design/game-search.md#s4) to
  [section 9](../design/game-search.md#s9); `docs/design/game-index.md`
  [section 3](../design/game-index.md#s3)

## Context and problem statement

Two applications want "find in files" over the whole game: a substring typed by the user, and
every bin object that mentions it as a name, a string value, a hash or a link. `ritobin-lsp`
ships it as a VS Code panel and `workspace/symbol`. LTK Manager ships a browser and a search
over the same installation.

`ritobin-lsp` carries 2,400 lines for it on top of `ltk_game_index`: a term walk, a postings
index, substring sheets, a name layer over its Mimir tables, query resolution, an object tree
and a lifecycle. 150 of those lines are a copy of the crate's own object build, the job
partition, the sniff and the read policy. The crate's `objects` feature streams a `PROP` for
object and class hashes and decodes no value; a term walk has to decode every object in full, and
a copy of the build was the only way to do that outside the crate.

ADR-0009 keeps "ranked or exhaustive search" and "directory trees" out of the crate, with "a
third index kind with its own heavy dependency" as the revisit trigger. The term index is a
third index kind. Its one dependency is `memchr`.

LTK Manager has not adopted `ltk_game_index`. Its own object index is a declarations-only
streaming build of 445 ms with no cache, and its surfaces over it are a ranked palette search
with a `class:` grammar, a regular-expression find, a folded object browser, and a References
document whose Embedded, Linked and File queries walk every bin of the installation at query
time. A term index answers those three queries at the object level from postings.

The corpus is 466,863 objects in 50,550 bin chunks across 394 archives, 2.4 GB decoded. A
term index over it is 200 MB of postings and a 100 MB strings sheet.

## Decision drivers

- One walk and one cache serve the LSP and the manager.
- A hash table update never forces a walk.
- The crate stays free of any hash table crate and of `ltk_ritobin`.
- `ltk_overlay` keeps building with `objects` alone.
- The copy of the object build disappears.

## Considered options

1. **A `search` feature of `ltk_game_index`** holding the term index, the sheets, the query and
   the tree, with names through a trait.
2. **A sibling crate `ltk_game_search`** in this workspace, depending on `ltk_game_index` with
   `objects`.
3. **Search stays in each application**, with the crate exposing a full-decode visitor hook on
   the object build.

## Decision

**The term index, the name sheets, query resolution and the object tree are the `search`
feature of `ltk_game_index`. Names enter through the `NameSource` trait. The query answers in
two layers, a match set and a page over it. Ranking, grammar, lifecycle, chunk reading, chunk
rendering and wire types stay in the application.** The rules are in `game-search.md`
[section 4](../design/game-search.md#s4) to [section 9](../design/game-search.md#s9).
`game-index.md` [section 3](../design/game-index.md#s3) states the boundary.

`Declaration` gains a `kind`. `ObjectIndex` gains `DeclarationId` access. `BuildOptions` gains
`on_archive`. The object cache format version is bumped.

## Consequences

- **Positive:** One build produces an object index and a term index. The LSP's copy of the
  object build is gone. The manager's search is the LSP's search, with the manager's tables.
  A table update recuts sheets in seconds and leaves the term index in place.
- **Negative:** `ObjectIndex::declarations` changes shape, and `ltk_overlay` follows. A fix
  to the search feature bumps the version of a crate `ltk_overlay` depends on. A release of the
  crate gates every search change the LSP wants. `memchr` joins the dependency tree. The
  crate's public surface grows by some twenty types. A feature-gated half of a crate breaks
  unnoticed without a CI job per feature set. The manager reaches the feature only after the
  `ltk_game_index` adoption ADR-0009 plans.
- **Revisit when:** a consumer needs structured value queries or numeric search; or the search
  feature's release cadence diverges from the index's far enough that one blocks the other.

## Pros and cons of the options

### A `search` feature of `ltk_game_index`

- Good: the term build reads the object build's private jobs and rows with no public seam.
- Good: one dependency line for the LSP and the manager; `objects` is already a feature and
  the shape is known.
- Bad: the crate owns a query language, a tree and two caches alongside a chunk table.
- Bad: `ltk_overlay`'s dependency carries code it never calls.

### A sibling crate `ltk_game_search`

- Good: independent versions; the index's consumers never see `memchr` or a result type.
- Bad: the term build needs the object build's job partition, sniff and policy, which forces a
  public iteration seam sized for one caller, or a copy of the 150 lines in the new crate.
- Bad: two changelogs and two release cycles for what both consumers use as a pair.

### Search stays in each application

- Good: the crate's boundary holds as ADR-0009 states it.
- Bad: the LSP and the manager each carry 1,500 lines of the same postings, sheets and query
  code, with two sets of bugs.
- Bad: the visitor hook alone does not remove the copy of the sniff and policy from the LSP.
