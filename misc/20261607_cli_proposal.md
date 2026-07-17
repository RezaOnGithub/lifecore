# LifeCore CLI Proposal

An intuitive CLI interface to common database operations that makes correct
usage of the API.

Features:

- Store database on disk in JSON format
- Event alias
- Batch operations via TOML "sidecars"
- Apply Suggestions
- Helpful `InteractiveAnalyzer` to aid in interactive usage
- Barklist
- Query Command
- GraphViz visualization

These features are described in this document.

# General Clarifications Regarding LifeCore

It should be noted that LifeCore strives to keep a consistent and validated
database at any `EventContext`; to closely enforce these invariants, the intent
is to run Basic Validation and all Analyzers at every database mutation, (where
Analyzers also scan the database in every valid window as well; see
`lifecore::analysis::AnalysisRunner`) and a database mutation shall only add
exactly 1 event to the database (in addition to bookkeeping/circumstance, but 1
logical addition nonetheless)

# Specification

## Storage

For now, the CLI shall store its data in pretty-printed JSON format with two
keys: `"lifecore"` maps to a serialized instance of `LifeCoreList` and `"cli"`
maps to a serialized instance of all the data relevant to the CLI program
itself. Whenever we refer to the "database" in this document, we are referring
to the serialized `LifeCoreList` however the CLI will need data storage of its
own for certain operations, and this state shall be kept separate from the
database as described. Whenever we refer to the "state file", we are referring
to the file holding the aforementioned JSON data.

By default, the CLI should operate on the state file in the current directory
named `LifeCoreState.json` UNLESS environment variable
`LIFECORE_STATE_FILE_PATH` is defined; then the state file path is the path that
is specified by the environment variable. In this mode CLI SHALL NOT fall back
to the implicit working directory path for the state file in case of errors.
(e.g. if the path specified in the environment variable is non-existant, do NOT
fallback to the file that may be present in the working directory)

Before the state file is usable, it must be initialized via an `init`
subcommand. Then, the state file with the path derived from the procedure above
is created with no initial events contained. `init` fails if the database file
already exists.

`init` is a database mutation operation; thus it must run Basic Validation and
the default analysis suite on the empty database, then invoking the `apply`
method and writing to the state file.

Upon deserializing the database, CLI must immediately abort if the database
contains a `Mock` circumstance; in general, `Mock` must never be present in the
persisted database in the state file, and the CLI may assert it is the case
before writing to the state file.

For the purpose of the MVP, the state file need not have a version field or
exact specification; if it can be deserialized, it is considered workable.

For the purpose of the MVP, the CLI shall read and write the state file
directly, with no file locking, or rename-based control.

## Transactions, Validation and Analysis

LifeCore database exists in 3 states: just read from the disk, provisional,
ready to be written back to the disk. To enter provisional state, new events
need to be added via `add_mock` and `connect_mock` method. After each addition
operation, if a basic validation error is raised, the current operation is
aborted. Then, AnalysisRunner must run the Analysis Suite associated with the
database. Note that (at least for now) the entire Analysis Suite must pass
without any Inconsistency in order to continue the operation.

Once all the desired new events are added in the provisional state, CLI shall do
nothing with the database UNLESS the user has opted to write changes. Then,
`apply` method of the database is called and, if successful, the database is
ready to be written back to the state file. Depending on whether the actual file
write operation is successful or not, the persistent database will be modified.

Note that the behavior is slightly different if a sidecar is present; all
sidecar content are implied to be provisional additions, thus on start-up, the
state of the database presented to the user is no longer "just read"- the
operation starts in the provisional state, (i.e. `is_mock()` returns true) and
has further limitations detailed in the following sections.

CLI is equipped with AnalysisRunner with all the currently-implemented Analyzers
in this workspace registered. For each write operation, if it is successful
(i.e. Basic Validation passes) then the complete AnalysisSuit is run. If no
Inconsistencies are generated, the database may proceed to the next state or
have additional mutations operations performed on it; otherwise, CLI must abort
current operation.

