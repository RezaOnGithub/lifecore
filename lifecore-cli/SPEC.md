# Next CLI Proposal

Status: proposal for the post-refactor CLI iteration.

This document replaces the interaction model in `lifecore-cli/SPEC.md`. It is
not a claim that the current CLI binary already implements this interface. It
keeps the existing safety boundary—validated provisional appends followed by
explicit persistence—but changes how the user names, finds, selects, and
dismisses things.

The command name in examples is `lifecore`, as shorthand for an installed
binary. This workspace continues to publish the interface through the
`lifecore-cli` executable and package. Examples assume the default
`./LifeCoreState.json` state file.

## Decisions in brief

- The user-facing unit is an **entry**, not an event. An entry is a node or an
  edge at one append-log index. A node can supersede an older node; it does not
  edit it.
- A **view** is a bounded history window. Its end is normally the latest entry
  in the effective graph. Freshness and type interpretation are always relative
  to that view.
- `Query` means the core `NodeQuery` concept: it selects nodes in one view. It
  is not yet a general graph language and does not return edges as candidates.
- An entry locator is `INDEX:FINGERPRINT`, for example `42:6b1f0e4c8a7d54b2`.
  The index says which append-log record is intended; the fingerprint is a short
  state-and-index checksum that catches ordinary transcription mistakes and
  locators copied from an independently initialized state.
- A query may be followed by `--select LOCATOR`. The CLI recomputes the query
  and rejects the command unless that locator is in its result. It never trusts
  a row number from an earlier terminal listing.
- A query may instead use `--explain LOCATOR` to show how each predicate
  evaluated for one node, including why that node was not returned.
- `HEAD` is a reserved dynamic node reference: the highest-index node in the
  effective graph, never an alias stored in CLI state and never the latest edge.
- A stored type declaration is a node whose node type is `Type`, matching
  `Type::TYPE_NAME` in the refactored core. CLI text no longer calls it
  `Node_Type`.
- `type override NAME` resolves the unique active declaration with that name in
  a selected view. Query-attested override remains available when additional
  predicates are useful as guards.
- Ordinary `add` fields and sidecar bodies use one recursive TOML payload syntax
  for text, signed integers, sequences, and ordered sets.
- Warnings and suggestions are **messages**. A persisted **dismissal** hides a
  non-fatal message; `barklist` and `bark` are retired terms. An inconsistency
  can never be dismissed.
- `check` renders messages. Dismissal and history-resolution filters affect
  presentation only; message-ID prefixes are resolved before those filters.
- Committing graph mutations may use the optional `--expect-head` guard. Help
  encourages scripts to supply it, but it is never mandatory.

## Vocabulary

| Term      | Meaning                                                                  |
| --------- | ------------------------------------------------------------------------ |
| entry     | One append-log record: a node or an edge.                                |
| index     | The zero-based `EntryIndex` of an entry; it is visible to users.         |
| locator   | An index plus a verified truncated fingerprint, used to name one entry.  |
| `HEAD`    | Reserved reference to the highest-index node in the effective graph.     |
| view      | A `Context`: history from index zero through an exclusive end.           |
| current   | Fresh in the view being discussed; never an eternal property.            |
| supersede | Append a child node whose parent is an older node.                       |
| append    | Add one node or edge to a provisional suffix.                            |
| stage     | Build a provisional `Mock` suffix on a clone.                            |
| analyze   | Run the configured analyzer suite against every valid historical prefix. |
| apply     | Replace a valid `Mock` suffix with the session circumstance.             |
| save      | Atomically persist canonical graph state and CLI state.                  |
| message   | An analyzer diagnostic: Inconsistency, Warning, or Suggestion.           |
| dismissal | CLI-state record that hides one Warning or Suggestion by canonical ID.   |

`EntryContext` remains an implementation type, but it is not a command-line
noun. The user supplies an index when naming an entry and selects a view with a
locator; the CLI constructs the appropriate bounded context. Raw serialized
context pairs are not accepted as input.

## State file and effective graph

The state file remains pretty JSON with two top-level values:

```text
lifecore  canonical LifeCoreList
cli       state-format version, immutable state UUID, entry-fingerprint
          version, message-ID version, aliases, dismissals, optional
          attached-sidecar path, and reserved CLI metadata
```

The first supported CLI state format is version 1. It has no migration or import
path from state files written by an earlier CLI: a missing UUID or version, or
an unsupported version, is a clear load error and never causes an automatic
rewrite. `HEAD` in a persisted alias registry is invalid version-1 state rather
than a name that a migration may reinterpret.

`LIFECORE_STATE_FILE_PATH`, when present, is authoritative. A failure to read or
write that path must not fall back to `./LifeCoreState.json`. `init` is the only
operation that creates a state file and fails if the resolved path already
exists. Persisted graph data must contain no `Mock` circumstance.

`init` generates an immutable UUIDv7 for the state. Moving or copying the state
file preserves the UUID; independently initialized states have different UUIDs.
Ordinary saves, sidecar commits, and format migrations must not silently replace
it. UUIDv7 exposes an approximate initialization time, which is acceptable for
this local state identifier.

An attached sidecar is still an external addendum, not saved graph content. On
ordinary invocation the CLI reads canonical state and builds the sidecar suffix
in memory. A successfully prepared suffix makes the **effective graph**: the
canonical graph followed by the provisional sidecar entries. Read-only graph
operations use that effective graph, including `query`, `entry show`, `check`,
`message show`, `type list`, `type show`, `graphviz`, `sidecar dump`,
`alias list`, and `analyzer list`. A query can therefore return sidecar-added
nodes, and its edge predicates can inspect sidecar-added edges. These operations
never apply or save the suffix.

