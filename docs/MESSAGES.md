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
    V{"Runner record:<br/>subjects, relevance and location valid?"}
    E["Err: InvalidAnalyzerOutput<br/>No AnalysisOutput returned"]
    K["FindingKey<br/>database + analyzer identity + category + certificate"]
    R["Runner context<br/>database, AnalyzerIdentity,<br/>EvaluationInputs, EvaluatedView"]
    O["Observation<br/>Owns key and occurrence / presentation fields"]
    D{"Whole Observation<br/>already recorded?"}
    X["Discard exact duplicate<br/>Keep the first observation"]
    P["AnalysisOutput.observations<br/>Append in emission order"]

    T --> C --> M
    A --> M
    M --> V
    V -->|no| E
    V -->|yes: move certificate; derive category| K
    R -->|database and analyzer identity| K
    K -->|owned key| O
    V -->|yes: move other Message fields| O
    R -->|frozen inputs and evaluated view| O
    O --> D
    D -->|yes| X
    D -->|no| P
```

`AnalysisRunner` constructs all configured analyzers through `AnalyzerFactory`.
The factory receives the original `Config.config` string unchanged. The runner
builds each `AnalyzerIdentity` from the registered `Config.name`, the analyzer's
`version()`, and that exact configuration string. Equivalent configuration
spellings can therefore produce different finding keys.

All analyzers initialize in suite order, receiving only frozen `EvaluationInputs`.
Then the first analyzer evaluates every prefix `0..=end_exclusive`, followed by
the second analyzer's full replay, and so on. Prefix `n` contains entries `[0, n)`.
Initialization and prefix zero are separate invocations, even for an empty graph.
The same inputs are passed to every invocation; no clock is read implicitly.
Because every prefix is evaluated, an analyzer can judge an event once, at the
prefix whose newest entry it is. TypeCheck does so for revision transitions
([SPEC §7.2](SPEC.md#72-successor-type-permission)): each judgment contributes one
observation, at that boundary, and reports select it at any head as an
inconsistency.

| Information supplied by | Destination | Rule |
| --- | --- | --- |
| `Message.analyzer_certificate` | `Observation.key.analyzer_certificate` | Move the complete `AnalyzerCertificate` string newtype, whether sticky or not. Shared code does not interpret its contents. |
| Selected graph/view and factory identity | `Observation.key.database`, `.analyzer` | Add database identity, registered name, exact version and original configuration string. |
| `Message.kind.ty()` | `Observation.key.kind` | Store only the `MessageType` category: inconsistency, warning or suggestion. |
| `Message.kind` | `Observation.kind` | Keep the complete `MessageKind`, including a suggestion's ordered actions. |
| `Message.subjects`, `.offending_event`, `.relevance`, `.message`, `.sticky` | Corresponding `Observation` fields | Move these fields directly; no nested `Message` remains. |
| Runner inputs and invocation | `Observation.inputs`, `.evaluated_view` | Record frozen `EvaluationInputs` and either `Initialization` or `Prefix(ViewId)`. |
| Selected view, frozen inputs, constructed identities | `AnalysisOutput.view`, `.inputs`, `.implementations` | Retain the run context even if no observations were emitted. Implementation identities preserve suite order and duplicates. |

An initialization message must have no location, subjects or relevance entries.
For replay, subjects and relevance entries must be visible in the evaluated
prefix. An `offending_event`, if supplied, must match an actual location in that
prefix or one of its earlier prefixes. It can describe an earlier event;
`evaluated_view` records when the analyzer evaluated it.

Certificate encoding belongs to the analyzer. Built-ins encode typed variants,
exact subjects and distinguishing domain details as deterministic JSON. The
runner does not derive a certificate from the other `Message` fields. Evaluation
boundaries, wording, actions and `sticky` are observation details, so changing
them can produce distinct observations of the same finding key. Deduplication
compares whole observations; grouping and dismissal compare whole finding keys.
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
    I{"Observation.kind<br/>is Inconsistency?"}
    C{"ReportOptions.advisories<br/>allows this category?"}
    B{"History mode OR Initialization<br/>OR Prefix(AnalysisOutput.view)<br/>OR Observation.sticky?"}
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
then only its selected observations. A selected sticky occurrence from an earlier
prefix is labeled “historical advisory; current applicability unconfirmed”.

For example, suppose a warning is emitted only at prefix 4 and the selected view
is prefix 8. Its normalized observation remains in the output in every row below.
Assuming it is the key's **only occurrence**, these are the grouping outcomes:

| Sticky | Selection | Advisory filter | Matching dismissal | Include dismissed | Selected and group returned? |
| --- | --- | --- | --- | --- | --- |
| No | Current | All | No | No | No: older nonsticky occurrence. |
| Yes | Current | All | No | No | Yes: sticky passes the boundary test. |
| Yes | Current | All | Yes | No | No: dismissal hides it. |
| Yes | Current | All | Yes | Yes | Yes: dismissed findings are requested. |
| Yes | Current | Suggestions or None | No | No | No: warnings are filtered out. |
| No | History | All | No | No | Yes: history passes the boundary test. |
| Yes | History | All | Yes | No | No: history does not bypass dismissal. |

Defaults are `Current`, `All`, and `include_dismissed = false`. Initialization
observations and observations at the exact selected view pass the boundary test
without `sticky`. Inconsistencies are always selected, regardless of boundary,
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
    A["Observation.kind = Suggestion(actions)<br/>SuggestedAction shares NewNode/NewEdge with Change"]
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
checking uses `Observation.key.database` and, for a prefix observation, its
`evaluated_view`. The opaque certificate is not an action or a commit receipt.
`sticky`, `ReportOptions` and `Dismissals` are not inputs to transaction acceptance.
Warnings and suggestions do not block it. Commit checks the original database,
entry count/content and ordered analyzer suite before replacing the graph.

Implementation: [runner and normalization](../src/analysis/mod.rs),
[identity types](../src/analysis/identity.rs),
[grouping and dismissal](../src/analysis/presentation.rs),
[suggestion preparation](../src/analysis/suggestion.rs),
[transaction checks](../src/transaction/mod.rs).
