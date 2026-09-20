---
name: write-prd
description: Write or revise a product requirements document under docs/prd/. Use when a feature needs its "why" settled before any API is designed - a capability a consumer cannot get today, a request that arrives as a problem rather than a signature, or a design doc that has accumulated requirements prose. Also use whenever the user asks for a PRD, a requirements doc, or numbered requirements, or asks to add, revise, or withdraw an FR-N.
---

# Write a PRD

A PRD holds **why a feature exists, who asks for it, and what it must do**. Nothing in it is an
API and nothing in it is a decision. See the document table in `CLAUDE.md`; the worked example is
`docs/prd/001-ptch-property-patches.md` in the `league-toolkit` repository.

## Does this need a PRD at all?

Write one when a **named consumer cannot do something today** and the requirements will outlive
any single ticket - when more than one ticket, test, or ADR will cite them.

Skip it for a bug fix, a refactor, a single-ticket addition with nothing to cite, or an API shape
that is already decided. An already-decided shape belongs in `docs/design/`, and the reasons it
beat the alternatives are ADRs.

If it is not clear which, ask one question: **what breaks, for whom, today?** If that has no
answer with a consumer's name in it, there is nothing to require yet - say so rather than
inventing a stakeholder.

## Where it goes

`docs/prd/NNN-slug.md` - three digits, next free number, never reusing one. The slug names the
feature, not the crate (`001-ptch-property-patches`, not `001-ltk-meta`). Start from
`docs/prd/template.md` and keep its section numbers even where a section is one line, so
`section 6` means the same thing in every PRD and every citation to one.

## What belongs here

| Goes in the PRD | Goes elsewhere |
| --- | --- |
| The problem, measured | - |
| Who asks, as a story | - |
| `FR-N`, one behaviour each | A signature implementing it -> `docs/design/` |
| Constraints the game imposes, as facts | Why we answered a constraint this way -> an ADR |
| Failure modes, ranked | The code that handles one -> a ticket |
| Out of scope | The argument that put it there -> an ADR |

The test when a paragraph will not settle: **a requirement survives a total rewrite of the
crate; a decision does not.** If rewriting the implementation in another language would keep the
sentence true, it is a requirement. If it would make the sentence moot, it is a decision, and it
is an ADR or a spec row.

## Rules that keep it useful

- **`FR-N` is a citation key, not an ordering.** Tickets, tests and ADRs cite it by number, so
  numbers never shift. Append new requirements at the end; withdraw one by leaving the number and
  marking it withdrawn with a date and a reason. The same holds for `AC-N`.
- **One behaviour per requirement, and testable.** "The crate SHALL ..." If no test can fail for
  want of it, it is background - move it to section 1 or section 5.
- **Section 5 is facts with citations.** Every constraint names the reversing note, the client
  function, or the corpus measurement that establishes it. A constraint you cannot cite is an
  assumption; label it as one rather than dressing it up.
- **Measurements name their specimen and their build.** "626 objects and 1,151 map keys, on one
  16.13 champion WAD" is arguable and checkable. "Many objects" is neither.
- **Consumers are crates, tools, and the people building them.** `league-mod` ships a CLI and libraries:
  a consumer is a mod author at the CLI, `ltk-manager`, or a crate building on `ltk_*`, never a
  player.
- **Failure modes are ranked by what they cost**, and each says what the design owes it. An
  unranked list of everything that could go wrong steers nothing.
- **No signatures.** Writing `pub fn` is the signal you are in the wrong file.

## Finishing

1. Fill the header: status, date, crates, tracking issue, spec, ADRs.
2. Point the spec's header at the PRD, so the citation runs both ways.
3. Status moves Draft -> In review -> Approved -> Implemented. An approved PRD gains requirements
   by appending; it does not get quietly rewritten.
4. ASCII only, never the section sign, and a section reference is a linked "section N".
5. Prose is declarative, and free of time and cause: no temporal anchor, no causal
   connective, no narrative. Both rules are in the Documentation section of CLAUDE.md.
6. If any ticket under `.scratch/*/issues/` renders from what changed, run the `sync-issues` skill
   before finishing.