An attachment forbids state-mutating operations that could otherwise save an
unrelated change alongside the suffix: `add`, `connect`, `type create`,
`type override`, `message apply`, `analyzer add`, `alias set`, `alias remove`,
`message dismiss`, and `message restore`. `sidecar commit` and `sidecar detach`
are the two exceptions. `graphviz` may write its user-requested output file, but
is still read-only with respect to CLI and graph state. A new `sidecar attach`
is accepted only when no sidecar is already attached; replacement requires
detach followed by attach. Detach must only remove attachment metadata; it must
not parse or read the sidecar.

Aliases remain optional conveniences for nodes. They store a durable locator
(the index and full, untruncated digest), not an implicit historical context or
a moving “fresh successor” pointer. An alias continues to name the entry it was
bound to until the user deliberately rebinds it. CLI output always prints a
locator even when it also prints an alias.

## Database lifecycle

Every graph-changing path uses the same transaction state machine. A command
handler must not invent a shortcut around basic validation, full analysis,
`Mock`, or atomic saving.

```text
                         load / deserialize
     [no state file] --------------------------> Loaded
          |                                        |
          | init                                   | clone and start an append plan
          v                                        v
      Initializing ----------------------------> Staged
          |                                      /   |
          | analyze empty graph                 /    | append node/edge
          v                                    /     | basic validation fails
       Analyzed <-------------------------------      v
          | analysis has no Inconsistency          Aborted
          |                                        (working clone discarded)
          | --commit                              ^
          v                                       |
        Applied  -- atomic same-directory save --> Saved
          |       save failure
          +---------------> SaveFailed

  Analyzed -- no --commit --> DryRunComplete (working clone discarded)
  Analyzed -- analysis has an Inconsistency --> Aborted
```

The transitions are:

1. **Loaded** is a successfully deserialized, canonical graph. The CLI rejects a
   loaded graph containing `Mock`.
2. **Staged** is a private clone with one or more newly appended `Mock` entries.
   Each `add_mock` or `connect_mock` immediately passes basic validation. A
   failure discards the entire clone, including earlier appends in the command.
3. **Analyzed** is a staged clone for which the whole configured analysis suite
   ran and produced no Inconsistency. Warnings and Suggestions are output but do
   not block this transition.
4. `--commit` is the only transition from **Analyzed** to **Applied**. `apply`
   replaces provisional circumstances. Without it, the command is a dry run and
   has no state-file effect.
5. **Saved** is reached only after an atomic write through a temporary file in
   the destination directory. A save failure leaves the old state file intact.

All committing graph mutations accept an optional
`--expect-head ENTRY-LOCATOR|EMPTY`.
Immediately before saving, the CLI rereads the canonical state and verifies that
its head still matches that locator, or an explicit empty-state token when
appropriate. A mismatch aborts without changing the state file. This
compare-before-save guard is useful for scripts and long-running clients, but it
is not file locking and is not mandatory. Command help should
recommend it for automation. `HEAD` is not accepted here: the guard needs the
stable locator of the actual latest entry, which may be an edge.

`EMPTY` matches only a canonical graph with no entries. Supplying
`--expect-head` without `--commit` is an error.

`init` follows the same logical route with an empty graph. The bootstrap
Suggestion emitted by TypeCheck is non-fatal. The initial state registers
`Type_Check` and `Interactive` analyzers, but neither name changes the
transaction rules.

CLI-state-only operations use a smaller, separate state machine. They never
apply a graph suffix:

```text
Loaded canonical state -> CLI state edited -> Saved
                         \-> rejected / save failed (no file change)
```

Examples are `alias set`, `alias remove`, `message dismiss`, `message restore`,
`sidecar attach`, and `sidecar detach`. A dismissal is not an analysis result, a
graph entry, or evidence that the underlying problem is fixed.

For an attached sidecar, every invocation has a view-construction state machine:

```text
Loaded canonical -- no attachment --> EffectiveReady (= Loaded view)
       |
       +-- attachment --> parse, stage, and basic-validate sidecar suffix
                          |\
                          | +-- parse or basic-validation failure --> SidecarUnavailable
                          |                                      sidecar detach --> Saved canonical
                          +---- analyze valid suffix
                                  |\
                                  | +-- Inconsistency --> SidecarDiagnosable
                                  |                        |\
                                  |                        | +-- check --> render full diagnostics
                                  |                        +---- sidecar detach --> Saved canonical
                                  +---- no Inconsistency --> EffectiveReady (provisional, read-only)

EffectiveReady -- sidecar commit --> Applied -> Saved, attachment cleared
```

`SidecarUnavailable` permits only `sidecar detach`. `SidecarDiagnosable` keeps
the fully staged, basically valid suffix long enough for `check` to run the
suite over every valid prefix through its final entry and render the resulting
messages; queries and other browsing commands remain unavailable. The
`sidecar detach` edge is deliberately available from both states. It is the
recovery path for a deleted, invalid, or inconsistent sidecar.

Failure to construct the configured analyzer suite, including a missing
factory, is `SidecarUnavailable`: it is an execution failure rather than an
emitted Inconsistency and therefore cannot be rendered as a diagnosable message.

## Entry locators and safe selection

### Locator format

An entry locator has this grammar:

```text
ENTRY-LOCATOR = INDEX ":" FINGERPRINT
INDEX         = one or more ASCII decimal digits
FINGERPRINT   = 5 to 128 lowercase hexadecimal digits
```

The CLI displays the first 16 hexadecimal digits by default and accepts a prefix
of at least 5. For example:

```text
42:6b1f0e4c8a7d54b2
```

The digest is SHA3-512 over a small, versioned checksum input containing:

