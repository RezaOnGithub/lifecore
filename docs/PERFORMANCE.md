# Checking cost

Status: measurements and remaining work. Checking replays every prefix of a
candidate under the configured suite
([ANALYZERS §1](ANALYZERS.md#1-execution-and-normalization)), so its cost grows
with history length; it is now roughly quadratic under TypeCheck and
Reconnect. The planning analyzer, now shelved (AGENTS.md), made it roughly
cubic (section 1). The writer's
busy-retry window ([STORAGE §4](STORAGE.md#4-writers)) is sized against the
quadratic case.

## 1. Measurements

### 1.1 Benchmark

`cargo bench -p lifecore-app` runs the checking benchmark,
`lifecore-app/benches/checking.rs`, in a release build timed with `Instant`.
Its histories come from `lifecore-workload`, whose crate documentation
specifies them: planning's ten declarations at `59c702c` as plain types, then
tasks of six entries each, with a `Review` closing each window. At one review
per 7 entries they have the shape of the planning measurements in section
1.2. One per 1,000 entries is a database growing by 1,000 entries a week with
one review a week.

For each size and density, the benchmark writes the snapshot as storage
publishes it and times `database.describe`, which reads, parses, and validates
it ("load"), and `check` of a one-node proposal under the suite of `lifecore
init --default-analyzers`, TypeCheck then Reconnect ("check"). A check loads
the snapshot again, has `prepare` validate the staged entry against the
indexed graph ([SPEC §9.1](SPEC.md#91-indexes)), and replays every prefix.
Until step 2 of the core round, `prepare` revalidated the whole graph before
staging and after the staged entry, with quadratic duplicate scans. Each cell is the
median of three runs. `LIFECORE_BENCH_SIZES` takes other comma-separated
entry counts, and `LIFECORE_BENCH_RUNS` another number of runs; the median of
an even number is the mean of the middle two.

Before and after the core indexes (step 2 of the core round), taken on a
shared virtual machine with one vCPU, where other work ran besides. The two
builds ran alternately in four rounds within twenty minutes, each run
starting only once the one-minute load average was below 1.3, with
`LIFECORE_BENCH_RUNS=5`. Each cell is the median over the four rounds of each
round's median, before → after. Before is `19f7747`, step 3a; the baseline
taken when the benchmark was added, on top of `febde6b`, agreed with it
within the noise (2.1 s for both checks at 4,000 entries).

| Entries | Reviews (1/7) | load (1/7) | check (1/7) | Reviews (1/1,000) | load (1/1,000) | check (1/1,000) |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 500 | 70 | 0.0017 → 0.0012 s | 0.034 → 0.025 s | 0 | 0.0015 → 0.0011 s | 0.032 → 0.022 s |
| 1,000 | 141 | 0.0047 → 0.0023 s | 0.12 → 0.10 s | 0 | 0.0041 → 0.0022 s | 0.11 → 0.088 s |
| 2,000 | 284 | 0.014 → 0.0052 s | 0.48 → 0.40 s | 1 | 0.015 → 0.0045 s | 0.43 → 0.36 s |
| 4,000 | 570 | 0.053 → 0.0099 s | 1.9 → 1.6 s | 3 | 0.056 → 0.0096 s | 1.9 → 1.5 s |

Times are wall-clock. On that machine, under load from other work, single
runs differed by more than a factor of three, and even quiet rounds by a
quarter at 4,000 entries, so compare only runs taken close together on one machine.

Load is now linear in history length, five times faster at 4,000 entries:
validation looks duplicates up by hash instead of scanning the prefix before
each entry. Check is 15 to 25 percent faster and still grows with the square
of history length, about `1e-7 · n²` seconds. The validation it no longer
repeats was quadratic but small beside replay, and replay is TypeCheck's:
callgrind attributes 91 percent of the instructions of a 1,000-entry check to
it, half to checking each current node's payload against its declaration and
a quarter to finding that declaration by name. The core indexes removed
TypeCheck's per-prefix scan for declarations and their parsing, but TypeCheck
emits every current finding at every prefix, so it still judges every current
node there (section 3). Density still makes no difference: neither TypeCheck
nor Reconnect reads reviews, so the seal terms of CORE-ROUND §4.2 are
measured only once an analyzer that seals at reviews returns.

### 1.2 Earlier measurements

Before the benchmark, measurements were taken by hand with a release build of
`lifecore-cli`; they are kept as history. In the first, each snapshot holds a
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

The domain analyzers were measured the same way at `59c702c`, before the
planning analyzer was shelved, on snapshots shaped like a plan: the ten planning declarations, then windows of seven entries each, a
task, its revision, a `committed` stance about the revision with its `About`
edge, a review, and, in the next window, a `Done` about the revision with its
`About` edge. Each cell is the median of three `check` runs of a one-node
proposal; the last column times `lifecore valuation` at the head.

| Entries | TypeCheck | + Reconnect | + Planning | all three | valuation |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 498 | 0.035 s | 0.036 s | 0.056 s | 0.057 s | 0.005 s |
| 995 | 0.14 s | 0.13 s | 0.22 s | 0.20 s | 0.009 s |
| 1,996 | 0.44 s | 0.44 s | 1.4 s | 1.4 s | 0.021 s |
| 3,998 | 1.7 s | 1.8 s | 10.8 s | 11.1 s | 0.082 s |

Reconnect adds almost nothing: it judges a prefix only when its newest entry
supersedes a node, with one scan of the view
([RECONNECT §4](RECONNECT.md#4-sticky)). Planning grows with the cube of
history length. Each seal rebuilds the planning graph of its prefix, and its
per-item questions (what an item serves, its `Enables` sources, readiness,
strict subgraphs) each scan every current edge of the view, so a seal costs
items times edges, and replay judges every seal. At 4,000 entries a check
under `lifecore init --default-analyzers` exceeds the 10-second busy window.
The valuation is computed at one boundary only. On these snapshots, where
almost every task is done, it costs about as much as loading the snapshot,
which `plan-id` also takes; it asks the same per-item questions, so with many
open items it grows with items times edges, as a seal does.

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

- TypeCheck's per-prefix work, which is nearly all of a check (section 1.1):
  every replayed prefix rebuilds the declaration table, from cached parses,
  and checks every current node's payload and every current edge's endpoints,
  so replay stays quadratic. Its payload check allocates a path part for
  every declared field even when nothing is wrong, and each node looks its
  declaration up by name, which lowers the constant only. Changing the shape
  means making TypeCheck's per-prefix work track what changed at the prefix,
  as its transition rule already does
  ([SPEC §7.2](SPEC.md#72-successor-type-permission)): judge a node's payload
  at its own prefix and again only where its type's declaration changes
  (K3), and an edge's endpoints likewise. Its nonsticky warnings about
  current state, re-emitted at every prefix, stand in the way
  ([PLAN §3.1](PLAN.md#31-open-questions)). Replaying fewer prefixes
  ("last N", CORE-ROUND §4.7) is a last resort, and state handed from prefix
  to prefix was rejected (CORE-ROUND §4.6).
- Keeping index templates in `LifeCoreList` and extending them on staging,
  instead of building them per run. Waits for a server cache that keeps lists
  alive across requests; until then every request loads the snapshot anyway.

Index template candidates considered and not admitted to the catalogue of
[SPEC §9.1](SPEC.md#index-templates), each with what it waits for. Admitting
one moves its definition into SPEC §9.1.

- `NodesByField { ty, field }`, a payload value to node positions. Waits for
  a consumer; the first plausible one is collections (documents by path, in
  [docs/backlog](backlog/)).
- The current members of a type at a boundary, an interval or stabbing index.
  K3 plus staleness enumerates every node of the type ever appended, which at
  weekly seals is small (about 50,000 tasks × 260 seals ≈ 1.3·10⁷ checks over
  five years). Waits for a consumer that enumerates current members at every
  prefix.
- Sums of a field over a position range. Waits for a consumer; Planning's
  budget overrun sums over current stances, which is not a position range.
- Reachability or transitive closure over labels. Blocked by admission
  criterion 1: it is not fixed at append, since one edge can change the
  reachability of many pairs and history may hold cycles. DAG Check computes
  it at the prefixes it judges.
- A key computed by an analyzer-supplied function. Blocked by criterion 3:
  it is analyzer state by another name, which the core could neither test
  against a reference nor share, and it reopens the state handed from prefix
  to prefix that CORE-ROUND §4.6 rejects. A shape several analyzers need is
  added to the catalogue instead.
- Per-entry derived values defined by an analyzer. Parsed declarations were
  admitted as core index K8, because the parse is core code; there is no
  analyzer-defined analogue, for the same reason as the previous item.

A cheaper check must reproduce full-prefix results exactly
([PREFIX-VALIDITY §5](PREFIX-VALIDITY.md#5-failure-handling-and-cost)).
Caching analysis output across checks is not pursued. The server reloads the
snapshot for every request, so load cost matters there too; a later server
cache of loaded lists would need a freshness check, because dismissal edits
change the snapshot without rotating the graph revision (see snapshot change
detection in the [plan's backlog](PLAN.md#3-backlog)).

## 4. Query costs

The cost of each query of [SPEC §9.1](SPEC.md#91-indexes) in a view of `n`
entries, with the core indexes and by its reference scan. `r` is the size of
the result, `k` the length of the key's list in the index, and `d` the depth
of a revision chain.

| Query | With core indexes | Reference scan |
| --- | --- | --- |
| `is_stale`, a node | O(1) | O(n) |
| `children`, `edges_of` | O(log k + r) | O(n) |
| `nodes_of_type`, `edges_labelled` | one hash lookup, O(log k + r) | O(n) |
| `nodes_with_content`, `edge_between` | one hash, O(log k) plus comparing the bucket | O(n) value comparisons |
| `root_of` | O(1) | O(d) |
| `descendants`, `fresh_successors` | O(D log D) for the `D` descendants, stale ones included | O(n), and O(n · d) for `fresh_successors` |
| `declarations` | every `Type` node ever appended, no parsing | O(n), parsing each current declaration |
| Structural validation of the history | O(n) plus hashing | O(n²) |
| Staging one entry | O(1) amortized plus hashing | O(n), and O(n²) before step 2 |

The core indexes take two lists and a root slot per entry plus one hash-map
entry per node and per edge. A clone of the graph, which `prepare` makes for its
candidate, copies them with the entries. Debug builds also validate the whole
history at every staged entry, to compare the incremental result with it, so
staging is linear there.

The index templates of [SPEC §9.1](SPEC.md#index-templates), for a key whose
list holds `k` edges in the run, of which `r` are returned. Lineage templates
find the key's root through K7 in O(1). The reference scan, `template.scan`,
reads `View`'s queries, so its cost depends on whether the view carries the
core indexes.

| Query | With the template | Reference scan, core indexes | Reference scan, none |
| --- | --- | --- | --- |
| `positions` | one hash lookup, O(log k + r) | `EdgesAt`: O(deg) for the key's K2 list; `EdgesAtLineage`: O(e) for the label's K4 list | O(n), and O(n · d) for `EdgesAtLineage` |
| `count` | one hash lookup, O(log k) | as `positions` | as `positions` |
| `in_range` | one hash lookup, O(log k + r) | as `positions` | as `positions` |
| `current` | O(k), checking each listed edge's endpoints by K1 | as `positions` | O(n) per listed edge |

Here `deg` is the key's number of incident edges and `e` the number of edges
with the label. Building the requested templates is one pass over the run's
view: per edge, one hash lookup by its label, then for each of the `t_label`
distinct templates with that label a lookup of the other endpoint and a
membership test in `other`, and a push when it is listed, so O(t_label) per
edge besides hashing. The runner builds them once per run under
`Indexing::All`, and skips the pass when no analyzer requests a template.
Templates take one hash-map entry per key and one position per listed edge.
