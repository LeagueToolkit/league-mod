---
name: write-spec
description: Write or revise the API design spec for one feature or domain under docs/design/. Use whenever a surface, wire format, traversal rule, error or testing approach is being designed or has changed - including when an implementation departed from what the spec said, which is edited in place rather than annotated. Also use when the user asks for a design doc, an API spec or a surface, when domain vocabulary needs defining, and when a spec has accumulated phase notes, implementation notes, changelogs or review resolutions that need folding back in.
---

# Write a spec

A spec is the **current and complete statement of one feature's surface**: the vocabulary, the
types, the signatures, the wire format, the traversal rules, the errors, the tests. It is what a
reader opens to learn what is true, and every sentence in it is true now.

One spec per feature or domain, at `docs/design/<feature>.md`. It is scoped by subject, never by
release or phase, so it has no end - it is edited for as long as the feature exists.

## The one rule

**A spec is edited in place. It is never appended to.**

When the code departs from the spec, the section that stated the old thing is rewritten to state
the new one. It does not keep the old text and gain a note further down recording the departure.
That is the specific failure this discipline exists to prevent: it turns reading into "read the
section, then find every later note that amends it", with nothing to tell the reader how many
notes there are or whether they found them all. A section that is wrong is worse than a section
that is missing, because it is believed.

So a spec never contains:

- a section named for a phase, a PR, a release, or a review round
- "implementation notes", "what changed", "what the reference changed", "open questions since"
- a rule with a correction note attached, or a decision dated in its text
- two tables of the same kind of thing, split by when they were written

Each of those is a ladder rung. The content in them is not deleted - it is folded into the section
that owns the subject, in the present tense, and the rung goes away.

## Every rule has exactly one home

| Document | Answers | Tense | Lifecycle |
| --- | --- | --- | --- |
| **Spec** `docs/design/` | What is true | Present | Edited in place, forever |
| **ADR** `docs/adr/` | Why this and not that | Present | Immutable; superseded, never edited |
| **PRD** `docs/prd/` | Why at all, for whom | Present | Requirements appended, never renumbered |
| **Ticket** `.scratch/*/issues/` | What to build next | Imperative | Closed when done |

The spec is the only one of the four that states a current rule. An ADR records that a choice was
made and what it beat; it is **not** where a reader looks to find out how the code behaves today.
When the two disagree, the spec wins and the ADR gets superseded. If a rule appears in both, one
copy is stale and nobody can tell which.

## Vocabulary comes first

A spec's second section, after the summary, names its domain: every term the rest of the document
uses in a specific sense, defined once - the types, the words for the operations, and the words
this crate deliberately does not use with what it uses instead.

**Domain modeling belongs here, not in an ADR.** A definition changes when the understanding of the
domain changes, so it needs a container that can be rewritten; an immutable dated record is exactly
the wrong one. If an ADR argued for a name, the ADR keeps the argument and the vocabulary section
holds the name.

## Structure follows the domain

Order the sections the way someone learning the feature would want them, never the order the work
happened in. A reader who wants to know how one operation behaves reads one section and is done.

A workable spine: summary, vocabulary, evidence, wire format, data model, one section per
operation, testing, the rules table, appendices. Numbers are for citing, so tickets and ADRs can
point at [section 9.2](#s9.2). Every numbered heading carries its anchor - `### <a id="s9.2"></a>9.2
Traversal rules` - so a heading can be reworded freely; renumbering is what breaks citations, so
when the structure changes, fix the anchors and every citation to them in the same commit.

## The rules table

The end matter is one normalized table - `| ID | Rule | Instead of | Why | Spec |` - holding every
rule too small to deserve its own section. It is the spec's index of settled questions.

- **One table**, ordered by subject. Never a second table for rules settled later.
- **IDs are stable citation keys.** Tickets and tests cite `D11`. A rule that changes keeps its ID
  and gets its row rewritten; a rule that is withdrawn keeps its ID and says so. New rules append.
- **A row states the rule, not the argument.** If the Why needs more than a sentence, the rule has
  an ADR: name it in the Spec column and cut the row back to the rule.
- No dates, no "settled in review on", no phase attribution. Who settled a rule and when is what
  `git log` and the ADR are for.

## Evidence is dated; the spec is not

Measurements are the exception and the reason the rule is stated so narrowly. "238 of 238 chunks
round-trip byte for byte" is a fact about a specific client build, and it goes stale in a way a
rule does not. Keep measurements in an appendix, each naming the client version and corpus it was
taken against. That appendix is the only place in a spec where a date belongs.

## Editing an existing spec

1. **Find the section that owns the subject**, not the end of the document. If two sections own it,
   that is the bug - merge them.
2. **Rewrite it in the present tense** to say what is now true. Do not describe the change.
3. **If the change had alternatives worth recording**, write the ADR (`write-adr` skill) and cite
   it from the rules table. The spec still states the rule outright.
4. **Delete what the rewrite made redundant.** A spec that only grows is one nobody reads to the
   end, and its last sections are where the stale text hides.
5. **Fix every citation** that moved - ADRs, the PRD, tickets - in the same change.

## Finishing

- Cite requirements as `FR-N` and decisions as `ADR-NNNN`; do not restate either.
- ASCII only, never the section sign, and a section reference is a linked "section N".
- Prose is declarative, and free of time and cause: no temporal anchor, no causal
  connective, no narrative. Both rules are in the Documentation section of CLAUDE.md.
- If a ticket renders from a section that moved or changed, run the `sync-issues` skill.