- a domain and fingerprint-format version;
- the immutable state UUID;
- the entry index;
- an entry-kind tag;
- the stored node or edge type name; and
- for a node, the number of top-level body fields.

The encoding uses fixed-width or length-prefixed components so concatenated
values are unambiguous. An edge and a zero-field node have distinct kind tags;
an edge does not pretend to have zero fields. Circumstance, node field values,
node parents, and edge endpoints are deliberately excluded, so applying a
dry-run suffix does not rename its entries merely by replacing `Mock` with a
session circumstance. The state format records the fingerprint version.

The fingerprint is an ergonomic state-and-index checksum, not a content hash,
tamper-evidence mechanism, or replacement for query and freshness checks. It
does not detect changed field values, parents, or endpoints when the other
inputs remain equal. Copies of one state file intentionally retain compatible
locators; independently initialized states normally reject each other's locators
because their UUIDs differ.

Locator resolution is exact:

1. Parse the index without accepting signs, whitespace, or integer overflow.
2. Load the entry at that index in the current effective graph; reject a missing
   index.
3. Recompute its digest and require it to start with the supplied lowercase
   prefix.
4. If an operation expects a node, require that the resolved entry is a node.
5. If an operation uses a view, require that the index lies inside that view.

A prefix mismatch is an error even though an index alone can technically locate
an entry. Five hexadecimal digits provide only a short human-oriented check; the
CLI displays a longer prefix by default. Message IDs, which lack an index
component, resolve prefixes against their complete unfiltered universe and
refuse ambiguous prefixes.

All new-node and new-edge output includes its locator. `entry show LOCATOR`
accepts either kind and prints kind, index, fingerprint, stored type name,
fields or endpoints, circumstance, and freshness in the requested view.

### Aliases and node references

Aliases are ASCII, case-sensitive C-style identifiers. They and locators have
disjoint grammars:

```text
ALIAS    = [A-Za-z_][A-Za-z0-9_]*
NODE-REF = "HEAD" | ALIAS | ENTRY-LOCATOR
```

`HEAD` is reserved exactly in uppercase; `head` remains an ordinary alias name.
An input containing `:` is never an alias. A syntactically valid locator with a
bad fingerprint is an error, not an alias lookup; `foo:bar` and `42` are invalid
as both a locator and an alias. This makes a `NODE-REF` unambiguous. When it is
an alias, the CLI resolves its stored full locator and then applies the same
node-kind and view checks it would apply to an explicit locator.

Outside sidecar processing, `HEAD` resolves once against the effective graph
loaded for the operation, to the visible node with the greatest entry index. It
errors when that graph has no nodes. It is necessarily current in that view,
because a newer child node would itself be a later node. The resolved locator is
always included in command output. `HEAD` is a convenience for node-reference
positions only; it never means the latest append-log entry, and it is not
accepted where stable entry identity is required.

Node-reference positions such as `add --parent`, `connect --from`, and
`connect --to` accept `NODE-REF` and reject an edge locator. Identity-sensitive
positions always require an `ENTRY-LOCATOR`: `--select`, `--explain`, `--at`,
`entry show`, and `--expect-head`. Scripts should normally use locators; aliases
are an intentionally rebindable convenience.

### Query-attested selection

The old two-invocation, one-based-table convention is removed. A table row has
no durable meaning. A selection is a locator rechecked against the query in the
same invocation:

```text
lifecore query nodes <COMBINATOR> [QUERY OPTIONS]
  [--select ENTRY-LOCATOR [-- <SELECTION ACTION>]]
```

Without `--select`, the command lists the deterministic query result. With it,
the CLI evaluates the query first, resolves the locator, and requires the
selected node’s absolute index to occur in that result. The selected node’s
query context is preserved for display and for the action. Without a selection
action after `--`, the command displays the single selected node.

Initial selection actions are:

- `entry inspect`;
- `alias set NAME`;
- `type override`;
- `add-child TYPE [ADD OPTIONS]`.

`entry inspect` uses the same detailed renderer as standalone `entry show`.
`add-child` accepts ordinary `add` field, commit, and expectation options except
for `--parent`; the selected node is unconditionally its parent.

For example, this rebinds an alias only if entry 42 is still a current `Note`
whose title matches the query:

```console
$ lifecore query nodes all --where fresh --where 'type Note' \
    --where 'field title matches milk' \
    --select 42:6b1f0e4c8a7d54b2 -- alias set buy_milk
```

The CLI errors distinctly for an invalid locator, an entry outside the view, an
entry that is not a node, and a valid node locator that the query did not
return. A selection action does not persist a result set; it is an atomic
query-and-membership guard.

Commands needing two independently selected endpoints, such as `connect`, take
two `NODE-REF` values. Explicit locators are fingerprint-checked, but endpoints
are not pretended to be results of one node query:

```console
$ lifecore connect --from 42:6b1f0e4c8a7d54b2 --type Relates_To \
    --to 49:23aa6e91cddf11a0 --commit
```

## Query

`Query` is CLI construction of `lifecore::NodeQuery`. A query evaluates only
nodes in one bounded view and returns nodes in ascending entry index. It does
not silently inspect the unbounded universe, and it does not query edges as
candidates. Edge predicates are relationships tested on each candidate node.

```text
lifecore query nodes <all|any|none>
  [--at ENTRY-LOCATOR]
  [--where PREDICATE]...
  [--format auto|table|lines|json]
  [--select ENTRY-LOCATOR [-- ACTION...] | --explain ENTRY-LOCATOR]
```

`--at` means “view through and including this entry”; it is valid for a node or
edge locator. Without it, the view ends at the latest entry in the effective
graph. Output states the view end, for example
`view: through 49:23aa6e91cddf11a0`, because freshness can change in a later
view.

