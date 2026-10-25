---
name: proposal
description: Write or revise a proposal in docs/proposals/ for an analyzer or feature that is specified but not implemented. Use when drafting a new proposal, revising one after review, or adding its explainer.
---

# Proposals

A proposal specifies behavior the code does not yet have. Its reader is the
project owner, who wrote the core and will judge whether the design is sound
and whether the human-shaped authoring tools (change files, the CLI, the
browser) make the behavior obvious. Write for that review, not for an
implementer who already agrees.

A proposal has its own directory, `docs/proposals/<name>/`, holding
`<NAME>.md`, the contract, `<NAME>-EXPLAINER.md`, a walkthrough, and
`harness/`, which replays the explainer's change files (see below). Links to
the core contracts go up two levels, as in `../../SPEC.md`. AGENTS.md lists
the pair under "Proposed, not implemented" until the work lands. Then the
contract and explainer move to `docs/` beside the contracts they join, and
the implementation's tests replace the harness, as the RECONNECT and PLANNING
replay tests did.

## The contract

Open with the status line and the documents it relies on. Then, before any
rule, a vocabulary section with two tables:

- Node types introduced: name, role, each field with its policy (mandatory,
  optional) and value type, and what makes an instance terminal if anything.
- Edge labels interpreted: one row per permitted `({from types}, label, {to
  types})` set, with the meaning of the edge. State that any other endpoint
  pair for those labels is an inconsistency.

Every term used in a rule is defined before the rule uses it. Each rule names
its judging prefix and the exact entries its certificate carries.

State the analyzer's **activation conditions** in one place before its
findings: the kinds of prefix at which it can emit a new finding, such as one
whose newest entry is a node with a predecessor, an entry of the proposal's
own types or labels, or a declaration of one of its types, and that every
other prefix yields nothing. Say so plainly even when the answer is every
prefix. Name any nonsticky message re-emitted at other prefixes while its
condition holds. Name the analyzers the proposal relies on, such as
TypeCheck, and the checks it leaves to them.

Keep a **Design decisions** section and append to it whenever a choice is
made, including choices made in a review conversation. One entry per
decision: the decision in a sentence, the alternative it rejected, and the
reason. A steering remark that changes a rule is a decision and gets an entry;
without one, the next writer inherits the rule as a principle and loses the
reason, which is how rules drift past what was meant.

Keep an **Open questions** section and an **Acceptance tests** section with
structural assertions, checked before and after the relevant append. Include
a prefix outside the activation conditions that yields nothing.

## The explainer

The explainer carries one running example from an empty database to the
behavior under discussion, in the order a user meets it. Show the entries as
the change files a user would actually write ([FILES](../../../docs/FILES.md)),
not as prose, so that the reader can judge whether the authoring tools make
the behavior obvious. Each rule of the contract appears in the explainer at
least once with concrete entries and the finding or absence of finding it
produces. Where a reader would ask "why not the other way", answer in a
callout and link the matching design decision.

Every change file in the explainer comes from a real run, so its positions
and origins are the ones the core assigned and every file passes the
structural checks and TypeCheck. The proposal's `harness/` keeps the change
files as templates, the run they produced, and the explainer's text as a
template; its README says how to use it. The planning harness, retired when
PLANNING landed, is the model for a new one: see `git show
c6fcf32:docs/proposals/planning/harness/README.md` and its siblings. Never
edit a position or origin by hand: edit the template and regenerate. Before
you finish, check that the explainer matches what its template produces.

## Style

Redundancy is welcome until the proposal is implemented. The explainer may
restate what SPEC or ANALYZERS say when the reader needs it to follow the
example; link the owner as well. Prefer a worked example before a case table,
and one case per example. Do not compress a rule into a sentence a reader has
to unpack; two sentences with the terms named beat one with them implied.

When the owner questions a rule in review, record the answer as a design
decision first, then adjust the rule. If the question reveals that two
readers understood a rule differently, the rule needs an example, not only a
reword.
