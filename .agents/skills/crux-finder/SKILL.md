---
name: crux-finder
description: Collaboratively pressure-test fuzzy requirements to uncover the contradiction, missing distinction, or uncertainty that governs a problem. Use when a user wants adversarial brainstorming, a shared requirement-and-concern map, an independent re-entry review, or a final sweep after the crux has been found. Keep the method domain-neutral; do not use it for straightforward task decomposition after requirements are already settled.
---

# Crux Finder

Build shared understanding before committing to a plan. Maintain a light
human-editable map of requirements, concerns, and possible answers while
searching energetically, but not dogmatically, for the crux.

## Start or resume the exploration

- Use the user's existing document when one exists. Read it in full before
  editing it, and preserve human edits and distinctive wording.
- When the user requests a shared artifact and gives no path, use
  `CRUX_MAP.md`. Copy `assets/crux-map-template.md` when creating it.
- Keep the discussion conversational when a file would add ceremony without
  improving collaboration.
- Read primary source material that bears on the requirements. Keep facts,
  interpretations, and speculative concerns distinguishable in plain language.

Treat the map as an exploratory surface, not a specification or decision log.
Requirements may be fuzzy, contradictory, duplicated, or later abandoned.

## Maintain the map

Use this shape:

1. State each requirement as a short heading, ideally one line.
2. Put concerns, counterexamples, conflicts, assumptions, and consequences
   directly beneath the requirement as ordinary bullets, ideally no more than
   two lines.
3. Nest possible solutions, counterpoints, synergies, workarounds, experiments,
   or deferrals beneath the concern they answer.

Use simple numbering only to make cross-references easy. Avoid status fields,
tag vocabularies, tables, owners, scoring, and mandatory metadata unless the
user's domain genuinely needs them. Do not mechanically give every requirement
the same number of concerns or answers.

Add material where a human reader would expect to find it. For a conflict
between two requirements, explain the collision beneath the requirement where
it is easiest to understand and refer to the other by number. Duplicate the
point only when each placement reveals a different consequence.

## Apply constructive pressure

Try hard to break each important requirement without becoming performatively
negative. Look for:

- a concrete counterexample;
- another desirable requirement that it frustrates;
- an ambiguity whose interpretations lead to different outcomes;
- a hidden assumption or displaced cost;
- a boundary of actor, scale, time, or environment where it becomes harmful;
- an unacceptable consequence; or
- a reason an attractive answer would fail.

Prefer specific collisions over generic cautions such as "this may be hard."
Develop the strongest reasonable version of the requirement before criticizing
it. Add promising answers beneath concerns, but do not silently treat an answer
as agreement.

## Concentrate and diffuse

When a contradiction appears, sharpen it:

- isolate the smallest pair of claims that cannot both hold;
- give a circumstance in which they collide;
- test whether different meanings or scopes merely make them appear
  incompatible; and
- preserve why both sides remain desirable.

Do not invent a compromise to restore consistency. When no worthwhile
trade-off is visible, say so plainly and leave the contradiction open. Broaden
the search across actors, time, scale, scope, assumptions, underlying aims,
sequencing, separation, and unexpected alternatives. Let diffuse exploration
produce sharper tests rather than demanding an immediate answer.

Return to focused contradiction-seeking when the wider exploration exposes a
more precise collision. Be eager to find a singular incompatibility, but do not
manufacture one: several diffuse tensions or no identifiable crux are valid
outcomes.

## Propose a candidate crux

Add or revise the optional **Candidate crux** only when a compact statement:

- explains several concerns;
- exposes a consequential incompatibility, missing distinction, or
  uncertainty;
- would simplify the discussion if understood; and
- cannot be dissolved by cosmetic rewording.

State it as a provisional observation, often in the form "we want X and Y, but
the apparent means of achieving X undermines Y." Continue testing it. Do not
declare the crux found merely because the document has become long.

## Join midway

When entering an existing exploration:

1. Read the original question, the full map, relevant sources, and any candidate
   crux.
2. Reconstruct the central tension independently before accepting the previous
   framing.
3. State the strongest missed contradiction or explain why the candidate crux
   does not organize the concerns.
4. Continue the map rather than replacing it with a summary.

Preserve genuine disagreement. Do not manufacture consensus between the user,
earlier agents, reviewers, or source material.

## Sweep only after discovery

Do not sweep merely because the map is untidy, repetitive, or large. Sweep only
when the user requests it after believing that the crux or a useful reframing
has been found. If the exploration still lacks a useful center, say that it is
not ready.

Infer what survived from the discussion; do not require participants to
maintain statuses during exploration. Recommend the smallest suitable outcome:

- replace the map with a fresh working document;
- preserve the map and create a separate working document;
- retain only a short account of the crux; or
- discard the artifact when the original framing has dissolved.

For a durable working document, prefer:

```markdown
# [Subject]

## Aim

## Crux

## Working requirements

## Unresolved tensions

## Paths not carried forward
```

Include **Paths not carried forward** only for ideas likely to recur or whose
rejection taught something important. Explain why they were not carried
forward without presenting them as forbidden future work. Preserve unresolved
disagreement and relevant evidence, but do not preserve the exploratory tree
merely for completeness.

Treat replacement and deletion as user decisions. Never destroy the shared map
without explicit approval.
