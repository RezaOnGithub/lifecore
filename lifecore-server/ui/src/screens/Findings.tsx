import type { AnalysisRunParams, MessageType } from "../contract.ts";
import { useData } from "../data.tsx";
import { evaluationParams } from "../draft/model.ts";
import { FailureView } from "../failure.tsx";
import { IdLink } from "../links.tsx";
import {
  categoryCounts,
  type FindingFilter,
  groupFindings,
} from "../model/findings.ts";
import {
  type AnalysisRuns,
  type Ran,
  runKey,
  shownRun,
  useAnalysisRuns,
} from "../views/analysisRuns.ts";
import { GroupRow } from "../views/FindingRow.tsx";
import { EvaluationChoice } from "../views/TrayHeader.tsx";

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
  filter,
}: {
  ran: Ran;
  store: AnalysisRuns;
  filter: FindingFilter;
}) {
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
    </div>
  );
}

function Report({
  ran,
  store,
  filter,
}: {
  ran: Ran;
  store: AnalysisRuns;
  filter: FindingFilter;
}) {
  const groups = groupFindings(ran.run.analysis, store.dismissals(ran), filter);
  return (
    <>
      <Context ran={ran} />
      {ran.labels !== null && (
        <p className="muted">
          Entries are shown by position: their labels could not be read.
        </p>
      )}
      <Filters ran={ran} store={store} filter={filter} />
      {groups.length === 0 ? (
        <p className="muted" data-testid="no-findings">
          No findings on show.
        </p>
      ) : (
        <ul className="finding-groups" data-testid="finding-groups">
          {groups.map((group) => (
            <GroupRow key={group.id} group={group} ran={ran} />
          ))}
        </ul>
      )}
    </>
  );
}

// Findings of the stored database: the stored suite replayed over the
// selected history on request, grouped by finding (SPEC §8.3). Running is
// explicit because replay can take seconds; the last run stays on show,
// labeled with its own view and inputs, until the next one.
export function FindingsScreen() {
  const store = useAnalysisRuns();
  const { asOf, refresh } = useData();
  const { evaluation, filter, last } = store.screen();
  const state = last === null ? undefined : store.get(last.key);
  const ran = shownRun(state);
  const running = state?.status === "running";
  const run = () => {
    const params: AnalysisRunParams = {
      ...(asOf === null ? {} : { as_of: asOf }),
      ...evaluationParams(evaluation),
    };
    const key = runKey(params, refresh);
    store.setScreen({ last: { key, asOf, refresh } });
    store.run(key, params, true);
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
          selected history; on a large database it takes a while.
        </p>
      )}
      {state?.status === "failed" && <FailureView failure={state.failure} />}
      {ran !== null && last !== null && (
        <>
          {last.asOf !== asOf ? (
            <p className="banner asof" data-testid="findings-moved">
              This run is of [0, {ran.run.analysis.view.end_exclusive}), and the
              top bar now selects{" "}
              {asOf === null ? "the whole history" : `[0, ${asOf})`}: Run again
              to analyze that.
            </p>
          ) : (
            last.refresh !== refresh && (
              <p className="muted">
                The screens were reloaded since this run: Run again for the
                stored state as it is now.
              </p>
            )
          )}
          <Report ran={ran} store={store} filter={filter} />
        </>
      )}
    </section>
  );
}