`--explain` resolves a node inside the selected view and reports the result of
every predicate plus the final combinator result. Unlike `--select`, the
explained node need not be returned by the query: explaining non-membership is
its primary use. The explanation includes the view used for freshness and edge
predicates. `--select` and `--explain` are mutually exclusive. Core exposes a
structured explanation containing one Boolean in predicate order and the final
combinator result, so the CLI does not duplicate predicate evaluation.

| Combinator | A candidate is returned when   | Empty predicate list   |
| ---------- | ------------------------------ | ---------------------- |
| `all`      | every predicate matches        | every node in the view |
| `any`      | at least one predicate matches | no nodes               |
| `none`     | no predicate matches           | every node in the view |

The empty-list behavior is explicit rather than inherited from an implementation
of `reduce()`. It makes `query nodes all` a useful node listing.

`--where` is repeatable; each predicate is one shell argument. The initial
textual grammar is a thin adapter over core predicates:

```text
fresh
type TYPE-NAME
field FIELD-PATH exists
field FIELD-PATH kind PAYLOAD-KIND
field FIELD-PATH equals-text TEXT
field FIELD-PATH equals-number INTEGER
field FIELD-PATH matches REGEX
field FIELD-PATH sequence-contains-text TEXT
field FIELD-PATH sequence-contains-number INTEGER
field FIELD-PATH sequence-count NATURAL
field FIELD-PATH set-contains-text TEXT
field FIELD-PATH set-contains-number INTEGER
field FIELD-PATH set-count NATURAL
field FIELD-PATH count NATURAL
outgoing EDGE-TYPE
incoming EDGE-TYPE
fresh-outgoing EDGE-TYPE
fresh-incoming EDGE-TYPE
not ( PREDICATE )
```

`FIELD-PATH` uses implemented `FieldQuery::from_str` syntax: a lowercase field
name followed by zero or more `[INDEX]`, `[*]`, or `[last]` components.
`tags[*]`, `history[last][0]`, and `title` are valid paths. A missing path,
wrong payload kind, or out-of-range index is an ordinary non-match, never a
crash. The first CLI exposes text and number equality and collection membership;
whole nested-payload equality awaits a canonical payload-literal syntax.

```console
# All current Tasks with a title containing "invoice".
$ lifecore query nodes all --where fresh --where 'type Task' \
    --where 'field title matches (?i)invoice'

# Nodes that are either current or have a current outgoing Blocks edge.
$ lifecore query nodes any --where fresh --where 'fresh-outgoing Blocks'

# Current Note nodes with no tag equal to "archived".
$ lifecore query nodes all --where fresh --where 'type Note' \
    --where 'not (field tags[*] equals-text archived)'

# Reproduce results just after entry 42.
$ lifecore query nodes all --at 42:6b1f0e4c8a7d54b2 --where fresh

# Explain why one Task was not returned.
$ lifecore query nodes all --where fresh --where 'type Task' \
    --where 'field title matches (?i)invoice' \
    --explain 42:6b1f0e4c8a7d54b2
```

The core query implementation must honor these identities—especially empty
`all`/`none` queries—before this command is exposed. The CLI must not hide an
incorrect core result with a special empty-table case. Missing paths,
wrong-kind payloads, and out-of-range selectors produce no values and ordinary
non-matches; query evaluation must not panic.

## Alias

Aliases are persisted CLI-state conveniences for nodes, not graph entries or
implicit moving pointers. They store the full durable locator of their target,
and remain bound to that exact entry until explicitly rebound. Multiple aliases
may refer to one node. CLI output always includes the locator even when it also
prints aliases.

```text
lifecore alias list [--at ENTRY-LOCATOR]
lifecore alias set NAME ENTRY-LOCATOR
lifecore alias remove NAME
lifecore query nodes ... --select ENTRY-LOCATOR -- alias set NAME
```

`alias set` creates a new binding or deliberately replaces an existing binding.
It requires an explicit node locator, never an alias, so there is no alias
indirection or cycle. The query-attested form binds the selected node only if it
is still a member of that query result. `alias list` shows each target's locator
and current/stale status in the chosen view. A stale target is not automatically
advanced to a successor; the user queries for the intended successor and rebinds
explicitly. With an attached ready sidecar, `alias list` also shows its
temporary aliases, clearly labelled as not yet persisted. `HEAD` is not shown in
that list and cannot be created, removed, rebound, or shadowed by an alias.
Version-1 state containing a persisted alias named `HEAD` fails clearly.

CLI alias metadata is not historical. `alias list --at` therefore lists every
stored alias; a target appended after the selected view has status
`outside-view` rather than current or stale. Such a target does not hide other
aliases or fail the listing.

## `type`: declarations and overrides

`type` is a convenience layer for appending an ordinary node containing
`lifecore::typing::Type`. It does not create a side database or bypass
TypeCheck. The command serializes a `Type` through `NodeBody` and appends a node
whose type name is exactly `Type`.

Before ordinary declarations can be valid, the bootstrap `Type` declaration must
be active. On an empty database, TypeCheck proposes it as a Suggestion; the user
is encouraged to apply that suggestion through the normal message workflow. A
failed TypeCheck run rejects a `type` append just as it rejects every other
append.

```text
lifecore type list [--at ENTRY-LOCATOR]
lifecore type show ENTRY-LOCATOR
lifecore type create NAME [TYPE OPTIONS]
  [--commit [--expect-head ENTRY-LOCATOR]]
lifecore type override NAME [--at ENTRY-LOCATOR] [--as NEW-NAME]
  [PATCH OPTIONS] [--commit [--expect-head ENTRY-LOCATOR]]
lifecore query nodes ... --select ENTRY-LOCATOR -- type override
  [--as NEW-NAME] [PATCH OPTIONS] [--commit [--expect-head ENTRY-LOCATOR]]
```