By default, `Interactive` and `TypeCheck` are registered as analyzers upon
`init`, (see above) This fact needs to clearly acknowledged to the user.

The built-in analysis runner shall be equipped with all analyzers implemented
thus far in the current workspace, with a straight-forward importing new ones in
code and recompiling.

(Organizing CLI functionality into a reusable system for integration of
thirdparty analyzers for the purpose of custom CLIs is not within the scope of
the current CLI proposal)

User may add new analyzers with `analyzer add` subcommand; no checks are
performed regarding whether such Analyzer and Analyzer factories exist.

## Diagnostics

After any database mutation operation, all diagnostics shall be printed, in a
tabular format fit to the width of the terminal, (default) or a simple
line-by-line fashion, (if not a tty) or in machine-readable JSON. (adjustible
via commandline parameters)

Note that, by design, the analysis runner will generate all diagnostics
resulting from all past views of the database in addition to the latest mutation
attempt. This is by design, and the mechanism for filtering non-fatal
diagnostics is described in a later section.

Users can trigger a basic validation check and view all diagnostics for the
current database, with barklist on or not, via `check` subcommand. This
operation shall not modify the state file.

Diagnostic Messages must be sorted with a stable and robust sorting strategy
before presenting to the user, which is also the same algorithm for selecting
the right message. (See below) Messages SHOULD be robustly hashable. (This may
require revising the current Hash implementation for the relevant datatypes, Ord
for `*EventContext`, and the robustness of the Analyzers themselves.)

The preferred hash algorithm is `SHA3-512` from `sha3` crate, for now.

## Mode Of Operation

The purposed mode of operation for adding an alias or removing an alias or
applying a suggestion or adding a diagnostic to the barklist by hash or adding
an alias' `PointedEventContext` to the barklist is as follows:

- User invokes the CLI for the aforementioned operation, and receives a numbered
  list of possible targets from the CLI.
- The user re-invokes the CLI with `--select X` appended to the parameter list
  to select an entry from the previous table; CLI performs the aformentioned
  operation on the selected item.
  - Note that to facilitate interactions of this sort, the order of output
    entries needs to be deterministic! This requires a review of the internal
    API and especially the diagnostics handling, plus sorting the results.
- Performing `--select` operations without seeing the latest database state is a
  user error; the user must query before repeating the same query with
  `--select`

The CLI shall keep track of whether the last invocation was for a query
subcommand and what the subcommand was, and check against the subcommand in
`--select`. For now it should remember only the very last invocation. If the
database state has changed due to database mutation, or sidecar
attachment/detachment/commitment, or alias definition/removal subcommand
invocation, this history is cleared to `None`.

## Alias

CLI should store alias information in the persisted CLI state. An alias is a
unique user-defined string that maps to a `PointedEventContext`.

Users cannot construct or specify `PointedEventContext` directly on the
commandline; instead they can alias values that are generated by a query or the
`PointedContext` created by an add operation.

For now, users can ONLY create aliases to node events.

Aliases and the details of the node they refer to can be viewed via `alias list`
subcommand. If the node referred to by the alias has been overridden, users are
given the option to update the alias to point to any of the fresh successors.

`alias remove <name>` to forget an alias.

Multiple aliases MAY refer to the same event.

Alias names are unique. As long as an alias exists, users may not add another
alias of the same name.

Failure to add an alias CAN abort an add operation.

## Singular Mutation

The CLI shall have basic `add` and `connect` functionality for adding a single
node or edge, and optionally `.apply()` and persisting these operations via
`--commit` flag. (i.e. "dry run" by default.) For now, `add` command should
support adding nodes with text and numerical fields, in a syntax such as
`--key FIELD --value PAYLOAD`. (Parsing other payload types is currently not
within the scope of the MVP but ought to be straightforward to implement later.)

## Sidecar

