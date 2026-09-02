---
name: sync-issues
description: Push ticket files and design-doc changes to their GitHub issues so the issues never drift from the repo. Use this whenever a ticket file under .scratch/*/issues/ has been created or edited, whenever a document it renders from (docs/design/, docs/prd/, docs/adr/) has changed, whenever the user asks to sync, update, publish, or create GitHub issues from tickets or a design doc, and at the end of any task that touched those files — even if the user did not mention issues. Also use it when a new ticket file needs a GitHub issue created for it.
---

# Sync tickets to GitHub issues

The repo is the single source of truth for planned work. Ticket files under
`.scratch/<project>/issues/*.md` — and the documents they draw from: `docs/design/` for the
API surface, `docs/prd/` for requirements, `docs/adr/` for decisions — are what gets edited;
the GitHub issues are rendered artifacts of them. Nobody hand-edits
an issue body on GitHub — if an issue and its ticket disagree, the ticket wins, and this
skill's job is to make GitHub match. That rule is what makes "which copy is current?" a
question that never needs asking.

## The mapping

Each ticket file carries YAML frontmatter binding it to its issue, and everything below
the frontmatter is the issue body, pushed verbatim:

```yaml
---
issue: 207
title: "Bin streaming foundation: mount, TOC, harvest, and the wire core"
labels: crate:ltk_overlay, enhancement, area:reading, blocked
---
```

`title` and `labels` apply on creation; on an existing issue only the body is pushed
(titles stay stable — see below). Because the body is verbatim, strip the frontmatter
before pushing (`sed '1,/^---$/d'` deletes line 1 through the closing `---`) and never put
an H1 title heading in the body — the issue title already renders above it.

**Labels are the one field a body push cannot carry.** `gh issue edit --body-file` leaves
them untouched, so a ticket whose `labels:` line changed silently diverges from GitHub —
the repo says `blocked` is gone, the issue still shows it, and nothing complains. When a
sync changes a ticket's labels, push them in the same step as the body:

```bash
gh issue edit <N> --repo LeagueToolkit/league-mod \
  --add-label "<added>" --remove-label "<removed>"
```

Work out `<added>` and `<removed>` by diffing the ticket's `labels:` against
`gh issue view <N> --json labels`, rather than re-pushing the whole set. A label on the
issue that no ticket names is somebody's triage — leave it alone.

A ticket without an `issue:` key has no GitHub issue yet — create one (see below) and
write the number back into the ticket's frontmatter immediately. That write-back is part
of creating the issue, not a follow-up: a created-but-unrecorded issue is how duplicates
happen on the next sync.

## Before pushing

Check that the checkout is the repository these tickets belong to:

```bash
git config --get remote.origin.url    # expect LeagueToolkit/league-mod
```

Every `gh` call below names `--repo LeagueToolkit/league-mod` explicitly rather than letting
`gh` infer it from the directory, which is what makes this check worth doing: on a fork, a mirror,
or a sibling repo opened by mistake, an inferred repo files somebody else's tickets in the wrong
tracker, and a hardcoded one files them in a tracker the working copy has nothing to do with.
Both are silent. If origin is not `LeagueToolkit/league-mod`, stop and ask which repository
the issues belong in. Never create issues in a repository that does not match the remote of the
checkout being synced.

## Procedure

1. **Work out which tickets are affected.** If the session edited specific tickets or document
   sections, sync those; if the user asked for a full sync, sync every ticket in the directory.
   Don't skip a ticket because its own file is unchanged when the spec, PRD or ADR sections
   it renders from changed.

2. **Create every missing issue first, in dependency order** - all the tickets with no `issue:`
   key, before any body is pushed. Bodies cross-reference issue numbers (`Part of #218`,
   `Blocked by #219`, the umbrella's Children list), and a number that does not exist yet renders
   as a dangling reference nobody notices. So a ticket that others reference is created before the
   tickets referencing it, and **each returned number is written into its ticket's frontmatter as
   it comes back**, before the next is created. An issue created but not recorded is how
   duplicates happen on the next sync.

   ```bash
   gh issue create --repo LeagueToolkit/league-mod \
     --title "<title>" --label "<labels>" --body-file <file>
   ```

   An umbrella and its children reference each other, so that cycle cannot be ordered away:
   create the umbrella first - its Children list is what names the work - then the children, then
   push the umbrella's body again in step 3 with the numbers filled in.

3. **Push every affected body**, now that every number resolves. Render it to a file in the
   scratchpad first, never inline:

   ```bash
   gh issue edit <N> --repo LeagueToolkit/league-mod --body-file <file>
   ```

   Follow with a label push for any ticket whose `labels:` line changed (see The mapping).

4. **Report by number**: issues created, bodies updated, labels added or removed, and any ticket
   deliberately skipped. `gh` prints a URL per call - check each one succeeded. "Synced" with no
   numbers in it is not a report; the numbers are what the maintainer checks against GitHub.

Titles stay stable across syncs — renaming an issue breaks people's links and
notifications, so change a title only when the user asks.

## Issue body format

Issues are **API proposals**, written the way the specs write their API-surface
sections (the model is `docs/design/bin-streaming.md` sections 4-5 in the `league-toolkit` repository). Structure, in order:

1. One short paragraph: what this delivers and which umbrella issue / spec it
   belongs to (e.g. "Part of #N (design: `docs/design/<doc>.md`
   [section 5](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/<doc>.md#s5))" -
   an issue body needs the absolute URL, because a bare `#s5` resolves against the issue page).
2. `## Proposed surface` — Rust signature blocks lifted from the spec, not
   paraphrased into prose. Keep the abbreviated doc comments; elide private internals
   with `/* … */`. If the spec has the signatures, extract them verbatim; prose
   summaries of an API are exactly the drift this skill exists to prevent.
3. Rationale and invariants as short prose or bullets — only what a reader needs to
   evaluate the proposal, not the spec's full narrative.
4. A `Blocked by #N` line naming real issue numbers (resolve them through the frontmatter
   mapping of the tickets it depends on).
5. The acceptance checklist (`- [ ]` items), last.

Labels follow the existing scheme: `crate:ltk_*` for the crate, `enhancement`/`bug`,
`format:*`, `area:reading`/`area:api`/`area:writing`, `breaking-change` where it applies,
`blocked` while a `Blocked by` line names an open issue.

## Voice

Everything published to GitHub must read as written by the repo owner. No AI attribution
of any kind: no "Generated with Claude Code" footers, no Co-Authored-By lines, no session
links, no mention of agents or assistants — in issue bodies, titles, or comments. Plain
technical register, matching the specs.

## What not to do

- Don't edit issue bodies with inline `--body` strings; Rust snippets full of quotes and
  backticks mangle in shell quoting. Always go through `--body-file`.
- Don't close, reopen, or comment on issues — this skill only creates bodies and keeps
  them current. State transitions are the maintainer's.
- Don't "improve" ticket content while syncing. Rendering is mechanical; if a ticket
  seems wrong, tell the user instead of silently fixing it on GitHub.
