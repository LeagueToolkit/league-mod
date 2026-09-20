# PRD-XXX: <feature name>

- **Status:** Draft | In review | Approved | Implemented
- **Created:** YYYY-MM-DD
- **Crates:** `ltk_*`
- **Tracking:** #N (umbrella), tickets in `.scratch/<project>/issues/`
- **Spec:** `docs/design/<doc>.md`

## <a id="s1"></a>1. Problem

What a consumer cannot do today, and what that costs them. Measured, where a measurement exists.

## <a id="s2"></a>2. Objective

The outcome that decides whether this worked. One or two sentences.

## <a id="s3"></a>3. Consumers and stories

Who asks for this. `league-mod` ships a CLI and libraries, so a consumer is a mod author at the CLI,
`ltk-manager`, or a crate building on `ltk_*` - not an end user of the game.

- As a **<consumer>**, I want **<capability>**, so that **<outcome>**.

## <a id="s4"></a>4. Requirements

### Functional

Numbered, one behaviour each, testable. `FR-1` is the citation key used by tickets and tests.

- **FR-1:** The crate SHALL ...

### Non-functional

Only where a real constraint exists: round-trip fidelity, allocation on a hot path, what a
consumer may not be forced to depend on.

## <a id="s5"></a>5. Constraints from the game

What the client does that the design has no freedom about, with a citation to the reversing note
or the corpus measurement that establishes it. Facts, not decisions - decisions are ADRs.

## <a id="s6"></a>6. Failure modes

What goes wrong in the field, ranked by how much it costs, and what the design owes each.

## <a id="s7"></a>7. Out of scope

What this deliberately does not do, and where the boundary was argued if it was argued.

## <a id="s8"></a>8. Acceptance

- [ ] **AC-1:** ...