`type list` displays current, well-formed stored declarations in name order and
then their locators. `type show` requires a locator for a `Type` node,
deserializes it with `Type::try_from_body`, and prints a copy-pasteable
declaration. `type create` starts with an empty declaration named `NAME`; a
duplicate active name is rejected by analysis.

`type override NAME` resolves the unique active, well-formed declaration named
`NAME` in the view selected by `--at`, or in the latest view when `--at` is
absent. TypeCheck-valid views contain at most one active well-formed declaration
with a given name. A name does not identify every historical declaration with
that name, so the view remains part of resolution.

The source need only be fresh in the selected historical view. It may already be
stale in the latest view, which permits another branch from an older
declaration. Query-attested override remains available for advanced workflows
that want predicates beyond declaration name and view.

`TYPE OPTIONS` are repeatable:

```text
--field 'FIELD-DECL'
--outgoing EDGE-TYPE
--incoming EDGE-TYPE
--allows-override TYPE-NAME
```

`FIELD-DECL` is parsed by implemented `FieldDecl::from_str`, which in turn uses
`ValueType::from_str` and `FieldStatus::from_str`. Its canonical form is:

```text
field_name = VALUE-TYPE : STATUS
```

Field names start with lowercase ASCII and continue with lowercase ASCII,
digits, or underscores. `STATUS` is `Mandatory`, `Encouraged`, `Deprecated`, or
`Optional`; lowercase forms are accepted. `VALUE-TYPE` accepts `PreciseTime`,
`Text`, `Number`, text and signed-number literals, `Sequence(...)`,
`Tuple(...)`, `OrderedSet(...)`, and `OneOf(...)`. Whitespace around punctuation
is permitted. Text literals are double quoted and presently have no escape
syntax, so a declaration containing a literal double quote is rejected rather
than silently changed.

```console
$ lifecore type create Task \
    --field 'title = Text : Mandatory' \
    --field 'status = OneOf("Todo", "Done") : Mandatory' \
    --field 'tags = OrderedSet(Text) : Encouraged' \
    --outgoing Blocks --incoming Blocked_By --allows-override ArchivedTask \
    --commit
```

### Overrides are patch-based

The declaration resolved by `type override` must be current in the chosen view,
well-formed, and stored in a node whose type is `Type`. The command copies that
stored `Type`, applies the patch, and appends it as a child of the resolved
declaration. The user never has to retype a whole schema to add a field.

The declaration name is preserved by default. `--as NEW-NAME` instead writes the
child with a different declared name. This supports branching and migration from
an older declaration, including one that is stale in the latest view. If a
differently named child supersedes a declaration that has no same-name active
successor, the old name may disappear from the active type table. TypeCheck then
decides whether existing nodes, duplicate names, and override permissions make
the candidate valid; the CLI does not guess or silently create a second branch.

```text
--field 'FIELD-DECL'              replace/add the named field
--drop-field FIELD-NAME           remove the named field
--allow-outgoing EDGE-TYPE        add an outgoing edge permission
--disallow-outgoing EDGE-TYPE     remove an outgoing edge permission
--allow-incoming EDGE-TYPE        add an incoming edge permission
--disallow-incoming EDGE-TYPE     remove an incoming edge permission
--allows-override TYPE-NAME       add a permitted child node type
--disallow-override TYPE-NAME     remove a permitted child node type
```

The parser rejects conflicting edits in one command, such as both `--field` and
`--drop-field` for `title`, or both allow and disallow for one edge name.
Replacing a field preserves its previous display position; newly added fields
and permissions append in option order. A no-op patch is rejected so history is
not cluttered by identical type versions.

```console
# Resolve the latest active Task declaration by name and append its successor.
$ lifecore type override Task \
    --field 'priority = Number : Optional' \
    --allow-outgoing Relates_To --commit

# The query-attested form remains available when extra membership guards matter.
$ lifecore query nodes all --where fresh --where 'type Type' \
    --where 'field name equals-text Task' \
    --select 12:45a8e2d41f7c9b0e -- type override \
    --field 'priority = Number : Optional' \
    --allow-outgoing Relates_To --commit

# Branch from the declaration that was active in a historical view.
$ lifecore type override Task --at 12:45a8e2d41f7c9b0e \
    --as FocusedTask --drop-field legacy_scope --commit
```

This append enters **Staged**, so all existing fresh `Task` nodes are checked
against the proposed active declaration before commit. A new mandatory field can
therefore make the command fail with TypeCheck inconsistencies. This is
intentional: `type override` makes the parent relationship convenient, not
unsafe.

## Ordinary appends

The basic commands remain small and explicit:

```text
lifecore add TYPE [--field 'FIELD-ASSIGNMENT']...
  [--parent NODE-REF]
  [--commit [--expect-head ENTRY-LOCATOR]]

lifecore connect --from NODE-REF --type EDGE-TYPE --to NODE-REF
  [--commit [--expect-head ENTRY-LOCATOR]]
```

`add --parent` and both `connect` ends require node references visible in the
current effective graph. A child normally has its parent’s node type; a
different type is allowed only when the active parent declaration’s
`additional_override` permits it. TypeCheck, not the CLI, owns that semantic
decision. Both commands print all candidate diagnostics and mark output as
`dry run` or `committed`.

### Shared TOML payload input

`add --field` and sidecar entry bodies use the same recursive TOML-to-`Payload`
conversion. The supported grammar is:

```text
PAYLOAD = TOML-STRING
        | TOML-SIGNED-INTEGER
        | { sequence = [PAYLOAD, ...] }
        | { ordered_set = [PAYLOAD, ...] }
```

