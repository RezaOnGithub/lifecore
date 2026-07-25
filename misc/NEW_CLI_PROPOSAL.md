# Next CLI Proposal

Status: proposal for the post-refactor CLI iteration.

This document replaces the interaction model in `lifecore-cli/SPEC.md`. It is
not a claim that the current CLI binary already implements this interface. It
keeps the existing safety boundary—validated provisional appends followed by
explicit persistence—but changes how the user names, finds, selects, and
dismisses things.

The command name in examples is `lifecore`. Examples assume the default
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
  The index says which append-log record is intended; the fingerprint detects a
  wrong database, stale copied output, or mistyped index.
- A query may be followed by `--select LOCATOR`. The CLI recomputes the query
  and rejects the command unless that locator is in its result. It never trusts
  a row number from an earlier terminal listing.
- A stored type declaration is a node whose node type is `Type`, matching
  `Type::TYPE_NAME` in the refactored core. CLI text no longer calls it
  `Node_Type`.
- Warnings and suggestions are **messages**. A persisted **dismissal** hides a
  non-fatal message; `barklist` and `bark` are retired terms. An inconsistency
  can never be dismissed.
- `-i` starts a small interactive mode for browsing, selection, type editing,
  suggestion application, and message dismissal. It is not a second command
  language or an interactive version of every batch feature.

## Vocabulary

| Term      | Meaning                                                                  |
| --------- | ------------------------------------------------------------------------ |
| entry     | One append-log record: a node or an edge.                                |
| index     | The zero-based `EntryIndex` of an entry; it is visible to users.         |
| locator   | An index plus a verified truncated fingerprint, used to name one entry.  |
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
cli       aliases, dismissals, optional attached-sidecar path, and CLI metadata
```

`LIFECORE_STATE_FILE_PATH`, when present, is authoritative. A failure to read or
write that path must not fall back to `./LifeCoreState.json`. `init` is the only
operation that creates a state file and fails if the resolved path already
exists. Persisted graph data must contain no `Mock` circumstance.

An attached sidecar is still an external addendum, not saved graph content. On
ordinary invocation the CLI reads canonical state and builds the sidecar suffix
in memory. Read operations see that **effective graph**. An attachment therefore
still forbids ordinary appends, analyzer changes, aliases, dismissals, and
suggestion application; `sidecar commit` and `sidecar detach` remain the two
escape routes. Detach must only remove attachment metadata; it must not parse or
read the sidecar.

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
       +-- attachment --> build provisional sidecar suffix
                          | validation or analysis failure
                          v
                      SidecarUnavailable
                          |\
                          | \-- other commands fail
                          +---- sidecar detach --> Saved canonical

build provisional sidecar suffix -- valid --> EffectiveReady (provisional, read-only)
EffectiveReady -- sidecar commit --> Applied -> Saved, attachment cleared
```

The `sidecar detach` edge is deliberately available from **SidecarUnavailable**.
It is the recovery path for a deleted or invalid sidecar.

## Entry locators and safe selection

### Locator format

An entry locator has this grammar:

```text
ENTRY-LOCATOR = INDEX ":" FINGERPRINT
INDEX         = one or more ASCII decimal digits
FINGERPRINT   = 12 to 128 lowercase hexadecimal digits
```

The CLI displays the first 16 hexadecimal digits by default and accepts a prefix
of at least 12. For example:

```text
42:6b1f0e4c8a7d54b2
```

The digest is SHA3-512 over a versioned canonical **entry witness**. The witness
contains entry kind; node type and parent or edge type and endpoints; and
node-body keys and payloads in canonical recursive order. It does not contain
the circumstance, so applying a dry-run suffix does not rename its graph entries
merely by changing `Mock` to a session circumstance. Lengths and variant tags
are encoded, so strings and payload variants cannot collide by concatenation.
The state format records the witness version.

The index makes the pair unique in one append-only database; the fingerprint is
an integrity and intent check, not a replacement for the index. Locator
resolution is exact:

1. Parse the index without accepting signs, whitespace, or integer overflow.
2. Load the entry at that index in the current effective graph; reject a missing
   index.
3. Recompute its digest and require it to start with the supplied lowercase
   prefix.
4. If an operation expects a node, require that the resolved entry is a node.
5. If an operation uses a view, require that the index lies inside that view.

A prefix mismatch is an error even though an index alone can technically locate
an entry. A 12-digit digest collision does not weaken identity because the index
is still checked. Message IDs, which lack an index component, refuse ambiguous
prefixes.

All new-node and new-edge output includes its locator. `entry show LOCATOR`
accepts either kind and prints kind, index, fingerprint, stored type name,
fields or endpoints, circumstance, and freshness in the requested view.
Node-only positions such as `add --parent`, `connect --from`, and `connect --to`
reject an edge locator rather than guessing.

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
- `add --parent-selected`.

For example, this rebinds an alias only if entry 42 is still a current `Note`
whose title matches the query:

```console
$ lifecore query nodes all --where fresh --where 'type Note' \
    --where 'field title matches milk' \
    --select 42:6b1f0e4c8a7d54b2 -- alias set buy-milk
```

The CLI errors distinctly for an invalid locator, an entry outside the view, an
entry that is not a node, and a valid node locator that the query did not
return. A selection action does not persist a result set; it is an atomic
query-and-membership guard.

Commands needing two independently selected endpoints, such as `connect`, take
two normal locators. They are fingerprint-checked but are not pretended to be
results of one node query:

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
  [--select ENTRY-LOCATOR [-- ACTION...]]
```

`--at` means “view through and including this entry”; it is valid for a node or
edge locator. Without it, the view ends at the latest entry in the effective
graph. Output states the view end, for example
`view: through 49:23aa6e91cddf11a0`, because freshness can change in a later
view.

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
```

The core query implementation must honor these identities—especially empty
`all`/`none` queries—before this command is exposed. The CLI must not hide an
incorrect core result with a special empty-table case.

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
lifecore type create NAME [TYPE OPTIONS] [--commit]
lifecore query nodes ... --select ENTRY-LOCATOR -- type override [PATCH OPTIONS] [--commit]
```

`type list` displays current, well-formed stored declarations in name order and
then their locators. `type show` requires a locator for a `Type` node,
deserializes it with `Type::try_from_body`, and prints a copy-pasteable
declaration. `type create` starts with an empty declaration named `NAME`; a
duplicate active name is rejected by analysis.

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

The selected node for `type override` must be a current, well-formed `Type`
declaration returned by the supplied query. The command copies that stored
`Type`, preserves its `name`, applies the patch, and appends it as a child of
the selected declaration. The user never has to retype a whole schema to add a
field. Changing the declaration name is intentionally impossible: an override is
the next version of the same declared type.

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
# Find the active Task declaration, then verify the selected entry remains
# part of that exact query before appending its successor.
$ lifecore query nodes all --where fresh --where 'type Type' \
    --where 'field name equals-text Task' \
    --select 12:45a8e2d41f7c9b0e -- type override \
    --field 'priority = Number : Optional' \
    --allow-outgoing Relates_To --commit
```

This append enters **Staged**, so all existing fresh `Task` nodes are checked
against the proposed active declaration before commit. A new mandatory field can
therefore make the command fail with TypeCheck inconsistencies. This is
intentional: `type override` makes the parent relationship convenient, not
unsafe.

## Ordinary appends

The basic commands remain small and explicit:

```text
lifecore add TYPE [--text NAME=VALUE]... [--number NAME=INTEGER]...
  [--parent ENTRY-LOCATOR] [--commit]

lifecore connect --from ENTRY-LOCATOR --type EDGE-TYPE --to ENTRY-LOCATOR
  [--commit]
```

`add --parent` and both `connect` ends require node locators visible in the
current effective graph. A child normally has its parent’s node type; a
different type is allowed only when the active parent declaration’s
`additional_override` permits it. TypeCheck, not the CLI, owns that semantic
decision. Both commands print all candidate diagnostics and mark output as
`dry run` or `committed`.

