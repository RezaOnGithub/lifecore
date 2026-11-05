# Analyzers

Status: core contract. [SPEC §8](SPEC.md#8-analyzers) defines what an analyzer
is and may do; this document specifies how the runner executes analyzers, how
findings are identified, and how reports are presented and dismissed.
[MESSAGES](MESSAGES.md) traces the current implementation in diagrams.

## 1. Execution and normalization

`TypedAnalyzer` returns analyzer-specific typed findings as `Vec<Self::MSG>` from
initialization and analysis. The shared adapter converts each finding with the
implementation's `convert_message(&self, message)` and returns `Vec<Message>`,
preserving order and duplicates for runner normalization. Conversion can use the
analyzer's state and configuration. Direct third-party `dyn Analyzer`
implementations return `Vec<Message>` and are also supported. There is no shared
`Output` wrapper, `InnerOutput` trait, or associated output container.

A `TypedAnalyzer` fails with its own `type Error: std::error::Error`, from
initialization and analysis alike; one that cannot fail uses
`std::convert::Infallible`. The adapter turns the error into its `Display`
text, which becomes the `message` of the `ExecutionFailure`, so the text must
stand alone. A `dyn Analyzer` returns that `String` directly. An analyzer
reading node payloads through typed decoding
([SPEC §6.5](SPEC.md#65-typed-decoding)) may assume that they fit their
declarations: a `PayloadError` is an execution failure, never a finding. With
TypeCheck configured and a decode type that reads only fields the analyzer's
declarations keep, decoding can fail only at a prefix TypeCheck rejects.

Keep domain findings and errors structured and assert their variants and
details in analyzer tests; presentation text cannot substitute for that
evidence. Factories and domain findings remain in their analyzer packages.
Shared `Message` output pairs
an opaque `analyzer_certificate: AnalyzerCertificate` with a `Diagnostic`: the
exact subject set, an optional focus entry, text, `MessageKind`, and
`sticky: bool`. The same `Diagnostic` is carried unchanged into the observation.
`AnalyzerCertificate` is a String newtype, serialized transparently as a string.

Both analyzer traits return `version() -> semver::Version`. The runner constructs
`AnalyzerIdentity` from the registered name, returned version, and original
configuration string supplied to the factory unchanged. Version equality is
exact, including prerelease and build metadata; it is not version-requirement
matching. The built-in analyzers, TypeCheck, DAG Check, and Reconnect,
parse their own `CARGO_PKG_VERSION`. Semantic
or encoding changes intended to invalidate earlier dismissals require a version
change.

`AnalysisRunner::new(EvaluationInputs)` freezes one input value for initialization
and every replayed prefix. Its complete shape is:

```rust
pub struct EvaluationInputs {
    pub evaluation_time: Option<lifecore::OffsetTimestamp>,
}
```

The default is `None`. An evaluation time is an instant with a UTC offset
([SPEC §6.4](SPEC.md#64-value-kinds-and-shapes)), so an analyzer can find the
local date it is evaluated on without reading the system zone. For `now`, the
host captures the instant and the local offset in effect at it, together and
once, before replay, and reports both. A zone it cannot determine is never
read as UTC: the call fails with `local_time_unknown`
([RPC §3](RPC.md#3-errors)), while an empty `TZ` is UTC by Unix
convention. That one capture is the only reading of the system zone, and determinism holds because the offset is an input like
the instant. No clock is read implicitly; a time-dependent analyzer must error
when the required time is absent. All configured implementations are
constructed first, initialized in suite order, then each replays boundaries
`0..=end_exclusive` in suite order. Empty histories execute both initialization
and prefix zero. Missing factories, invalid configuration, execution failures,
and malformed subject/focus output are errors.

An observation records `evaluated_view: EvaluatedView`:
`EvaluatedView::Initialization`, or `EvaluatedView::Prefixes { first, last }`,
the contiguous range of exact prefixes, including zero, at which the analyzer
emitted the same diagnostic under the same key
([section 2](#2-finding-identity-and-observations)).
An execution failure records the single evaluated view it failed at, with
analyzer identity and frozen inputs. Subjects and the focus entry must be
visible in the evaluated prefix. Initialization has no graph access, so an
initialization message has no focus and no subjects.

## 2. Finding identity and observations

`FindingKey` has four fields: database identity, `AnalyzerIdentity`, message
category, and `analyzer_certificate: AnalyzerCertificate`. The wrapped string is
the complete analyzer-owned finding identity, opaque to shared orchestration.
There is no shared `Fact`, nested `Finding`, versioned key envelope, or digest helper.
Grouping and dismissal
compare complete keys, including the entire string; hashes are only collection
implementation details and collisions do not establish equality.

Configuration identity preserves exact original spelling. TypeCheck and
Reconnect may accept empty text and empty JSON objects as equivalent
behavior, but `""`, `" "`, and `"{}"` identify different configurations. DAG Check may normalize its parsed sets
and empty filter/range records internally; order, duplicate, or whitespace
changes in the original string still produce distinct analyzer identities.

Built-ins deterministically JSON-encode their typed findings. TypeCheck's
certificate names the variant, the typed path/issue or schema details, and the
exact subject entry, and nothing about the boundary. DAG Check's names the
cyclic-component variant with sorted exact node/edge identities. Reconnect's
names the lost edge and the successor
([RECONNECT §5](RECONNECT.md#5-finding-identity)). Typed domain
data stays inside each analyzer; presentation wording is not a key.

Ordinary evaluation boundaries, evaluation time, wording, and suggested
actions do not by themselves change finding identity. When a time
threshold or transition distinguishes an issue, the analyzer must deliberately
include that detail in its certificate. Different subjects, distinguishing details,
versions, original configuration strings, categories, or databases stay distinct.
Repeated observations of a dismissed key remain dismissed.

The runner consumes each `Message`, moves its certificate into the `FindingKey`,
and keeps the `Diagnostic` beside that key and the evaluated view. The
certificate lives only in the key, and the frozen inputs only on the output.
Recording merges re-emission: a diagnostic equal in key and content to one
recorded at the immediately preceding prefix extends that observation's `last`;
one already covered by a recorded range is dropped; anything else starts a new
observation. An advisory that holds from prefix 10 to 50 is one observation with
that range, and one that holds again from 70 is a second, so the first prefix at
which a finding appeared is always retained. Wording that names the boundary
defeats merging, so analyzers keep boundaries out of message text. Grouping by
finding key is separate and retains distinct ranges and presentation. Duplicate
suite entries still execute in full and add nothing. The runner returns
`AnalysisOutput`, retaining the selected view, inputs, and ordered
implementation identities even when no messages were emitted. It supports Serde
round trips. Outputs are ephemeral;
[INTERFACES §5](INTERFACES.md#5-analysis-lifetime-and-dismissal-persistence)
owns their lifetime and the independent persistence of dismissals.

Serialized, a finding key's `kind` is its category in lowercase:
`inconsistency`, `warning`, or `suggestion`. A diagnostic is flat and tagged by
the same word in its own `kind`; a suggestion also carries its ordered
`actions`, possibly empty, and nothing else does. An evaluated view is
`{"kind": "initialization"}` or `{"kind": "prefixes", "first": …, "last": …}`.
These encodings follow the shared vocabulary of
[FILES §2.1](FILES.md#21-syntax-and-values); they do not change what a key or
diagnostic contains.

## 3. Presentation and dismissal

The default report selects initialization output, observations whose range
ends at the selected boundary, and advisories with `sticky = true`. A sticky
advisory stays visible at later prefixes until dismissed or filtered; it has no
automatic expiry. With fixed implementation, configuration, and inputs, replay
recovers the earlier observation even if the analyzer emits nothing at the
selected head. Sticky advisories whose range ends before the selected boundary
are labeled historical with current applicability unconfirmed. The flag does
not mean that a finding still applies, has been resolved, or should be
automatically repaired.

An analyzer chooses the flag by what an advisory describes. Mark sticky an advisory
about an event that the analyzer judges once, at the prefix that event ends, and
that later prefixes do not re-emit; otherwise it would leave current reports as
soon as history grows. Keep nonsticky an advisory about current state: it is
emitted again at every prefix where it still holds, so it leaves current reports
once resolved or superseded. Re-emission costs nothing in output size, since the
runner merges it into one range; sticky exists for conditions too costly to
re-judge at every prefix, where report selection keeps the single observation
visible instead.
TypeCheck's warnings (`UnknownSuccessorType`, and
encouraged or deprecated field issues) describe current declarations and current
nodes, so they are nonsticky. Its once-judged transition findings are
inconsistencies, which reports select at any boundary
([SPEC §7.2](SPEC.md#72-successor-type-permission)). Reconnect's suggestions
each judge an event once, a supersession, so they are sticky
([RECONNECT §4](RECONNECT.md#4-sticky)).

History mode admits all replayed observations, including nonsticky advisories,
subject to category and dismissal filters. Grouping first collects all observations
by complete finding key, then selects occurrences for display. Only groups with a
nonempty selected subset are returned; each exposes all its observations as well
as that subset, in deterministic first-emission order.

No generic resolution status is inferred. Non-emission, subject supersession,
or reconnecting only one split branch cannot establish resolution. Historical
observations remain available. An analyzer requiring a current-applicability
assessment must express that assessment through its own domain semantics.

`analysis::presentation` groups, filters, and renders reports. `Dismissals` is
serializable display state, with exact-key listing, lookup, dismissal, and restore
even when the finding is not currently emitted. Each record has a
`DismissalReason` (`Acknowledged`, `FalsePositive`, or `NoLongerRelevant`,
serialized `acknowledged`, `false_positive`, and `no_longer_relevant`) and
`note: Option<String>`. Updating a record replaces its reason and note. No reason
establishes resolution. Dismissal loading, writer exclusion, and atomic
publication belong to the shared application service used by HTTP and CLI.

A proposal may name the suggestion it implements; applying it acknowledges the
finding without asserting resolution ([SUGGESTIONS.md](SUGGESTIONS.md)).

All inconsistencies remain visible across history regardless of advisory filters,
report selection, or imported dismissal records. Dismiss operations reject
inconsistency keys. Transaction acceptance uses the unfiltered report and execution
errors, independently of dismissal reasons, notes, or presentation.

## 4. Explicit suggestions

`analysis::suggestion::prepare_suggestion(graph, observation, runner)` returns
`Result<Checked, Error>` for an explicitly selected suggestion.
It checks only that the observation source matches the supplied history and
that the observation is a suggestion; its ordered actions, `EntryChange` values
like a lowered proposal's, are then checked anew with `transaction::prepare`
under the runner's frozen evaluation inputs, including historical and
intermediate prefixes and the revision-intent rules of
[SPEC §7.4](SPEC.md#74-failure-and-persistence). Exact
existing references and earlier-operation references retain their meaning.
An accepted candidate and its prefixes can be inspected before explicit commit;
a rejection carries only its analysis and operation mapping.
Preparation neither applies the suggestion nor dismisses a finding or infers
reconnections. Exporting suggestions as proposals and verifying their
acknowledgment are application contracts in [SUGGESTIONS.md](SUGGESTIONS.md).
