import { useEffect, useState } from "react";
import type { AnalysisRunParams, EntryId } from "../contract.ts";
import { useData } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { useOpen } from "../links.tsx";
import { aboutEntry, defaultFilter, groupFindings } from "../model/findings.ts";
import { runKey, shownRun, useAnalysisRuns } from "./analysisRuns.ts";
import { GroupRow } from "./FindingRow.tsx";

// The inspector's findings about one entry: the current report of a run
// over the inspected view, without an evaluation time, narrowed to findings
// whose focus or subjects name the entry. Collapsed until asked for, since
// a run replays the whole history; then one run serves every inspection
// until the boundary changes or the screens reload.
export function EntryFindings({ id }: { id: EntryId }) {
  const store = useAnalysisRuns();
  const open = useOpen();
  const { asOf, refresh } = useData();
  const [dismissed, setDismissed] = useState(false);
  const expanded = store.entryOpen();
  const params: AnalysisRunParams = asOf === null ? {} : { as_of: asOf };
  const key = runKey(params, refresh);
  useEffect(() => {
    if (expanded) {
      store.run(key, JSON.parse(key).params as AnalysisRunParams);
    }
  }, [store, expanded, key]);
  const state = store.get(key);
  const ran = shownRun(state);
  let body = null;
  if (expanded) {
    if (ran === null) {
      body =
        state?.status === "failed" ? (
          <FailureView failure={state.failure} compact />
        ) : (
          <p className="muted">running the analyzers…</p>
        );
    } else {
      const analysis = ran.run.analysis;
      const all = aboutEntry(
        groupFindings(analysis, store.dismissals(ran), {
          ...defaultFilter,
          dismissed: true,
        }),
        id,
      );
      const hidden = all.filter((group) => group.dismissal !== null).length;
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
            <button
              type="button"
              disabled={state?.status === "running"}
              onClick={() => store.run(key, params, true)}
            >
              Run again
            </button>
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
            <FailureView failure={state.failure} compact />
          )}
          {groups.length === 0 ? (
            <p className="muted" data-testid="no-entry-findings">
              No current findings about this entry
              {hidden > 0 && !dismissed ? " besides dismissed ones" : ""}.
            </p>
          ) : (
            <ul className="finding-groups">
              {groups.map((group) => (
                <GroupRow key={group.id} group={group} ran={ran} />
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
          onClick={() => store.setEntryOpen(!expanded)}
        >
          Findings about this entry {expanded ? "▾" : "▸"}
        </button>
      </h3>
      {body}
    </section>
  );
}
