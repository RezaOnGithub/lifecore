> [!NOTE]
> The development is on
> [`rust-sketch01` branch](https://github.com/RezaOnGithub/lifecore/tree/rust-sketch01).
> This document is a WIP collection of ideas and introduces the basic concepts.
> At some point the sketch should get squashed and merged here, and effectively
> graduate to an actual alpha release.

# LifeCore: A Personal Graph Database

LifeCore is a typed graph database with basic version-control features,
primarily for "personal database"-type applications. Think Obsidian, with a bit
more structure but deliberately less flexibility. Additionally, it is aimed to
be a shared database between different personal productivity packages; e.g. your
notes could link to project management-related nodes, or semantic graph-style
concept nodes, or just other notes.

It intends to achieve this by building in a few core rules, and implementing
more complicated functionality as "Analyzer" plug-ins.

# Core Rules (aka "Basic Validation")

## Basic Constructs

- **Index**: All Nodes and Edges have a unique index, and are strictly ordered,
  which depends on the insertion order.
- **Node**: Has a `type` string, a map of strings to values ("Payload"), and
  optionally a `parent` index.
- **Edge**: Has a `from` index, `to` index, and a `type` string.

### Parents, Override, "Fresh" vs "Stale"

Nodes have a very minimal, single-parent VCS mechanism built into them. A node
that has a non-empty `parent` index is said to _override_ the node referred to
by that index. The "parent" node is considered _stale_ if it has a "child" node.
A node is considered _fresh_ if it is not a parent to any other node.

Note that this mechanism does not forbid multiple nodes from having the same
parent. Thus, the history self-organizes into a tree.

### Edges Are Lightweight

Earlier designs had edges as first-class entities that would have additional
fields just like nodes. Another design had gotten rid of edges in favor of
"connect" lists on the nodes themselves. The current design carries over edges,
but said edges are entirely "lightweight". They do not have versions, and do not
have additional fields.

### Stringly-Typed, With Caveats

All types are specified as strings, and by default, don't do anything. Now, all
type-checking is its own "Analyzer". (the ability to just turn it off +
dogfooding the Analyzer interface)

## 0. Indices Must Point To Entries Of Correct Types

- Edge `from` and `to` must point to different nodes.
- Node `parent` must point to a node, or be empty.

## 1. Duplicate Edges Are Forbidden, Duplicate Nodes Are Allowed Only Under A Condition

- The set of fresh nodes at any point in time must not contain a duplicate. That
  is, an exact duplicate (with the same parentage, types, and fields) is only
  allowed if the original is currently stale.

## 2. Entries Are Sorted And Temporally Consistent

- An edge may only connect nodes with an index lower than its index.

- A node may only have a parent that is older than itself (and thus has a lower
  index)

## 3. **Stale Rule**: After a node has been overriden, no new edges can connect to it.

This is the core part of the ruleset. A node that is stale cannot form new
connections.
