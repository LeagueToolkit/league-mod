---
name: write-ticket
description: Write or revise a ticket under .scratch/<project>/issues/ - one slice of implementable work, rendered to a GitHub issue. Use when a spec or PRD is ready and the work needs breaking into issues, when the user asks for a ticket, an issue, or a work breakdown, when a new piece of work needs an issue created, and when a ticket needs re-slicing because it grew too big to land in one PR.
---

# Write a ticket

A ticket is **one slice of implementable work**: something a person can land in a single PR and a
reviewer can accept or reject on its own. It is an API proposal, not a task description. The
worked set is `.scratch/ptch-diff-merge/issues/` in the `league-toolkit` repository.

This skill writes the file. The `sync-issues` skill pushes it to GitHub and **owns the issue body
format** - read it for the body structure, the label scheme and the voice rules rather than
guessing here.

## Where it goes

`.scratch/<project>/issues/NN-slug.md`. `<project>` is the effort, not the crate
(`overlay-builder`, `modpkg-v2`). `NN` orders the tickets within the project and is **not**
the issue number - the issue number lives in the frontmatter, and a ticket that has never been
pushed simply has no `issue:` key yet.

```yaml
---
issue: 219
title: "Overlay: pass-through of container chunks"
labels: crate:ltk_overlay, enhancement, area:api
---
```

`00-umbrella.md` is the project's parent issue: a **Documents** table pointing at the PRD, the
spec and the ADRs, a short "what this is for", and a **Children** checklist naming each
child issue with one line on what it delivers and what waits on it. Everything else lives in the
documents it links.

## Slicing

- **One reviewable change.** If the acceptance checklist splits cleanly into two groups that share
  nothing, it is two tickets.
- **Order by dependency, and say so.** A `Blocked by #N` line naming real issue numbers, resolved
  through the frontmatter of the tickets it depends on, and one line of prose on why the order
  holds ("goes first; #220 rests on it"). Say what is *not* blocked too, the way the umbrella
  closes its list with "#221's record surface and #223 depend on nothing here and can land in any
  order" - someone picking up work needs to know what is free, not only what is stuck.
- **A ticket with no consumer waiting is parked, not scheduled.** Say that in the ticket and name
  the decision that parked it, the way #221 carries `Bin::diff` as designed and parked under
  ADR-0004. A parked slice in a written ticket is cheaper than one rediscovered later.
- **A dropped ticket keeps its issue** and explains what killed it and what narrow case is left
  open. #222 is the model.

## Writing it

The structure is `sync-issues`' issue body format: one paragraph placing it, `## Proposed surface`,
rationale, `Blocked by`, checklist last. What matters most here:

- **Lift signatures verbatim from the spec.** Do not paraphrase an API into prose - that is
  exactly the drift the whole rendering arrangement exists to prevent. If the spec has no
  signature for this work, the spec is where to go next, not the ticket.
- **Cite, never restate.** `FR-N` for a requirement, `ADR-NNNN` for a decision, a design section
  for the surface. A ticket that re-argues a decision becomes a second copy of it that drifts. One
  sentence naming what was decided, then the pointer.
- **The rationale is only what a reviewer needs to evaluate the proposal** - the constraint that
  forces the shape, the invariant it must hold. Not the spec's narrative.
- **The checklist is test-shaped.** Each item is something that can be observed to pass:
  "`to_property_path` reports the first unnameable field hash rather than the last", not
  "handle errors properly". Checklist items go last, after everything a reader needs to judge them.
- **No H1 heading in the body** - the issue title renders above it.

## Finishing

1. ASCII only, never the section sign, and a section reference is a linked "section N".

2. Prose is declarative, and free of time and cause: no temporal anchor, no causal
    connective, no narrative. Both rules are in the Documentation section of CLAUDE.md.
3. Everything published to GitHub is in the maintainer's voice. No AI attribution of any kind.
4. Run the `sync-issues` skill: it creates the issue for a ticket with no number, writes the
   number back into the frontmatter, and pushes bodies and labels for the rest.