`sequence` maps to `Payload::Sequence`, including payloads checked against a
declared `Tuple(...)` value type. There is no `tuple` payload constructor.
`ordered_set` maps to `Payload::OrderedSet` and communicates that insertion
order is retained. Duplicate ordered-set elements are rejected rather than
silently discarded.

Booleans, floats, bare arrays, arbitrary inline tables, TOML date-time values,
and unknown constructor keys are errors. User-authored precise-time, civil-time,
and duration values are deferred to the date/time RFC below.

Each `FIELD-ASSIGNMENT` is parsed as a TOML document with exactly one root key
whose value is one supported `PAYLOAD`. Repeating the same field name in one
command is an error rather than a last-value-wins rule.

```console
$ lifecore add Note \
    --field 'tags = { ordered_set = ["home", "errand"] }' \
    --field 'brand_preference = { sequence = [
      { sequence = ["BrandA", "Humane", "Local"] },
      { sequence = ["BrandB", "Unknown", "Local"] }
    ] }' \
    --commit
```

A sidecar uses this identical converter; its entry and reference syntax is
specified below. Parsing produces payload values only; TypeCheck remains
responsible for determining whether those values match the active declaration.

## Sidecars

A sidecar is an attached TOML addendum whose `[[entry]]` tables run in source
order. Each entry appends one node, then its `connect_to` edges, then its
`connect_back_from` edges. All references are `NODE-REF` strings, so they may be
durable locators, aliases, or `HEAD`.

```text
lifecore sidecar attach PATH
lifecore sidecar detach
lifecore sidecar dump
lifecore sidecar commit [--expect-head ENTRY-LOCATOR]
```

```toml
[[entry]]
alias = "BuyMilk" # optional temporary alias for this new node
ty = "Note"
parent = "11:6b1f0e4c8a7d54b2" # optional NODE-REF

[entry.body]
tags = { ordered_set = ["home", "errand"] }
brand_preference = { sequence = [
    { sequence = ["BrandA", "Humane", "Local"] },
    { sequence = ["BrandB", "Unknown", "Local"] },
] }

[entry.connect]
connect_to = [{ target = "AtWork", ty = "Related" }]
connect_back_from = [{ target = "DairyContrib", ty = "Accumulate" }]
```

`ty` is mandatory. `alias`, `parent`, `body`, and `connect` are optional.
`connect_to` places the new node at the edge head; `connect_back_from` places it
at the tail. The body uses the shared recursive TOML payload grammar. A
sidecar-defined alias is temporary: later entries may use it, but it is not
saved until a successful `sidecar commit`. It may not shadow a persisted alias
or an alias introduced by an earlier entry, and it may not be named `HEAD`.

At the start of each sidecar entry, the CLI captures that entry's `HEAD` from
the graph snapshot immediately before its node is appended. Thus `HEAD` in the
first entry means the latest canonical node, while `HEAD` in a later entry may
mean a node added by an earlier entry. The captured value does not change after
the new node is appended, so a connection target of `HEAD` does not unexpectedly
become that entry's own new node. Other references retain their natural staging
time: the parent resolves before the new node, and connection targets resolve
after its optional temporary alias is introduced. This preserves intentional
self-connections written through that alias.

`sidecar attach` resolves `PATH` to an absolute existing path and persists only
that metadata; it deliberately does not parse the file. `sidecar detach` removes
only that metadata and never reads the sidecar. `sidecar commit` rebuilds the
suffix from canonical state, validates and analyzes it, applies it, saves the
new persistent aliases together with the graph, and clears the attachment. The
source TOML file is never deleted.

`sidecar dump` is a read-only preview of the prepared provisional suffix, not a
dump of TOML source. It prints only sidecar-appended nodes and edges in append
order, with their predicted locators, parent/body or endpoints, and temporary
aliases. It requires `EffectiveReady`; it reports an error rather than an empty
list when no sidecar is attached. Because locators exclude circumstance,
locators shown by `dump` remain valid after `commit`.

## Analyzer configuration

The configured analysis suite remains inspectable and extendable without
creating a node or edge:

```text
lifecore analyzer list
lifecore analyzer add NAME CONFIG [--commit [--expect-head ENTRY-LOCATOR]]
```

`analyzer list` displays configured name/configuration pairs in their stored
order. `analyzer add` stages a cloned suite, then runs the complete candidate
suite before it may save. A missing registered factory therefore rejects the
candidate during analysis rather than creating unusable persisted configuration.
It follows the same dry-run, atomic-save, and `--expect-head` rules as other
committing operations, but is a suite configuration change rather than an
append-log entry.

## GraphViz

```text
lifecore graphviz TARGET [--at ENTRY-LOCATOR] [--full] [--fields] [--dot]
```

GraphViz renders the selected effective-graph view and does not modify CLI or
graph state. By default it renders fresh nodes and edges. `--full` instead
renders all nodes and edges visible in the view, organizes node families into
clusters, and draws parent-child relationships as dashed special edges.

Nodes use GraphViz `mrecord` labels containing their locator, stored type name,
visible aliases, and, with `--fields`, their field dump. `--dot` writes the DOT
definition directly to `TARGET` and does not invoke GraphViz. Otherwise the CLI
streams the generated DOT to `dot -Tsvg` on standard input and writes the SVG to
`TARGET`, without creating a temporary DOT file. A requested visualization file
is an output artifact, not a state mutation.

## Messages and dismissal

An analyzer **emits** a Message when the complete analysis run over the current
effective graph produces it for any analyzed historical prefix. Emission is
computed anew on each invocation; messages are not journaled.

