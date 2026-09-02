---
name: write-adr
description: Write or supersede an architectural decision record under docs/adr/. Use before adding a crate or a dependency, changing the shape of a public API, or diverging from what the game client does - and whenever a design discussion settles a question that had real alternatives. Also use when the user asks for an ADR or a decision record, asks why something was decided, or when a spec is carrying an argument that should be a citation.
---

# Write an ADR

An ADR records **one choice**: the situation that forced it, the options it beat, which one was
taken, and what that costs. It is written once, dated, and never rewritten.

Its whole value is that it is temporally bound - it preserves what was known and weighed at a
moment. That is also its limit, and the limit is what the rules below protect.

## An ADR is not where a rule lives

The current rule lives in the spec (`docs/design/<feature>.md`), in the present tense, edited in
place. The ADR says a choice was made and why the alternatives lost. A reader asking **"what does
the code do?"** must never have to open an ADR to find out, because an immutable document cannot
answer a question about the present: the answer would be the original rule plus every correction
note and superseding record, and nothing tells the reader they found them all.

So an ADR does **not** contain:

- **Vocabulary or domain definitions.** What a term means changes as the domain is understood
  better, so it needs a container that can be rewritten. The spec's vocabulary section owns names
  and definitions. An ADR may argue for a name; the spec is where the name is then defined.
- **The authoritative statement of a current rule.** State the choice, then cite the spec section
  that specifies it. Where the two ever disagree, the spec is right and the ADR needs superseding.
- **An API surface.** No type definitions, no signature blocks to be kept in sync. Name the item;
  the spec holds its shape.

What it does contain is the part the spec deliberately drops: the alternatives, and why they lost.

## Does this decision deserve one?

All three must hold:

1. **Hard to reverse** - changing your mind later costs real work, a breaking API change, or a
   re-parse of shipped data.
2. **Surprising without context** - a reader six months out will ask "why on earth this way?"
3. **A real trade-off** - there were alternatives someone could reasonably have picked, and one
   was chosen for stated reasons.

Miss any one and it is not an ADR. It is a **rules-table row** in the spec: the rule, what it was
instead of, a one-sentence why. Of 29 rules in the PTCH work in the `league-toolkit` repository, 6 earned an ADR. That ratio is the
point - an ADR per decision is a pile nobody reads, and it buries the few that matter.

Two triggers force one regardless: **adding a crate or a workspace dependency**, and **diverging
from what the game client does**. Both are things a future reader would otherwise take for an
accident.

## Where it goes

`docs/adr/NNNN-slug.md` - four digits, next free number, never reused, never renumbered. Numbers
are global to the repo, not per feature. Start from `docs/adr/template.md`.

**The title is a terse noun phrase naming the decision, not a sentence arguing it.** Two to
five words in the codebase's own vocabulary, the same shape as a commit subject: `Path class
context`, `Single-visitor walk`, `Object cache provider`. Not `A field step names the class it is
on` - the claim and its reasoning belong in the Decision section, where they can be read in
full. The slug is the title, lowercased and hyphenated: `0012-path-class-context`. ADR-0001 and
ADR-0002 predate this rule and keep their sentence titles.

## Writing it

- **Context and problem statement** carries the evidence that makes the choice inevitable rather
  than arbitrary: what the client does, what the corpus measures, what the consumer needs. Cite
  `FR-N` rather than restating requirements.
- **Considered options** lists what was genuinely on the table, at least two. An option nobody
  could have picked is padding and makes the real choice look unexamined. If a rejected option was
  tempting, say why it was tempting - that is what stops it being re-proposed next quarter.
- **Decision** names the option taken, in bold, as the rule the code obeys, and **points at the
  spec section that states the rule**. The header date is the only moment in the record; the
  body is untensed and free of narrative. One or two sentences on what it means concretely - the error it produces,
  the invariant it holds - and no more. If the section is growing into a specification, that
  content belongs in the spec.
- **Consequences** must carry a **Negative** that is a genuine cost; "none" means the analysis is
  unfinished. ADR-0001 admits that a recomputed checksum hides a lying source TOC from the
  build result's consumer unless they read the warning. Then **Revisit when:** names the
  one fact that would change the answer, or says nothing foreseeable.
- **Pros and cons of the options** gives each rejected option a fair hearing, concretely.

## Changing one

An accepted ADR is a record of a moment. It is not a wiki page.

- **The decision changed** - write a new ADR that supersedes it, set the old one's status to
  `Superseded by ADR-NNNN`, and **edit the spec** so the rule it states is the new one. The spec
  edit is the part that matters; the ADR pair is the audit trail behind it.
- **A fact in it was wrong** - correct the fact in place and date the correction in the text, the
  way the `league-toolkit` repository's ADR-0003 dates a corrected reading of the client's pointer test. The decision keeps its number and its status.
- **The rule drifted without anyone deciding** - that is a spec edit, and the ADR is left alone. Do
  not retrofit an old record to match new behaviour; that destroys the only thing it was for.

## Finishing

1. Header: status, date, crates, and **Related** - the PRD and its `FR-N`, the issues, and the spec
   section that specifies the rule.
2. Wire the citation back: the spec's rules table names `ADR-NNNN` in its Spec column, and the
   ticket implementing it cites the ADR instead of carrying the argument.
3. ASCII only, never the section sign, and a section reference is a linked "section N".
4. Prose is declarative, and free of time and cause: no temporal anchor, no causal
   connective, no narrative. Both rules are in the Documentation section of CLAUDE.md.
5. If a ticket renders from a document that changed, run the `sync-issues` skill.
