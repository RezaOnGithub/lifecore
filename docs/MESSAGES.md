# Analyzer message flow

This describes the current library implementation. For the contract, see
[SPEC §8](SPEC.md#8-analyzers). The diagrams follow owned values into
`AnalysisOutput`, then borrowed references into presentation groups.

The planned application exports suggestions as proposals and, after verifying
them against its own replay, publishes their graph changes with finding
acknowledgment. That layer is described in [SUGGESTIONS.md](SUGGESTIONS.md); the
library flow below does not dismiss findings on commit.

A sticky message's certificate moves into `Observation.key` during normalization,
just like any other certificate. `AnalysisOutput::groups` borrows that key into a
`FindingGroup` before applying display filters. The group is returned only if at
least one of its observations is selected. `sticky` satisfies the boundary test
for an advisory; category and dismissal filters still apply.

## From analyzer findings to observations

```mermaid
flowchart TD
    T["TypedAnalyzer<br/>Vec&lt;Self::MSG&gt;: domain findings"]
    C["convert_message<br/>Analyzer owns certificate encoding and presentation"]
    A["Direct Analyzer implementation"]
    M["Vec&lt;Message&gt;<br/>Order and duplicates preserved by typed adapter"]
    V{"Runner record:<br/>subjects and focus visible in the prefix?"}
    E["Err: InvalidAnalyzerOutput<br/>No AnalysisOutput returned"]
    K["FindingKey<br/>database + analyzer identity + category + certificate"]
    R["Runner context<br/>database, AnalyzerIdentity,<br/>EvaluationInputs, EvaluatedView"]
    O["Observation<br/>key, evaluated range, and the Diagnostic moved whole"]
    D{"Same key and Diagnostic<br/>already recorded?"}
    X["Extend that observation's last prefix"]
    DROP["Drop: already covered"]
    P["AnalysisOutput.observations<br/>Append with first = last = this prefix"]

    T --> C --> M
    A --> M
    M --> V
    V -->|no| E
    V -->|yes: move certificate; derive category| K
    R -->|database and analyzer identity| K
    K -->|owned key| O
    V -->|yes: move the Diagnostic| O
    R -->|evaluated view| O
    O --> D
    D -->|yes, ending at the preceding prefix| X
    D -->|yes, covering this prefix| DROP
    D -->|no, or only ranges ending earlier| P
```

Initialization messages have no prefix, so an identical one is dropped and
never merges with a prefix observation. A diagnostic whose previous range ended
more than one prefix ago starts a new observation, which is how a gap is kept.

`AnalysisRunner` constructs all configured analyzers through `AnalyzerFactory`.
The factory receives the original `Config.configuration` string unchanged. The runner
builds each `AnalyzerIdentity` from the registered `Config.name`, the analyzer's
`version()`, and that exact configuration string. Equivalent configuration
spellings can therefore produce different finding keys.

All analyzers initialize in suite order, receiving only frozen `EvaluationInputs`.
Then the first analyzer evaluates every prefix `0..=end_exclusive`, followed by
the second analyzer's full replay, and so on. Prefix `n` contains entries `[0, n)`.
Initialization and prefix zero are separate invocations, even for an empty graph.
The same inputs are passed to every invocation; no clock is read implicitly.
Because every prefix is evaluated, an analyzer can report an event once, at the
prefix whose newest entry it concerns.
[SPEC §8.3](SPEC.md#83-presentation-and-dismissal) states when such an advisory
must be sticky, and [SPEC §7.2](SPEC.md#72-successor-type-permission) how
TypeCheck judges revision transitions this way.

| Information supplied by | Destination | Rule |
| --- | --- | --- |
| `Message.analyzer_certificate` | `Observation.key.analyzer_certificate` | Move the complete `AnalyzerCertificate` string newtype, whether sticky or not. Shared code does not interpret its contents. |
| Selected graph/view and factory identity | `Observation.key.database`, `.analyzer` | Add database identity, registered name, exact version and original configuration string. |
| `Message.diagnostic.kind.ty()` | `Observation.key.kind` | Store only the `MessageType` category: inconsistency, warning or suggestion. |
| `Message.diagnostic` | `Observation.diagnostic` | Move the whole `Diagnostic`: subjects, focus, text, `MessageKind` with a suggestion's ordered `EntryChange` actions, and `sticky`. |
| Runner invocation | `Observation.evaluated_view` | `Initialization`, or `Prefixes { first, last }`, extended when the same key and diagnostic recur at the next prefix. Frozen inputs live only on the output. |
| Selected view, frozen inputs, constructed identities | `AnalysisOutput.view`, `.inputs`, `.implementations` | Retain the run context even if no observations were emitted. Implementation identities preserve suite order and duplicates. |

An initialization message has no focus and no subjects. For replay, the
subjects and the focus entry must be visible in the evaluated prefix;
`evaluated_view` records the prefixes at which the analyzer emitted it.

Certificate encoding belongs to the analyzer. Built-ins encode typed findings,
including exact subjects and distinguishing domain details, as deterministic
JSON. The runner does not derive a certificate from the diagnostic. Wording,
focus, actions and `sticky` are diagnostic details, so changing them produces a
distinct observation of the same finding key, while re-emitting the same
diagnostic at the next prefix extends the existing observation's range. Merging
compares key and diagnostic; grouping and dismissal compare whole finding keys.
Neither comparison treats a hash collision as equality.

Missing factories and invalid configuration return errors. If an analyzer
returns `Err(String)`, the runner creates an `ExecutionFailure` with analyzer
identity, inputs, evaluated view and error text, wrapped in
`LifeCoreError::AnalyzerExecution`. These failures do not become observations.
Any execution or normalization error aborts the run without returning a partial
`AnalysisOutput`. Emitting an inconsistency is different: replay continues and
the successful output contains that observation for the acceptance check below.

## From observations to displayed groups

Each call to `AnalysisOutput::groups(dismissals, options)` builds new groups.
`FindingGroup` borrows its key and observations from the output, and its optional
dismissal from `Dismissals`. The following selection decisions run for **every
observation**, after its reference has been added to `group.observations`.

```mermaid
flowchart TD
    O["AnalysisOutput.observations()<br/>Each Observation in emission order"]
    G["Find or create FindingGroup by complete FindingKey<br/>key = borrowed Observation.key"]
    DS["Dismissals<br/>FindingKey + reason + optional note"]
    ALL["Always append reference to group.observations"]
    I{"Observation.diagnostic.kind<br/>is Inconsistency?"}
    C{"ReportOptions.advisories<br/>allows this category?"}
    B{"History mode OR Initialization<br/>OR last == AnalysisOutput.view<br/>OR sticky?"}
    D{"No group.dismissal<br/>OR include_dismissed?"}
    YES["Append reference to group.selected"]
    NO["Keep only in group.observations"]
    DONE["After processing ALL observations"]
    KEEP{"group.selected is nonempty?"}
    OUT["Return FindingGroup<br/>key, all observations, selected subset, dismissal"]
    HIDE["Omit group from returned Vec<br/>AnalysisOutput remains unchanged"]

    O --> G
    DS -->|lookup by exact key on group creation| G
    G --> ALL --> I
    I -->|yes: bypass all advisory filters| YES
    I -->|no: warning or suggestion| C
    C -->|no| NO
    C -->|yes| B
    B -->|no| NO
    B -->|yes| D
    D -->|no| NO
    D -->|yes| YES
    YES --> DONE
    NO --> DONE
    DONE --> KEEP
    KEEP -->|yes| OUT
    KEEP -->|no| HIDE
```

The returned group's certificate is `group.key.analyzer_certificate`: a field
of the borrowed key, not a separate certificate collection. The certificate
alone is insufficient to join a group; database, analyzer identity and category
must also match. Groups follow the order of their first observation.

`group.observations` includes every normalized occurrence of that key in this
output, including occurrences excluded from `group.selected`. A selected
occurrence keeps the group in the result even when other occurrences are hidden.
`render` prints each returned group's category, analyzer name and certificate,
then only its selected observations. A selected sticky occurrence whose range
ends before the selected view is labeled “historical advisory; current
applicability unconfirmed”.

For example, suppose a warning is emitted only at prefix 4, so its range is
`[4, 4]`, and the selected view is prefix 8. Its normalized observation remains in the output in every row below.
Assuming it is the key's **only occurrence**, these are the grouping outcomes:

| Sticky | Selection | Advisory filter | Matching dismissal | Include dismissed | Selected and group returned? |
| --- | --- | --- | --- | --- | --- |
| No | Current | All | No | No | No: its range ends before the view. |
| Yes | Current | All | No | No | Yes: sticky passes the boundary test. |
| Yes | Current | All | Yes | No | No: dismissal hides it. |
| Yes | Current | All | Yes | Yes | Yes: dismissed findings are requested. |
| Yes | Current | Suggestions or None | No | No | No: warnings are filtered out. |
| No | History | All | No | No | Yes: history passes the boundary test. |
| Yes | History | All | Yes | No | No: history does not bypass dismissal. |

Defaults are `Current`, `All`, and `include_dismissed = false`. Initialization
observations and observations whose range ends at the selected view pass the
boundary test without `sticky`. A nonsticky warning that still holds at the head
has been re-emitted at every prefix since it appeared, so its range ends at the
view and it is selected; its `first` prefix tells when it appeared. Inconsistencies are always selected, regardless of boundary,
sticky flag, advisory filter or even an imported dismissal record.

Sticky is a property of an occurrence, not its finding key. It has no automatic
expiry and does not require re-emission at the selected head. With fixed analyzer
implementation, configuration and inputs, full replay recovers that earlier
occurrence. An analysis ending before it was emitted cannot recover it, and a
later run with different inputs need not emit it. Grouping never copies findings
from a previous `AnalysisOutput` or invents them from dismissal records.

`Dismissals::dismiss` stores an owned complete key, a reason and an optional note;
it rejects inconsistency keys. Updating a record replaces its reason and note.
`restore` removes the matching record. Neither operation changes observations:
a later `groups` call consults the new display state. No dismissal reason,
non-emission or subject supersession establishes resolution.

## From observations to transaction checks

Display is one consumer of `AnalysisOutput`. The `observations`, `errors`,
`warnings` and `suggestions` iterators expose unfiltered occurrences (the latter
three restrict category only). Transaction preparation checks the unfiltered
output too. A caller may explicitly choose a suggestion observation for checking;
being selected for display does not automatically apply its actions.

```mermaid
flowchart TD
    O["Caller chooses an Observation"]
    S{"prepare_suggestion:<br/>matching source and kind = Suggestion?"}
    A["Observation.diagnostic.kind = Suggestion(actions)<br/>Vec&lt;EntryChange&gt;, the type a lowered proposal has"]
    P["transaction::prepare(graph, changes, runner)<br/>Stage; validate structure and revision intent at each prefix"]
    R["Runner checks all candidate prefixes<br/>Fresh unfiltered AnalysisOutput"]
    G{"Any Inconsistency?"}
    J["Checked::Rejected(Rejection)<br/>complete AnalysisOutput and operation mapping; no candidate graph"]
    T["Checked::Accepted(PreparedTransaction)<br/>owns the candidate and its AnalysisOutput"]
    C{"Caller explicitly commits;<br/>original base still matches exactly?"}
    OK["Replace graph atomically in memory"]
    E["Return error<br/>Original graph unchanged"]

    O --> S
    S -->|yes| A --> P
    S -->|no| E
    P -->|valid| R
    P -->|invalid structure, references or intent| E
    R -->|success| G
    R -->|execution or normalization failure| E
    G -->|yes| J
    G -->|no| T --> C
    C -->|yes| OK
    C -->|no: StaleBase| E
```

This diagram follows the suggestion path; direct `transaction::prepare` behaves
the same after staging, including the intent check. Preparation uses the supplied runner's frozen
inputs, not automatically the selected observation's original inputs. Source
checking uses `Observation.key.database` and, for a prefix observation, both
ends of its `evaluated_view` range. The opaque certificate is not an action or a commit receipt.
`sticky`, `ReportOptions` and `Dismissals` are not inputs to transaction acceptance.
Warnings and suggestions do not block it. Commit checks the original database,
entry count/content and ordered analyzer suite before replacing the graph.

Implementation: [runner and normalization](../src/analysis/mod.rs),
[identity types](../src/analysis/identity.rs),
[grouping and dismissal](../src/analysis/presentation.rs),
[suggestion preparation](../src/analysis/suggestion.rs),
[transaction checks](../src/transaction/mod.rs).