The **message universe** is the set of distinct full message IDs emitted by that
analysis run before dismissal and resolved-by-history presentation filters are
applied. It therefore includes Inconsistencies, visible and dismissed Warnings
and Suggestions, and messages computed as resolved by history.

Messages producing the same complete ID are collapsed before prefix resolution
or presentation. Message-ID version 1 uses a tagged, length-prefixed canonical
binary encoding with explicit collection lengths and recursive payload tags.

The CLI renders messages deterministically by offending location (context end,
then entry index), severity, analyzer name/configuration, text, and canonical
message ID. Output includes analyzer, severity, location, relevance, text,
action summary for Suggestions, and a short handle:

```text
m:77ef2c15985edb1a  Warning  Type_Check  entry 42:6b1f0e4c8a7d54b2  ...
```

A message ID is SHA3-512 over a versioned canonical representation of severity,
analyzer name/configuration, offending location, text, normalized relevance,
and, for Suggestions, ordered actions. Relevance is sorted only for identity;
action order remains significant. The persisted dismissal registry contains the
full digest and ID-format version, never a display prefix or rendered text.

```text
lifecore check [--include-dismissed | --only-dismissed] [--include-resolved]
lifecore check --dismissal-ids
lifecore message show MESSAGE-ID
lifecore message dismiss MESSAGE-ID
lifecore message restore MESSAGE-ID
lifecore message apply MESSAGE-ID [--commit [--expect-head ENTRY-LOCATOR]]
  [--keep-message]
```

`message list` is replaced by `check`. By default, `check` shows emitted,
active, non-dismissed messages plus every Inconsistency. `--include-dismissed`
also shows emitted dismissed messages, while `--only-dismissed` restricts output
to emitted dismissed messages. Those two flags are mutually exclusive.
`--include-resolved` is orthogonal and admits emitted messages whose relevance
has been resolved by later history.

`check --dismissal-ids` reads the persisted registry instead of rendering the
message universe. It lists every dismissed full ID and whether that ID matches a
message emitted today. This includes orphaned dismissals whose original message
is no longer emitted and whose old rendered body is therefore unavailable. It is
a standalone mode and does not combine with the presentation filters above.

Because an Inconsistency is never presentation-filtered,
`check --only-dismissed` still includes every emitted Inconsistency alongside
emitted dismissed non-fatal messages. Registry-only `check --dismissal-ids` is
the sole exception because it does not render the message universe.

`MESSAGE-ID` is `m:` followed by 5 to 128 lowercase hexadecimal digits. `show`,
`dismiss`, and `apply` resolve a prefix against the entire message universe,
before any presentation filtering, and require exactly one distinct full digest.
A visible message and a dismissed or resolved message can therefore make the
same short prefix ambiguous. `restore` resolves against the union of the message
universe and the persisted dismissal registry so an orphaned dismissal can still
be removed.

“Entire message universe” means every message reproducible by the current
analyzer run, not every message ever observed. A no-longer-emitted message is
known only by its full persisted dismissal ID when it was previously dismissed.

Emission, resolution, and dismissal are separate axes rather than one linear
message state machine:

```text
Computed analyzer/presentation state:

Not emitted <----> Emitted and active <----> Emitted and resolved by history

Persisted dismissal state for a Warning or Suggestion:

Not dismissed -------- message dismiss --------> Dismissed (full ID saved)
Not dismissed <-------- message restore -------- Dismissed

Suggestion application:

Emitted Suggestion -- apply --> candidate Staged and Analyzed
candidate dry run or failure --> graph and dismissal registry unchanged
candidate --commit --> graph Saved; full ID dismissed unless --keep-message
```

`message dismiss` is a CLI-state-only save. It rejects Inconsistencies and never
modifies the graph. `message apply` follows the full graph lifecycle. On a
successful commit it dismisses the applied Suggestion unless `--keep-message`
was supplied; an already registered dismissal is not implicitly restored. A dry
run never creates a dismissal.

Dismissing an already-dismissed Warning or Suggestion is idempotent. Restoring
requires a matching persisted dismissal; a currently emitted but non-dismissed
message is not sufficient.

Inconsistencies are always visible and always block graph commit. A Warning or
Suggestion can additionally become **resolved by history** when its specific
offending entry and every relevant entry are stale in the latest view. That is a
computed presentation state, not a persisted dismissal. A context-only message,
or one without entry relevance, is not silently declared resolved.
`--include-resolved` exposes the computed history.

## Built-in `Interactive` analyzer

`init` registers the built-in `Interactive` analyzer alongside `Type_Check`.
When an analyzed window ends in a child node, `Interactive` considers the
child's parent connections and may suggest recreating eligible incoming or
outgoing connections for the child. It suppresses a suggestion when the opposite
endpoint is stale or when a fresh successor already has the equivalent
connection. These are ordinary Suggestions: they are visible through `check` and
are applied only through `message apply`.

## Output, errors, and determinism

`--format auto|table|lines|json` applies to entries, queries, types, aliases,
analyzer configuration, sidecar dumps, messages, and transaction results. `auto`
chooses a table only for a TTY. JSON includes full locator and message digests,
display prefixes, view boundaries, message visibility state, and transaction
state. GraphViz has its own DOT or SVG target-file output.

JSON listings use the envelope `{ "view": ..., "items": [...] }`. Transaction
JSON contains the applicable view, transaction state, predicted or committed
entries, and messages. Transaction diagnostics use ordinary message visibility
rules, with every Inconsistency retained.

Presentation order is never incidental hash-map order:

- query results sort by entry index ascending;
- type listings sort by declaration name then entry index;
- aliases sort by name;
- analyzer configuration retains stored order;
- fields and permissions retain declaration insertion order;
- messages use the order specified above; and
- sidecar entries retain TOML source order.