The MVP still accepts only text and signed integer bodies through `add`. Complex
payload construction belongs in a later value-input proposal. Type declarations
are the narrow exception because their implemented parsers already provide a
safe, readable input language.

## Messages and dismissal

Analysis produces Messages for every valid historical prefix. The CLI renders
them deterministically by offending location (context end, then entry index),
severity, analyzer name/configuration, text, and canonical message ID. Output
includes analyzer, severity, location, relevance, text, action summary for
Suggestions, and a short handle:

```text
m:77ef2c15985edb1a  Warning  Type_Check  entry 42:6b1f0e4c8a7d54b2  ...
```

A message ID is SHA3-512 over a versioned canonical representation of severity,
analyzer name/configuration, offending location, text, normalized relevance,
and, for Suggestions, ordered actions. Relevance is sorted only for identity;
action order remains significant. The persisted dismissal registry contains the
full digest and ID-format version, never a display prefix or rendered text.

```text
lifecore message list [--include-dismissed] [--include-resolved]
lifecore message show MESSAGE-ID
lifecore message dismiss MESSAGE-ID
lifecore message restore MESSAGE-ID
lifecore message apply MESSAGE-ID [--commit] [--keep-message]
```

`MESSAGE-ID` is `m:` followed by at least 12 hexadecimal digits. `show`,
`dismiss`, and `apply` resolve it against the current message set and require
exactly one match. `restore` also resolves registered dismissals, so an old
dismissal may be removed even if its message is not emitted today.

The message state machine is separate from graph correctness:

```text
Visible Warning/Suggestion -- dismiss --> Dismissed (full ID saved)
Dismissed -- restore --> Visible if emitted and otherwise eligible
Visible/Dismissed Suggestion -- apply dry-run --> candidate Analyzed
candidate Analyzed -- apply --commit --> graph Saved; suggestion dismissed
candidate failure --> original graph and dismissal registry unchanged
```

`message dismiss` is a CLI-state-only save. It rejects Inconsistencies and never
modifies the graph. `message apply` follows the full graph lifecycle. On a
successful commit it dismisses the applied Suggestion unless `--keep-message`
was supplied; a dry run never creates a dismissal. `--include-dismissed`
replaces the old `--no-bark` spelling.

Inconsistencies are always visible and always block graph commit. A Warning or
Suggestion can additionally become **resolved by history** when its specific
offending entry and every relevant entry are stale in the latest view. That is a
computed presentation state, not a persisted dismissal. A context-only message,
or one without entry relevance, is not silently declared resolved.
`--include-resolved` exposes the computed history.

## Interactive mode (`-i`)

`lifecore -i` starts a TTY-only session. It loads and analyzes the same
effective graph as ordinary commands, holds no lock, and persists no hidden
session. JSON output and `-i` are mutually exclusive. Non-interactive commands
remain complete for automation; `-i` replaces repeated copying of locators and
message handles while a human explores one state.

The first interactive subset is deliberately bounded:

| Available in `-i`                            | Deliberately not available in `-i`   |
| -------------------------------------------- | ------------------------------------ |
| query and entry inspection                   | sidecar attach, detach, or commit    |
| select a query result                        | analyzer configuration changes       |
| bind an alias to the selected node           | free-form sidecar authoring          |
| create and patch a `Type` declaration        | generic batch scripting              |
| list, show, dismiss, restore, apply messages | arbitrary persistent command history |
| review and commit one staged action          | generic `add` and `connect`          |

`add` and `connect` stay non-interactive initially because compound payloads and
two endpoint forms need deliberate input design; a half-designed prompt would be
less safe than explicit locators.

