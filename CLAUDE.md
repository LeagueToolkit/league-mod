# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Documentation

Four kinds of document, one job each. What separates them is not only subject but **lifecycle** — mixing a living document with a historical record is what turns a design doc into an unreadable pile:

| Document   | Answers                                                      | Tense      | Lifecycle                                   | Where                            |
| ---------- | ------------------------------------------------------------ | ---------- | ------------------------------------------- | -------------------------------- |
| **PRD**    | Why at all, for whom, what it must do (`FR-N`)               | Present    | Requirements append; numbers never shift    | `docs/prd/NNN-slug.md`           |
| **Spec**   | What is true: surface, wire format, traversal, errors, tests | Present    | **Edited in place, forever**                | `docs/design/<feature>.md`       |
| **ADR**    | Why this option and not that one                             | Present    | **Immutable** — superseded, never rewritten | `docs/adr/NNNN-slug.md`          |
| **Ticket** | What to build next                                           | Imperative | Closed when done                            | `.scratch/<project>/issues/*.md` |

- **Every rule has exactly one home, and it is the spec.** An ADR records that a choice was made and what it beat; it is never where a reader looks to learn how the code behaves today. A rule stated in two places has one stale copy and no way to tell which.
- **Domain vocabulary lives in the spec**, in a section near the top. A definition changes as the domain is understood better, so it needs a container that can be rewritten — an immutable dated record is the wrong one.
- **A spec is edited in place, never appended to.** When the code departs from it, the section that stated the old thing is rewritten to state the new one. No phase sections, no "implementation notes", no correction notes appended below. Measurements are the one exception: they are facts about a specific build and live in a dated appendix.
- A spec **cites**: requirements as `FR-N`, decisions as `ADR-NNNN`. It does not restate them. Two copies of one argument drift.
- **A section reference is a linked "section N".** Every numbered heading in a PRD or a spec carries a stable anchor — `## <a id="s4.3"></a>4.3. Views` — and every citation is a link whose text says what it points at: `[section 4.3](#s4.3)`, parenthesised where it is an aside, and spelled out in full on both halves of a pair or a range — `[section 4.2](#s4.2) and [section 4.3](#s4.3)`, `[section 4](#s4) to [section 9](#s9)` — so no link ever renders as a bare number with nothing to say what it is. Appendices are `[appendix B](#appendix-b)`. A cross-document reference names the file first — `` `overlay-builder.md` [section 6](overlay-builder.md#s6) `` — and a ticket uses the absolute `https://github.com/LeagueToolkit/league-mod/blob/main/<path>#sN` form, because a bare fragment in an issue body resolves against the issue page instead. Inside a code block a reference stays plain prose: a link cannot render there, and doc comments get copied into source. The anchor is the point — a heading can be reworded freely and no citation breaks.
- **Prose is declarative, and free of time and cause.** This holds for every document above and
  for every doc comment in the code. A sentence states one fact that is true of the subject as it
  is. It does not say when the fact became true, what it replaced, or what it follows from.
  - **No temporal anchor.** No `now`, `currently`, `today`, `previously`, `used to`, `no longer`,
    `still`, `will`, `once X lands`, `after the refactor`, `as of`, `for now`, `going forward`.
    The lifecycle markers a document type owns are the one exception: an ADR's status line, a
    PRD's status, the date on a withdrawn `FR-N`, the build named on a measurement.
  - **No causal connective between facts.** No `because`, `since`, `so`, `so that`, `therefore`,
    `hence`, `thus`, `as a result`, `which means`, `in order to`, `this is why`. Each fact is its
    own sentence and the reader derives the consequence. A why is written as the fact that grounds
    the rule, next to the rule, not as a chain joining the two.
  - **No narrative.** A document does not tell what was done, decided, tried, or changed. It states
    what is. `git log` and the ADR carry the history.

  ```
  Bad   A chunk with a bad checksum is now passed through with a warning, because the client
        rejects it and we changed the build to match.
  Good  The client rejects a chunk whose checksum disagrees with its bytes. The build recomputes
        the checksum and reports a mismatch as a warning.

  Bad   Paths are lowercased before hashing so that they match the client's WAD lookup.
  Good  The client looks up a WAD entry by the XXHash64 of its lowercased path. A path is
        lowercased before hashing.
  ```

  A `Why` column, an ADR's context, and a PRD's problem statement are still declarative: they
  hold the facts a decision rests on, each as its own statement. An ADR is a record of one
  moment and its header date carries that moment; the body is untensed. The Decision states the
  option taken as the rule the code obeys.

  Doc comments carry the most volume and lean on `so that`. A purpose clause is a second
  sentence stating the invariant:

  ```
  Bad   /// Recomputes the checksum so that a lying source TOC never reaches the client.
  Good  /// Recomputes the checksum over the bytes in flight. The overlay TOC carries the computed value.

  Bad   /// Returns `None` once the layer has been dropped, since a build never re-reads it.
  Good  /// Returns `None` for a dropped layer. A build reads each layer once.
  ```
- Write an ADR before adding a crate, changing the shape of a public API, or diverging from what the game client does. Name at least two viable alternatives with concrete trade-offs.
- Templates: `docs/prd/template.md`, `docs/adr/template.md`. Worked example: PRD-001, ADR-0001 to ADR-0006 and `docs/design/ptch-property-patches.md` in the `league-toolkit` repository. ADR-0001 and ADR-0002 here predate the format and keep their shape.
- Skills: `write-prd`, `write-spec`, `write-adr` and `write-ticket` produce these files, `sync-issues` renders tickets to GitHub. Each carries the rule for its own document (when it is worth writing, how it is numbered, what it must not absorb).

## Issue Sync

GitHub issues are rendered from the ticket files in `.scratch/*/issues/` (frontmatter `issue: N` maps each ticket to its issue). When a task changes a ticket file, or a document under `docs/design/`, `docs/prd/` or `docs/adr/` that a ticket renders from, run the `sync-issues` skill before finishing so the issues never drift from the repo. Anything published to GitHub (issues, PR bodies, commits) is written in the maintainer's voice — no AI attribution of any kind.

## Project Overview

League Mod Toolkit - A Rust workspace containing CLI tools and libraries for creating, managing, and distributing League of Legends mods using the `.modpkg` format.

**Consumers.** [LTK Manager](https://github.com/LeagueToolkit/ltk-manager) is the desktop app
where end users interact with this code, and the main consumer of these crates. Its checkout
is the reference for what a public API here has to serve. An API question the manager raises
is settled in this repo's spec, and the manager cites it.

## Quick Commands

### Rust (CLI and Libraries)

```bash
# Build all crates
cargo build --release

# Run CLI
cargo run --bin league-mod -- <command>

# Run tests
cargo test

# Run tests for specific crate
cargo test -p ltk_modpkg

# Lint
cargo clippy

# Format
cargo fmt
```

## Architecture Overview

### Workspace Structure

This is a Cargo workspace with the following crates:

- **`league-mod`** - CLI tool for mod developers (init, pack, extract, info)
- **`ltk_modpkg`** - Binary format library for `.modpkg` files (reading, writing, compression)
- **`ltk_mod_project`** - Configuration library (JSON/TOML config, metadata structures)
- **`ltk_mod_core`** - Shared utilities (League path detection, cross-platform utilities)
- **`ltk_fantome`** - Fantome archive format support (`.fantome` files)
- **`ltk_overlay`** - Overlay building engine (WAD patching, game file indexing)

## Mod Format Reference

### Project Structure
```
my-mod
|-- mod.config.json           # Project configuration
|-- content                   # Mod content by layer
|   |-- base                  # Base layer (priority 0)
|   |   |-- Aatrox.wad.client # Files for Aatrox WAD
|   |   |-- Map11.wad.client  # Files for Summoner's Rift
|   |-- high_res              # Optional layer
|-- build                     # Output .modpkg files
```

### Layer System
- Layers have priorities (higher = loaded later)
- Higher priority layers override lower priority layers
- Base layer always present (priority 0)
- Additional layers are optional

## CI/CD

All contributions go through CI:
- Code compilation (Linux, Windows, macOS)
- Test suite execution
- Clippy linting
- Format verification
- Security audit
- License checks

## Commits and PRs

One conventional-commit subject line. No body, no trailers, no `Co-Authored-By`, no session
links. A PR is that same subject as its title and an empty body. Never commit or push unasked.

The scope is the crate without its `ltk_` prefix (`modpkg`, `fantome`, `overlay`), `cli` for
`league-mod`, or, for work outside one, the area (`docs`, `ci`, `workspace`). release-plz reads
the type and the `!` marker for the version bump; a body carries nothing it uses.

**A subject names the change, it does not describe it.** A plain verb and a domain noun phrase,
roughly three to six words, in the codebase's own vocabulary. No contrastive clause, no mechanism,
no narrative verb, and drop articles that carry nothing.

```
Bad   fix(fantome): skip thumbnails that cannot be converted instead of failing the whole import
Good  fix(fantome): skip unconvertible thumbnails on import

Bad   feat(overlay): pass chunk bytes through from the container while recomputing the checksum
Good  feat(overlay): pass through container chunks

Bad   docs(modpkg): describe the layer priority order and how higher layers win
Good  docs(modpkg): specify layer priority
```

The same shape holds for an **ADR title**: a noun phrase naming the decision (`Pass-through
checksum`, `Import-time normalization`), never a sentence stating it. Only a **test name** stays
narrative, because it is read one at a time and is the only sentence saying what the case is.