To further ease the use of LifeCore, the user may specify any number of
operations in a separate file, dubbed "a sidecar" from here on out. You can
think of the sidecar as an extension of the main database, describing entries to
be added.

The path to the sidecar file is persisted in the CLI state in the state file.

Having an attached sidecar imposes the following restrictions on the operation
of the CLI:

- Suggestions cannot be applied.
- New aliases may not be created.
- Existing aliases may not be removed.
- Singular mutations are not allowed.
- Adding by hash or by PointedEventContext to the barklist is not allowed.
- Any database mutation operation (other than `sidecar commit` detailed below)

Other, "read-only" operations are performed as normal, after Sidecar enteries
have been added to the provisional state. (See below)

Sidecar entries are inserted in the memory-resident representation of the
database, (entering and operating in provisional mode) without writing to the
state file, with validation and analyzers run for each singular mutation just
like singular database mutation operations in regular operation.

A sidecar may be attached with a dedicated subcommand
`sidecar attach <filename>`. It may be committed or detached similarly.

### Format of The Sidecar

Sidecars are regular TOML files, with special rules for converting between TOML
values and `PayloadExt`. Sidecars are loaded at every invocation of the CLI; you
may think of them as an addendum to the database, or means of performing batch
insertion.

Sidecar enteries are stored in an array of TOML tables located in the root
table. Each array entry contains a number of fields describing the entry. Here
is a sample:

```toml
[[entry]]
alias = "BuyMilk" # alias to this node; optional
ty = "Note" # type of this node; mandatory
parent = "P" # alias to the parent; optional
[entry.body] # node fields; optional (mandatory for typechecking against most types)
content = "Buy Milk"
[entry.connect] # node connections to be created immediately afterwards; optional
connect_to = [
    { target = "AtWork", ty = "Related" }
]
connect_back_from = [
    { target = "DairyContrib", ty = "Accumulate" }
]
```

Sidecar entries describe a new node, and how it shall be connected to the rest
of the graph. The sample results in the following changes to the database:

- A new node of type "Note" is added, with a single string PayloadExt
  corresponding to the field `content`.
- Edges in `connect_to` are added in the order of iteration. (in the order that
  they appear in the deserialized table) The node is the head of the edge.
- Edges in `connect_back_from` are added in the order of iteration. (in the
  order that they appear in the deserialized table) The node is the tail of the
  edge.
- An alias "BuyMilk" is temporarily added to the alias namespace in memory but
  not written back to CLI state. (to enable making use of this alias in
  subsequent sidecar entries or query operations)

Sidecars entries may refer to aliases already introduced before attaching the
sidecar. a side entry may refer to aliases defined in preceding entries.

Rules for converting TOML values to PayloadExt are not settled on right now, and
the only mandatory functionality for now is the straightforward forwarding of
integers and strings to PayloadExt variants. The implementation must be flexible
enough for adding future conversion methods regarding Timestamp and Serialized
and, potentially, new PayloadExt types. These will likely involve inline tables.

`sidecar dump` subcommand shall dump all events added in the provisional state.
(One implementation strategy is to query for all events that have the `Mock`
circumstance index)

### Attaching The Sidecar

The sidecar shall be attached via `sidecar attach` subcommand, followed by
relative path to the sidecar file. CLI then shall resolve the relative path to
an absolute path, and add this path and sidecar attachment status to CLI state,
which will be persisted in the state file.

Sidecar attachment shall only fail if the file does not exist; other failure
modes such as deserialization error or error during processing shall not result
in sidecar attachment failure.

### Detaching The Sidecar

Once the sidecar is detached, the state file MUST be identical to the state
before the sidecar was attached with the exception of the last query log.
(outlined in a previous section)

(Note that this is not an explicit condition to be checked for, so much it is an
implementation guideline and a guiding principle when validating this proposal
for conflicting requirements.)

### Committing Sidecar Files

