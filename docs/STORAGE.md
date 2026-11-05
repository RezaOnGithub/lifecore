# Storage and publication

Status: application contract for selecting, reading, and publishing the
database. These requirements belong to the synchronous application service
shared by HTTP and CLI; the graph and analyzer libraries do not own filesystem
publication.

## 1. Database selection

Resolve the database path in one synchronous host helper:

1. Use `LIFECORE_STATE_FILE_PATH` when set; an empty value is an error.
2. Otherwise use `LifeCoreState.json` in the process working directory.
3. Resolve any relative selected path against that directory. Do not search
   parent directories or fall back after a selected path fails.

The server resolves its path once at startup; each CLI process resolves its own
path. Failures to open an existing database remain errors. Only `init` creates
one. Moving the file preserves database identity. Frontend requests never select
arbitrary host files.

## 2. Snapshot and readers

Store one JSON snapshot at the selected state path. Its envelope contains:

| Field | Stored content |
| --- | --- |
| `graph_revision` | UUID identifying this graph/suite generation ([section 3](#3-graph-revision)). |
| `graph` | The complete graph: database identity (`id`), ordered `entries` in the vocabulary of [FILES §2.5](FILES.md#25-entry-shaped-types), and the exact ordered analyzer `suite`, each a `name` and its verbatim `configuration`. |
| `dismissals` | Exact finding keys, dismissal reasons, and optional notes. |

Graph, revision, and dismissals form one atomic publication unit. There is no
stored `AnalysisOutput` or report directory. Rewriting the full snapshot is an
accepted initial cost. Reject malformed envelopes and structurally invalid
graphs on load; ordinary inspection does not require installed analyzers.

Other CLI/server processes may write the same database, so the server keeps no
state between requests: each request opens the selected snapshot afresh, and
historical requests select a prefix of it. Readers open one snapshot and read
it completely; replacement leaves that open snapshot immutable, so a read
concurrent with a publication returns the complete old or new snapshot. Reads
and standalone checks need no lock. A current-state reload reopens the selected
path; retaining an old open handle cannot observe replacement.

## 3. Graph revision

The graph revision is an application-maintained generation identifier, not a
content hash or a graph boundary. Generate a fresh UUID whenever graph contents
or the exact ordered analyzer suite changes, and publish it atomically with
that state. Equal-count divergent histories and suite-only changes therefore
invalidate old [plan IDs](FILES.md#optional-structured-plan-identity).
Dismissal edits, no-op writes, and token minting leave the revision unchanged.
`ViewId` describes a graph prefix; it does not include the analyzer suite,
which history does not version.

Verify a token's database and revision against the loaded snapshot; core commit
still compares database identity, original entry count/content, and the exact
ordered suite. Moving or copying an entire snapshot preserves its identities; a
subsequent graph/suite mutation allocates a fresh revision. Never combine a
revision with unrelated graph state. Revision maintenance assumes
application-owned snapshots: direct edits that retain the stored revision are
outside this guarantee, and the revision cannot detect them. Use change files
and application operations to mutate the database.

## 4. Writers

Append `.lock` to the state path for the stable lock file, defaulting to
`./LifeCoreState.json.lock`. Keep this file across publications; do not lock the
replaceable snapshot inode or delete/recreate the lock file on release. All
cooperating writers use this target, including initialization and
dismiss/restore. Use the standard library's `File::lock`/`try_lock`. A writer
that finds the lock held retries for a bounded window, then returns a
structured busy error instead of blocking indefinitely. The window defaults to
10 seconds, leaving a margin over a competing apply's check under the default
suite, which takes about two seconds at 4,000 entries and grows roughly
quadratically ([PERFORMANCE §1](PERFORMANCE.md#1-measurements)). Hosts can change the window
with `Service::with_busy_retry`. Ordinary operations on a missing database fail
without creating the snapshot or its lock file, and a refused `init` creates no
lock file.

A mutation acquires writer exclusion, reloads the current snapshot, verifies
the expected base, checks/prepares the change, and stages any verified
acknowledgment alongside the reloaded dismissal records
([SUGGESTIONS §4](SUGGESTIONS.md#4-atomic-application)). Dismissal edits
likewise reread and preserve other records under writer exclusion. Only a
snapshot read under the writer lock for its own state path can replace the
published one; an unlocked read is a preview. Do not hold locks across browser
editing or between standalone check and apply. An optimization that prepares
outside the lock must compare the exact base again under writer exclusion
before publication.

Allocate a fresh graph revision only if graph/suite state changes. Write the
complete candidate to a temporary file in the same directory, with the
published snapshot's permissions applied before any content, flush and
synchronize it, then atomically replace the published snapshot. Synchronize
the containing directory before releasing the lock and acknowledging success.
Initialization uses no-clobber atomic publication and never replaces an existing
state path: it hard-links the synchronized temporary file into place, so it
requires an existing containing directory on a filesystem with hard links. No
publication step may expose a partly written snapshot.

- Exclude cooperating writers from exact base verification through publication;
  two candidates prepared against one base cannot both replace it.
- Initialize a new empty database without clobbering an existing one.
- Publish a whole checked plan atomically. Failures before publication preserve
  the previous stored state. Flush/synchronize required data before publication
  and synchronize the containing storage after publication.
- Reload graph/suite, graph revision, and dismissal data without changing
  identities. Dismiss/restore survives reload and absent current emission.
- Validate a supplied token's database/base expectation before preparation and
  preserve the chosen concrete base through publication even when no token was
  supplied. Persist the base identity consistently with the graph/suite state.
- Publish graph changes and any acknowledgment as one unit. Invalid proposals
  and pre-publication failures preserve both.

## 5. Failure and uncertainty

A failure before publication leaves the previous stored state intact. After
publication, failed synchronization may leave the caller uncertain: the new
state may already be visible, so this is never a rollback, and a
post-publication failure never justifies claiming that one half of a combined
graph and acknowledgment change rolled back. Report that uncertainty together
with the outcome the call would have returned, including the new graph
revision and operation-to-entry mapping, and support inspection of the stored
state; do not automatically repeat the changes. A disconnected client leaves an
executing mutation to complete as
[INTERFACES §1](INTERFACES.md#1-shared-synchronous-application) describes.
Token minting alone supplies no durable application receipt; retry recovery is
[backlogged](backlog/RECEIPTS.md).

## 6. Acceptance scenarios

| Scenario | Required outcome |
| --- | --- |
| Supported writes change entries/suite, including equal-count divergence | Publish a fresh graph revision atomically and reject old tokens; require deliberate refresh/replanning. |
| A dismissal changes, a no-op is written, or a token is minted | Preserve graph revision; existing plan bases remain valid. |
| Initialization targets an existing database | Refuse without clobbering it. |
| Publication fails before replacement | Leave prior state intact. |
| Synchronization fails after publication | Report possible publication; permit state inspection and do not automatically retry. Receipt recovery is deferred. |
| Dismissal is saved, reloaded, restored, or edited by concurrent frontends | Retain exact keys/reasons/notes and preserve each writer's update; never affect acceptance. |
| A reader opens the snapshot during publication | Return one complete old/new snapshot, never mixed graph/revision/dismissal data. |
| The application restarts | Reload graph/suite and dismissals; recompute analysis without a persisted report registry. |
| Writer advances the base during checking | Reject under writer exclusion before publishing any operation. |