Errors should name the broken invariant and include both supplied and actual
locator prefix when possible. Useful distinct errors include
`unknown entry
index`, `entry fingerprint mismatch`, `entry is not a node`,
`entry is outside
the selected view`,
`selected entry was not returned by query`,
`selected node
is not a valid Type declaration`,
`field declaration syntax invalid`, and `message ID is ambiguous`. No such
failure may panic or save partial state.

## Suggested extensions and where they fit

These are not prerequisites for the first implementation.

### TTY client RFC

A future menu-driven TTY client requires a new specification amendment; it is
not part of the accepted command surface. Such a client should reuse the batch
services and transaction boundaries. It is unrelated to the built-in
`Interactive` analyzer, whose name describes connection-recreation Suggestions
and does not imply terminal behavior.

### Type-impact and health summaries RFC

Extend a future TTY client's review screen and batch `type override --summary`
to report nodes and edges whose TypeCheck outcome changes under the proposed
declaration. It belongs after **Analyzed**, because only the real provisional
graph and analyzer output can determine impact. It is a presentation summary
over existing diagnostics, not a second schema checker.

`check --summary` could group current health by severity, analyzer name and
configuration, stored node or edge type, and count, with an explicit
`unattributed` bucket for context-level diagnostics. Both summary forms remain
RFCs because generic Messages do not yet carry structured logical-type subjects.
Inferring a type from diagnostic prose is unacceptable.

Basic validation currently returns one failure rather than a diagnostic
collection. A health summary may report that failure and skip analyzers that
require a valid graph, but reporting every validation failure would require a
separate core API change.

### Date, civil-time, and duration input RFC

The first shared TOML payload grammar deliberately has no constructor for
`PreciseTime` and does not interpret TOML date-time values. Existing stored
`PreciseTime` payloads still need deterministic display and internal
fingerprinting, but the CLI and sidecars do not author new ones yet.

An RFC should design core vocabulary and CLI validation together. It must
distinguish absolute instants from civil dates, civil times, civil date-times,
and zoned date-times; distinguish fixed durations from calendar spans; define
timezone and daylight-saving ambiguity policies; and specify storage,
round-trippable display, equality, ordering, query predicates, and migration
from the existing `PreciseTime` variant. Unsupported time input should produce a
deliberate error rather than silently becoming text.

### Circumstance provenance display RFC

`Circumstance::Session` currently stores an hour-rounded zoned time, hostname,
and username. These values are unverified and equal circumstances are
deduplicated, so they are coarse provenance rather than an author identity,
exact creation time, transaction boundary, or security audit record.

`entry show` already includes the circumstance in its detailed output. A later
`--show-provenance` option could add a compact `recorded hour` and
`username@hostname` column to query and entry listings. JSON should use
structured fields and label the data unverified. Staged entries display
`provisional; circumstance assigned on commit` instead of presenting `Mock` as
persisted provenance.

Circumstance remains outside entry fingerprints and has no effect on graph
semantics. Circumstance query predicates, privacy controls, exact instants, and
a unique commit/session identifier require a separate design before they are
exposed.

### Saved named queries

Later, CLI state may store a name, `NodeQuery`-compatible predicates, and a view
policy such as `latest`. Execution must still construct a bounded view at run
time and require a locator for mutation. This belongs in CLI metadata beside
aliases, never as an implicit mutable result set; `--select` remains the
membership guard.

## Deferred implementation boundaries and acceptance criteria

The exact module boundaries remain an RFC for the implementation phase. The
following split records likely responsibilities without making them part of the
accepted command interface.

Command handlers should share orchestration rather than stage their own graphs.
A practical module split is:

- `state`: state-file resolution, persisted state, `Mock` rejection, atomic
  save, immutable state UUID, and format versions;
- `transaction`: lifecycle state machine and append/analyze/apply helpers;
- `locator`: versioned state-and-index checksum encoding, SHA3-512 prefix
  parsing, and view-aware resolution;
- `query`: predicate parsing, `NodeQuery` construction, structured explanation,
  rendering, and selection;
- `type_command`: `FieldDecl` parsing, `Type` construction and patching;
- `payload_input`: shared TOML assignment and recursive payload conversion;
- `analyzer_command`: configured-suite listing and cloned-suite staging;
- `messages`: canonical IDs, emission universe, dismissal registry, visibility,
  `check`, and suggestion apply;
- existing `sidecar` and `graphviz` modules adapted to locators and views.

The proposal is ready to claim implementation only when tests cover: dry-run
append leaves state-file bytes unchanged; committed state has no `Mock`; basic
validation and analyzer failures roll back the whole candidate; a correct index
with wrong digest is rejected; aliases and locators are lexically disjoint and a
bad locator fingerprint never falls back to alias lookup; `--select` rejects a
locator absent from its query result; `--explain` reports non-membership without
selecting it; direct type override resolves the declaration by name and view,
uses it as parent, and preserves its name unless `--as` is supplied; malformed
or duplicate TOML field assignments never append; nested sequences and ordered
sets round-trip through sidecars; a valid attached sidecar is visible to query,
entry inspection, type inspection, GraphViz, and `check`; `check` renders an
inconsistent but basically valid sidecar through its final entry while ordinary
browsing remains unavailable; `sidecar dump` preserves append order and its
predicted locators remain valid after commit; an Inconsistency cannot be
dismissed; a dismissal survives reload; message-prefix ambiguity is detected
across visible, dismissed, and resolved messages; an unknown analyzer factory
does not persist configuration; `HEAD` resolves only to the latest node, is
rejected when no node exists or stable entry identity is required, and resolves
against the pre-node snapshot of each sidecar entry; `--expect-head` mismatch
saves nothing; and an invalid attached sidecar can still be detached without
being read.