```text
SessionStart -> Browse
Browse -- run query --> QueryResults
QueryResults -- choose row --> SelectedNode
QueryResults -- back ----------> Browse
SelectedNode -- inspect --------> SelectedNode
SelectedNode -- alias/type override/add child --> PlannedAppend
PlannedAppend -- validate + analyze --> Review
Review -- commit --> Browse (reload saved effective state)
Review -- cancel --> Browse (discard clone)
Browse -- messages --> MessageList
MessageList -- focus one --> FocusedMessage
FocusedMessage -- dismiss/restore --> MessageList (CLI state saved)
FocusedMessage -- apply suggestion --> PlannedAppend
any screen -- quit --> SessionEnd
```

Choosing a row makes the same locator-backed selection as `query --select`.
Before an action consumes it, the client re-runs the query in the current view
and verifies membership. A graph-changing operation invalidates all in-memory
selections after commit or cancellation and returns to `Browse`. Old row numbers
never become identities.

At `Review`, the UI shows exact planned entries and their predicted locators,
unfiltered Inconsistencies, visible Warnings/Suggestions, and whether the next
action is a dry run or a commit. Commit has no implicit “yes” default. With an
attached sidecar, the session is read-only and does not offer mutation menus.

## Output, errors, and determinism

`--format auto|table|lines|json` applies to entries, queries, types, messages,
and transaction results. `auto` chooses a table only for a TTY. JSON includes
full locator and message digests, display prefixes, view boundaries, message
visibility state, and transaction state.

Presentation order is never incidental hash-map order:

- query results sort by entry index ascending;
- type listings sort by declaration name then entry index;
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

### Expected-head guard

Add `--expect-head ENTRY-LOCATOR` to committing graph mutations. Immediately
before atomic save, verify the newly read canonical state still ends at that
locator (or an explicit empty-state token). It is a compare-before-save guard,
not file locking, and belongs between **Applied** and **Saved**. It gives
scripts and interactive sessions a clear failure when another writer advanced
the log since their query.

### Query explanation

Add `query nodes ... --explain ENTRY-LOCATOR` after basic selection is stable.
It should show every predicate’s result for one candidate and the view used for
freshness. It slots beside `--select`: selection enforces membership, while
explanation explains non-membership. Core should eventually expose structured
explanations rather than make the CLI duplicate predicate evaluation.

### Type-impact preview

Extend interactive `Review` and non-interactive `type override --summary` to
report nodes and edges whose TypeCheck outcome changes under the proposed
declaration. It belongs after **Analyzed**, because only the real provisional
graph and analyzer output can determine impact. It is a presentation summary
over existing diagnostics, not a second schema checker.

### Saved named queries

Later, CLI state may store a name, `NodeQuery`-compatible predicates, and a view
policy such as `latest`. Execution must still construct a bounded view at run
time and require a locator for mutation. This belongs in CLI metadata beside
aliases, never as an implicit mutable result set; `--select` remains the
membership guard.

## Implementation boundaries and acceptance criteria

Command handlers should share orchestration rather than stage their own graphs.
A practical module split is:

- `state`: state-file resolution, persisted state, `Mock` rejection, atomic
  save;
- `transaction`: lifecycle state machine and append/analyze/apply helpers;
- `locator`: canonical entry-witness encoding, SHA3-512 parsing, view-aware
  resolution;
- `query`: predicate parsing, `NodeQuery` construction, rendering, selection;
- `type_command`: `FieldDecl` parsing, `Type` construction and patching;
- `messages`: canonical IDs, dismissal registry, visibility, suggestion apply;
- `interactive`: the `-i` state machine over the same command services; and
- existing `sidecar` and `graphviz` modules adapted to locators and views.

The proposal is ready to claim implementation only when tests cover: dry-run
append leaves state-file bytes unchanged; committed state has no `Mock`; basic
validation and analyzer failures roll back the whole candidate; a correct index
with wrong digest is rejected; `--select` rejects a locator absent from its
query result; a type override uses the selected declaration as parent and
preserves its name; malformed field syntax never appends; an Inconsistency
cannot be dismissed; a dismissal survives reload; interactive cancellation saves
nothing; and an invalid attached sidecar can still be detached without being
read.
