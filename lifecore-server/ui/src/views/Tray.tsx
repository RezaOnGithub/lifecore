import { useState } from "react";
import type { DecimalString } from "../contract.ts";
import { useData } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { attachFindings } from "../draft/findings.ts";
import type { Screen } from "../navigation.ts";
import { EARLIER, FindingList, judged } from "./Findings.tsx";
import { OperationRows, rowsOf } from "./OperationRow.tsx";
import { statusWord, TrayHeader } from "./TrayHeader.tsx";

// What the last report says beyond single operations: findings of
// operations no longer in the draft, of the committed base, and of the
// suite's initialization.
function ReportSummary() {
  const { state } = useDraft();
  const { bumpRefresh } = useData();
  const { outcome } = state;
  const judgment = judged(state);
  if (outcome === null || judgment === null) {
    return null;
  }
  const attachment = attachFindings(outcome.report);
  const list = (findings: typeof attachment.history) =>
    findings.length === 0 ? (
      <p className="muted">none</p>
    ) : (
      <FindingList
        findings={findings}
        names={judgment.names}
        current={judgment.current}
      />
    );
  return (
    <section className="report" data-testid="report">
      <h3>
        {outcome.source === "check" ? "Check" : "Apply"}:{" "}
        {outcome.accepted ? "accepted" : "rejected"}
      </h3>
      <p className="muted">
        base [0, {outcome.report.base.end_exclusive}), evaluation time{" "}
        {outcome.report.analysis.inputs.evaluation_time ?? "none"}
        {!judgment.current && "; the draft changed since"}
      </p>
      {judgment.unplaced.length > 0 && (
        <>
          <h4>for operations no longer in the draft</h4>
          {list(judgment.unplaced)}
        </>
      )}
      <h4>already in history</h4>
      {list(attachment.history)}
      {attachment.settled > 0 && (
        <p className="muted">
          {attachment.settled} more resolved before the base, not listed
        </p>
      )}
      <h4>initialization</h4>
      {list(attachment.initialization)}
      <button type="button" className="link" onClick={bumpRefresh}>
        refresh the screens
      </button>
    </section>
  );
}

// A right-hand column: a fixed header with the calls and banners, and the
// operations below it, which scroll on their own. `head` is the current
// entry count, the base the operations' provisional positions start from
// until a check of this content names its own.
export function Tray({
  screen,
  head,
}: {
  screen: Screen;
  head: DecimalString | null;
}) {
  const { state } = useDraft();
  const [collapsed, setCollapsed] = useState(false);
  const { entries } = state.proposal;
  if (collapsed) {
    return (
      <aside className="tray collapsed" aria-label="Draft" data-testid="tray">
        <button
          type="button"
          onClick={() => setCollapsed(false)}
          aria-label="expand the draft tray"
          title="expand the draft tray"
        >
          ▸
        </button>
        <span className="strip">
          Draft: {entries.length} operation{entries.length === 1 ? "" : "s"},{" "}
          {statusWord(state)}
        </span>
      </aside>
    );
  }
  const judgment = judged(state);
  const base =
    judgment?.current === true && state.outcome !== null
      ? state.outcome.report.base.end_exclusive
      : head;
  const selected =
    screen.kind === "operation" ? screen.name : (state.selected ?? null);
  return (
    <aside className="tray" aria-label="Draft" data-testid="tray">
      <TrayHeader screen={screen} onCollapse={() => setCollapsed(true)} />
      <div className="tray-body">
        {entries.length === 0 ? (
          <p className="muted">
            No operations. Use "New node of this type" on a table, Revise,
            Branch, and Connect in the inspector, New edge, or Import.
          </p>
        ) : (
          <>
            {judgment !== null && !judgment.current && (
              <p className="muted dim-note" data-testid="earlier">
                Statuses and findings are {EARLIER}.
              </p>
            )}
            <OperationRows
              rows={rowsOf(entries, state.kept)}
              base={base}
              selected={selected}
              judgment={judgment}
            />
          </>
        )}
        <ReportSummary />
      </div>
    </aside>
  );
}
