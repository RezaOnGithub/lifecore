import { useEffect, useMemo, useState } from "react";
import type { AnalysisRunParams, EntryId } from "../contract.ts";
import { useData } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { useOpen } from "../links.tsx";
import {
  aboutEntry,
  defaultFilter,
  type FindingGroup,
  groupFindings,
} from "../model/findings.ts";
import { type Ran, runKey, shownRun } from "./analysisRuns.ts";
import {
  AnalyzerName,
  GroupRow,
  MAY_BE_STORED,
  PREVIOUS,
} from "./FindingRow.tsx";
import { useAnalysisRuns } from "./useAnalysisRuns.ts";

// How long a changed boundary or reload waits before the open section runs
// for it, so a key passed over on the way is never run.
const SETTLE_MS = 300;

function Analyzers({ ran }: { ran: Ran }) {
  const { implementations } = ran.run.analysis;
  if (implementations.length === 0) {
    return <span className="muted">no analyzers configured</span>;
  }
  return (
    <span className="muted" data-testid="entry-analyzers">
      by{" "}
      {implementations.map((analyzer, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: the suite may repeat an analyzer
        <span key={index}>
          {index > 0 && ", "}
          <AnalyzerName analyzer={analyzer} />
        </span>
      ))}
    </span>
  );
}

// The inspector's findings about one entry: the current report of a run
// over the inspected view, without an evaluation time, narrowed to findings
// whose focus or subjects name the entry. Collapsed until asked for, since
// a run replays the whole history; then one run serves every inspection
// until the boundary changes or the screens reload, and a collapsed
// section counts from it without running.
export function EntryFindings({ id }: { id: EntryId }) {
  const store = useAnalysisRuns();
  const open = useOpen();
  const { asOf, refresh } = useData();
  const [dismissed, setDismissed] = useState(false);
  const expanded = store.entryOpen();
  const params = useMemo(
    (): AnalysisRunParams => (asOf === null ? {} : { as_of: asOf }),
    [asOf],
  );
  useEffect(() => {
    if (!expanded) {
      return;
    }
    const timer = setTimeout(() => store.run(params, refresh), SETTLE_MS);
    return () => clearTimeout(timer);
  }, [store, expanded, params, refresh]);
  const state = store.get(runKey(params, refresh));
  const ran = shownRun(state);
  const all: FindingGroup[] | null =
    ran === null
      ? null
      : aboutEntry(
          groupFindings(ran.run.analysis, store.dismissals(ran), {
            ...defaultFilter,
            dismissed: true,
          }),
          id,
        );
  const hidden = all?.filter((group) => group.dismissal !== null).length ?? 0;
  const count = all === null ? null : all.length - hidden;
  // Expanding reuses a known run, a failed one too; only this replaces it.
  const again = (
    <button
      type="button"
      disabled={state?.status === "running"}
      onClick={() => store.run(params, refresh, true)}
    >
      Run again
    </button>
  );
  let body = null;
  if (expanded) {
    if (ran === null || all === null) {
      body =
        state?.status === "failed" ? (
          <>
            <FailureView failure={state.failure} compact />
            <div className="row">{again}</div>
          </>
        ) : (
          <p className="muted">running the analyzers…</p>
        );
    } else {
      const analysis = ran.run.analysis;
      const groups = dismissed
        ? all
        : all.filter((group) => group.dismissal === null);
      body = (
        <>
          <div className="row">
            <span className="muted">
              current report at [0, {analysis.view.end_exclusive}), evaluation
              time{" "}
              {analysis.inputs.evaluation_time === null
                ? "absent"
                : analysis.inputs.evaluation_time}
            </span>
            <Analyzers ran={ran} />
            {hidden > 0 && (
              <label>
                <input
                  type="checkbox"
                  checked={dismissed}
                  onChange={(event) => setDismissed(event.target.checked)}
                />{" "}
                show dismissed ({hidden})
              </label>
            )}
            {again}
            <button
              type="button"
              className="link"
              onClick={() => open({ kind: "findings" })}
            >
              all findings
            </button>
          </div>
          {state?.status === "running" && (
            <p className="muted">running the analyzers…</p>
          )}
          {state?.status === "failed" && (
            <>
              <FailureView failure={state.failure} compact />
              <p className="muted" data-testid="entry-findings-previous">
                {PREVIOUS}
              </p>
            </>
          )}
          {store.uncertain(ran) && (
            <p className="banner uncertain" data-testid="dismissal-uncertain">
              {MAY_BE_STORED}
            </p>
          )}
          {groups.length === 0 ? (
            <p className="muted" data-testid="no-entry-findings">
              No current findings about this entry
              {hidden > 0 && !dismissed ? " besides dismissed ones" : ""}.
            </p>
          ) : (
            <ul className="finding-groups">
              {groups.map((group) => (
                <GroupRow key={group.id} group={group} ran={ran} about={id} />
              ))}
            </ul>
          )}
        </>
      );
    }
  }
  return (
    <section className="entry-findings" data-testid="entry-findings">
      <h3>
        <button
          type="button"
          className="link disclosure"
          aria-expanded={expanded}
          onClick={() => {
            if (!expanded) {
              // A click runs at once; only later changes of key wait.
              store.run(params, refresh);
            }
            store.setEntryOpen(!expanded);
          }}
        >
          Findings about this entry {expanded ? "▾" : "▸"}
        </button>{" "}
        {count !== null && (
          <span
            className="muted"
            data-testid="entry-findings-count"
            title={`from the run over [0, ${ran?.run.analysis.view.end_exclusive})`}
          >
            {count} current
            {hidden > 0 && `, ${hidden} dismissed`}
          </span>
        )}
      </h3>
      {body}
    </section>
  );
}
