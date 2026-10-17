import type {
  AnalysisRunParams,
  DecimalString,
  MessageType,
} from "../contract.ts";
import { useData } from "../data.tsx";
import { evaluationParams } from "../draft/model.ts";
import { FailureView } from "../failure.tsx";
import { IdLink } from "../links.tsx";
import {
  categoryCounts,
  clusterGroups,
  type FindingFilter,
  filterText,
  groupFindings,
} from "../model/findings.ts";
import {
  type AnalysisRuns,
  type Ran,
  runKey,
  type ScreenMemo,
  shownRun,
} from "../views/analysisRuns.ts";
import { MAY_BE_STORED } from "../views/Dismissal.tsx";
import {
  BulkProgress,
  ClusterRow,
  GroupRow,
  PREVIOUS,
} from "../views/FindingGroups.tsx";
import { labelOf } from "../views/FindingRow.tsx";
import { EvaluationChoice } from "../views/TrayHeader.tsx";
import { useAnalysisRuns } from "../views/useAnalysisRuns.ts";

// The evaluation context every displayed analysis names (INTERFACES §5).
function Context({ ran }: { ran: Ran }) {
  const { view, inputs, implementations } = ran.run.analysis;
  return (
    <table className="payload context" data-testid="findings-context">
      <tbody>
        <tr>
          <th scope="row">view</th>
          <td>
            [0, {view.end_exclusive})
            {view.head !== null && (
              <>
                {" "}
                <span className="muted">newest</span> <IdLink id={view.head} />
              </>
            )}
          </td>
        </tr>
        <tr>
          <th scope="row">evaluation time</th>
          <td>
            {inputs.evaluation_time === null ? (
              <span className="muted">absent</span>
            ) : (
              <code>{inputs.evaluation_time}</code>
            )}
          </td>
        </tr>
        <tr>
          <th scope="row">analyzers</th>
          <td>
            {implementations.length === 0 ? (
              <span className="muted">none configured</span>
            ) : (
              <ul className="implementations">
                {implementations.map((analyzer, index) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: the suite may repeat an analyzer
                  <li key={index}>
                    {analyzer.name} {analyzer.version}{" "}
                    <span className="muted">configuration</span>{" "}
                    <code>{JSON.stringify(analyzer.configuration)}</code>
                  </li>
                ))}
              </ul>
            )}
          </td>
        </tr>
      </tbody>
    </table>
  );
}

const CATEGORIES: { kind: MessageType; words: string }[] = [
  { kind: "inconsistency", words: "inconsistencies" },
  { kind: "warning", words: "warnings" },
  { kind: "suggestion", words: "suggestions" },
];

function Filters({
  ran,
  store,
  memo,
}: {
  ran: Ran;
  store: AnalysisRuns;
  memo: ScreenMemo;
}) {
  const { filter, text } = memo;
  const counts = categoryCounts(
    ran.run.analysis,
    store.dismissals(ran),
    filter.selection,
  );
  const set = (patch: Partial<FindingFilter>) =>
    store.setScreen({ filter: { ...filter, ...patch } });
  return (
    <div className="row findings-filters" data-testid="findings-filters">
      {CATEGORIES.map(({ kind, words }) => {
        const { findings, dismissed } = counts[kind];
        return (
          <label
            key={kind}
            title={
              kind === "inconsistency"
                ? "inconsistencies are always shown"
                : undefined
            }
          >
            <input
              type="checkbox"
              checked={kind === "inconsistency" || filter[kind]}
              disabled={kind === "inconsistency"}
              onChange={(event) =>
                kind !== "inconsistency" &&
                set({ [kind]: event.target.checked })
              }
            />{" "}
            {words}{" "}
            <span
              className={`badge kind-${kind}`}
              data-testid={`count-${kind}`}
            >
              {findings}
            </span>
            {dismissed > 0 && (
              <span className="muted"> ({dismissed} dismissed)</span>
            )}
          </label>
        );
      })}
      <span className="filter-gap" />
      <label title="initialization, what holds at the boundary, and sticky advisories">
        <input
          type="radio"
          name="findings-selection"
          checked={filter.selection === "current"}
          onChange={() => set({ selection: "current" })}
        />{" "}
        current
      </label>
      <label title="every observation of the replayed prefixes">
        <input
          type="radio"
          name="findings-selection"
          checked={filter.selection === "history"}
          onChange={() => set({ selection: "history" })}
        />{" "}
        history
      </label>
      <span className="filter-gap" />
      <label>
        <input
          type="checkbox"
          checked={filter.dismissed}
          onChange={(event) => set({ dismissed: event.target.checked })}
        />{" "}
        show dismissed
      </label>
      <span className="filter-gap" />
      <input
        type="search"
        aria-label="filter by message or focus"
        placeholder="message or focus"
        title="advisories whose message or focus label contains this; inconsistencies are always shown"
        size={24}
        value={text}
        onChange={(event) => store.setScreen({ text: event.target.value })}
      />
    </div>
  );
}

