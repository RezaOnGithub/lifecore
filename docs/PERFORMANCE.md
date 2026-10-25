# Checking cost

Status: measurements and remaining work. Checking replays every prefix of a
candidate under the configured suite
([ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization)), so its cost grows
with history length; it is now roughly quadratic under TypeCheck and
Reconnect, and roughly cubic once Planning is configured. The writer's
busy-retry window ([STORAGE §4](STORAGE.md#4-writers)) is sized against the
quadratic case.

## 1. Measurements

Measured ad hoc with a release build of `lifecore-cli`. Each snapshot holds a
`Task` type, tasks, revisions, and `DependsOn` edges, with TypeCheck
configured. The table times `database.describe` through `rpc`, and `check` of a
one-node proposal, before (`a07483e`) and after the two changes below.

| Entries | describe before | describe after | check before | check after |
| ---: | ---: | ---: | ---: | ---: |
| 500 | 0.03 s | 0.004 s | 0.8 s | 0.016 s |
| 1,000 | 0.3 s | 0.009 s | 21 s | 0.06 s |
| 2,000 | 3.1 s | 0.03 s | 370 s | 0.25 s |
| 4,000 | 33 s | 0.11 s | not run | 1.0 s |

Before, `View::is_stale` scanned every visible node, and TypeCheck re-judged
every revision transition in the view at every replayed prefix, so cost grew
faster than the cube of history length. Two changes fixed that:

- Staleness is answered in constant time from a derived index of each node's
  first-child position, owned by `LifeCoreList`, rebuilt on load, extended by
  `prepare` only with validated entries, and compared with each view's length
  ([SPEC §9](SPEC.md#9-inspection-and-queries)).
- TypeCheck judges a revision transition only at the prefix whose newest entry
  is the successor; full replay still judges every transition once, with
  unchanged finding keys and acceptance
  ([SPEC §7.2](SPEC.md#72-successor-type-permission)).

Where compared (describe and entry listings up to 2,000 entries, check up to
1,000), outputs matched the baseline apart from the new entry's random origin,
though that one-node proposal yields no observations. On the 500-entry
snapshot, a proposal that declares a type, revises a current Task into it, and
appends two more nodes is rejected by both: the baseline reports three
`BadSuccessor` observations (then named `BadOverride`) at boundaries 502–504,
the new code one at 502, under the same key apart from the provisional entry
origin.

The domain analyzers were measured the same way, on snapshots shaped like a
plan: the ten planning declarations, then windows of seven entries each, a
task, its revision, a `committed` stance about the revision with its `About`
edge, a review, and, in the next window, a `Done` about the revision with its
`About` edge. Each cell is the median of three `check` runs of a one-node
proposal.

| Entries | TypeCheck | + Reconnect | + Planning | all three |
| ---: | ---: | ---: | ---: | ---: |
| 498 | 0.035 s | 0.036 s | 0.056 s | 0.057 s |
| 995 | 0.14 s | 0.13 s | 0.22 s | 0.20 s |
| 1,996 | 0.44 s | 0.44 s | 1.4 s | 1.4 s |
| 3,998 | 1.7 s | 1.8 s | 10.8 s | 11.1 s |

Reconnect adds almost nothing: it judges a prefix only when its newest entry
supersedes a node, with one scan of the view
([RECONNECT §4](RECONNECT.md#4-sticky)). Planning grows with the cube of
history length. Each seal rebuilds the planning graph of its prefix, and its
per-item questions (what an item serves, its `Enables` sources, readiness,
strict subgraphs) each scan every current edge of the view, so a seal costs
items times edges, and replay judges every seal. At 4,000 entries a check
under `lifecore init --default-analyzers` exceeds the 10-second busy window.

## 2. Output size

A nonsticky warning about current state is re-emitted at every prefix where it
holds. When each emission was a distinct observation, a proposal of n tasks
missing an encouraged field produced n(n+1)/2 observations: 20,100
observations and a 36 MB check result for 200 tasks, at `b20673d`. Recording
one observation per contiguous run of prefixes
([ANALYZERS §2](ANALYZERS.md#2-finding-identity-and-observations)) gives the
same 200 tasks 200 observations and a 0.36 MB result, with the check at 0.05 s
and 5 MB of memory.

## 3. Backlog

Not scheduled:

- Incremental `basic_validation`: `prepare` still revalidates the whole graph
  before staging and after every staged entry; only the appended suffix needs it.
- Hash-based duplicate checks: node and edge duplicate detection still scans the
  prefix before each entry.
- Adjacency indexes: `children`, `edges_of`, `descendants`, and
  `fresh_successors` still scan the view.
- TypeCheck type-table or parse caching: every prefix re-parses each current
  declaration and rebuilds its type table.
- Planning's seal judging: index the current `Enables`, `Helps`, `About`, and
  `Until` edges by endpoint once per seal instead of scanning the view for
  each item, so that a seal costs time linear in its prefix and replay under
  Planning becomes quadratic ([section 1](#1-measurements)).
- Reuse of base-prefix observations: every check replays the committed base
  prefixes again.
- A repeatable benchmark in the repository, replacing these ad hoc measurements.

Caching or incremental checks must reproduce full-prefix results exactly
([PREFIX-VALIDITY §5](PREFIX-VALIDITY.md#5-failure-handling-and-cost)), keyed
by exact base, suite, implementations, and inputs. The server reloads the
snapshot for every request, so load cost matters there too; a later cache would
need a freshness check, because dismissal edits change the snapshot without
rotating the graph revision (see snapshot change detection in the
[plan's backlog](PLAN.md#3-backlog)). Acknowledgment verification reads
observations beginning within the committed base from the same replay, so an
incremental checker must still provide them.