This operation is performed only per user's explicit command. The sidecar is
loaded, provisional state is built, and the database is made ready for
write-back similar to singular database mutation operations, and written back
thusly.

When committing, new aliases defined by the sidecar are added to the CLI state,
and no sidecar is attached anymore.

The sidecar file itself shall NOT be deleted by LifeCore CLI.

## Interactive Analyzer

This is a new Analyzer specifically for generating helpful suggestions in
interactive use. Currently it implements a single check, but may be augmented
later for new features.

Current Task: If the last event in the analysis window is a node, and if the
node has a parent, consider the edges of the parent and suggest to recreate the
same connections using the child IF the other end of the edge is not a stale
node AND any fresh successors of the parent don't already have a similar
connection.

## Barklist and Non-Fatal Diagnostics Filtering

It is not a typo; Barklist contains the hash of ignored Warnings and Suggestions
that should not be diplayed anymore, alongwith PointedEventContext of events the
user considers "resolved".

Current Filtering Algorithm:

- Never filter or suppress Inconsistency or other errors, which abort the
  current operation anyway.
- If hash of the Warning of Suggestion match with a known hash stored in the CLI
  database, filter away.
- If `offending_event` and `relevance` fields of Warning and Suggestion contain
  only "resolved" PointedEventContext, (via `absolute()` compare) filter away.
  - Note: If `offending_event` does not contain a PointedEventContext the
    message cannot be filtered in this manner.

Barklist may be disabled for the output of any operation if `--no-bark`
parameter is specified.

## Applying Suggestions

Refer to "Mode Of Operation".

SuggestedAction currently captures the same basic mutation operations as
"Singular Mutation" above, and applying them with alias creation is subject to
the same aliasing failure mode described above.

Once applied, Suggestion is added to the barklist by hash unless `--no-bark` is
specified by the user.

## Query

There are two kinds of query operations: filtering and picking. Invoking the
`query filter` subcommand without any predicates presents the user with every
single event in the database in chronological order. (Older events are printed
to stdout first) Invoking the `query pick` subcommand without any predicates
presents the user with no events. (Result of the query is an empty set)

In `filter` mode, an event is displayed if it passes ALL predicates. In `pick`
mode, an event is displayed if it passes ANY predicate. (Conjunction vs
Disjunction)

The list of predicates:

- `--fresh-by <ALIAS>`, events that are fresh within
  `.shrink_to_inclusive_left()` of the alias.

- `--type <TYPE>`, nodes or edges with the specified type name.

- `--match-text <REGEX>`, is true if the event is a node and has any
  PayloadExt::Text fields that match the specified regex.

The list above will be expanded in the future. (Implement accordingly)

## GraphViz

Requires a target file path from the user. A basic implementation for now; has
only 2 modes:

- Fresh nodes and edges by the latest state of the database. (within the
  `.last()` context)
- Full database dump, with node families organized into clusters with
  parent-child relation ships specified with a special edge shape. (within
  `.last()` context)

The visualization shall use the `mrecord` shape for nodes, where the label
contains the node PointedEventContext, type, alias, and (per user request) a
dump of all the field contained.

The generation process must result in an SVG file without requiring temporary
`dot` files, by writing directly over `stdin` and redirecting the GraphViz
output to the target SVG file.

The user may opt to only get a `dot` file, in which case GraphViz executable is
not invoked at all and the graph visualization definition is written to the
target file.

# Implementation Strategy

The implementation must be contained within the `lifecore-cli` crate and be
composed of at least the following modules:

- `graphviz`, reusable visualization algorithm
- `interactive_analyzer`, reusable `Interactive` analyzer.
- `sidecar` for sidecar parsing

You may decide on the code organization with regard to the rest of the features.

The implementation should contain a modest amount of logging, and it shall
initialize `env_logger` with `trace` log level by default for now. Use `log`
structured logging and the `env_logger` crate to log to stderr.

Performance is not a concern, and simplicity and correctness concerns override
performance.