function Report({
  ran,
  store,
  memo,
}: {
  ran: Ran;
  store: AnalysisRuns;
  memo: ScreenMemo;
}) {
  const { filter, text, expanded } = memo;
  const groups = filterText(
    groupFindings(ran.run.analysis, store.dismissals(ran), filter),
    text,
    labelOf(ran.nodes),
  );
  const clusters = clusterGroups(groups);
  const analyzers = ran.run.analysis.implementations.length;
  const bulk = store.bulk();
  const expand = (id: string, open: boolean) =>
    store.setScreen({
      expanded: open
        ? [...expanded.filter((other) => other !== id), id]
        : expanded.filter((other) => other !== id),
    });
  return (
    <>
      <Context ran={ran} />
      {ran.labelFailure !== null && (
        <p className="muted">
          Some entries are shown by position: their labels could not be read.
        </p>
      )}
      <Filters ran={ran} store={store} memo={memo} />
      {bulk !== null && (
        <BulkProgress
          bulk={bulk}
          store={store}
          onShow={() => expand(bulk.cluster, true)}
        />
      )}
      {clusters.length === 0 ? (
        <p className="muted" data-testid="no-findings">
          No findings on show.
        </p>
      ) : (
        <ul className="finding-groups" data-testid="finding-groups">
          {clusters.map((cluster) => {
            const [only] = cluster.groups;
            return cluster.groups.length === 1 && only !== undefined ? (
              <GroupRow
                key={only.id}
                group={only}
                ran={ran}
                selection={filter.selection}
              />
            ) : (
              <ClusterRow
                key={cluster.id}
                cluster={cluster}
                ran={ran}
                selection={filter.selection}
                analyzers={analyzers}
                expanded={expanded.includes(cluster.id)}
                onExpand={(open) => expand(cluster.id, open)}
              />
            );
          })}
        </ul>
      )}
    </>
  );
}

// Whether the top bar now selects another view than the run's. A
// whole-history run of [0, N) is the view as of N; a run as of N is not
// known to be the whole history.
function moved(
  ran: Ran,
  last: { asOf: DecimalString | null },
  asOf: DecimalString | null,
): boolean {
  return asOf === null
    ? last.asOf !== null
    : asOf !== ran.run.analysis.view.end_exclusive;
}

// Findings of the stored database: the stored suite replayed over the
// selected history on request, grouped by finding (SPEC §8.3). Running is
// explicit because replay can take seconds; the last run stays on show,
// labeled with its own view and inputs, until the next one.
export function FindingsScreen() {
  const store = useAnalysisRuns();
  const { asOf, refresh } = useData();
  const memo = store.screen();
  const { evaluation, last } = memo;
  const state = last === null ? undefined : store.get(last.key);
  const ran = shownRun(state);
  const running = state?.status === "running";
  const run = () => {
    const params: AnalysisRunParams = {
      ...(asOf === null ? {} : { as_of: asOf }),
      ...evaluationParams(evaluation),
    };
    // Pinned before it starts, so no other run evicts it.
    store.setScreen({ last: { key: runKey(params, refresh), asOf, refresh } });
    store.run(params, refresh, true);
  };
  return (
    <section aria-label="Findings" data-testid="findings-screen">
      <h2>Findings</h2>
      <div className="row">
        <EvaluationChoice
          evaluation={evaluation}
          disabled={running}
          onChange={(next) => store.setScreen({ evaluation: next })}
        />
        <button
          type="button"
          disabled={
            running ||
            (evaluation.kind === "at" && evaluation.time.trim() === "")
          }
          onClick={run}
        >
          Run
        </button>
        {running && <span className="muted">running the analyzers…</span>}
        <span className="muted">
          {asOf === null
            ? "over the whole history"
            : `over the view [0, ${asOf})`}
        </span>
      </div>
      {state === undefined && (
        <p className="muted">
          Run replays the stored suite of analyzers over every prefix of the
          selected history; on a large database it takes a while. Each Run reads
          the stored state afresh, including writes by other clients.
        </p>
      )}
      {state?.status === "failed" && <FailureView failure={state.failure} />}
      {ran !== null && last !== null && (
        <>
          {state?.status === "failed" && (
            <p className="banner asof" data-testid="findings-previous">
              {PREVIOUS}
            </p>
          )}
          {moved(ran, last, asOf) ? (
            <p className="banner asof" data-testid="findings-moved">
              This run is of [0, {ran.run.analysis.view.end_exclusive}), and the
              top bar now selects{" "}
              {asOf === null ? "the whole history" : `[0, ${asOf})`}: Run again
              to analyze that.
            </p>
          ) : (
            last.refresh !== refresh && (
              <p className="banner asof" data-testid="findings-reloaded">
                The screens were reloaded since this run: Run again for the
                stored state as it is now.
              </p>
            )
          )}
          {store.uncertain(ran) && (
            <p className="banner uncertain" data-testid="dismissal-uncertain">
              {MAY_BE_STORED}
            </p>
          )}
          <Report ran={ran} store={store} memo={memo} />
        </>
      )}
    </section>
  );
}
