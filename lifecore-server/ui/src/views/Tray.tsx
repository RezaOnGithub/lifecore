import { useState } from "react";
import type { DecimalString, Observation } from "../contract.ts";
import { useData } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import {
  type Attached,
  type Attachment,
  attachFindings,
  countFindings,
} from "../draft/findings.ts";
import { kindWords } from "../model/findings.ts";
import type { Screen } from "../navigation.ts";
import { EARLIER, FindingList, judged } from "./Findings.tsx";
import { OperationRows, rowsOf } from "./OperationRow.tsx";
import { statusWord, TrayHeader } from "./TrayHeader.tsx";

// Findings first emitted in the committed history: often many, and not
// this draft's doing. Inconsistencies are never folded (ANALYZERS §3), so they
// are listed, those the base no longer emits included; the advisories still
// at the base fold into one line with their counts until expanded, and
// those no longer emitted there are only counted.
function HistoryFindings({
  attachment,
  observations,
  names,
  current,
}: {
  attachment: Attachment;
  observations: readonly Observation[];
  names: readonly string[];
  current: boolean;
}) {
  const [open, setOpen] = useState(false);
  const inconsistent = (attached: Attached) =>
    attached.observation.diagnostic.kind === "inconsistency";
  const inconsistencies = [...attachment.history, ...attachment.ended]
    .filter(inconsistent)
    .sort(
      (a, b) =>
        observations.indexOf(a.observation) -
        observations.indexOf(b.observation),
    );
  const advisories = attachment.history.filter((a) => !inconsistent(a));
  const { endedAdvisories } = attachment;
  return (
    <>
      {inconsistencies.length > 0 && (
        <>
          <h4 data-testid="history-inconsistencies">
            already in history: {kindWords(countFindings(inconsistencies))}
          </h4>
          <FindingList
            findings={inconsistencies}
            names={names}
            current={current}
          />
        </>
      )}
      {advisories.length > 0 ? (
        <>
          <h4>
            <button
              type="button"
              className="link disclosure"
              aria-expanded={open}
              data-testid="history-findings"
              onClick={() => setOpen(!open)}
            >
              already in history: {kindWords(countFindings(advisories))}{" "}
              {open ? "▾" : "▸"}
            </button>
          </h4>
          {open && (
            <FindingList
              findings={advisories}
              names={names}
              current={current}
            />
          )}
        </>
      ) : (
        inconsistencies.length === 0 && (
          <>
            <h4>already in history</h4>
            <p className="muted">none</p>
          </>
        )
      )}
      {endedAdvisories > 0 && (
        <p className="muted" data-testid="history-ended">
          {endedAdvisories} more advisor{endedAdvisories === 1 ? "y" : "ies"}{" "}
          from history no longer emitted at the base, not listed
        </p>
      )}
    </>
  );
}

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
      <HistoryFindings
        attachment={attachment}
        observations={outcome.report.analysis.observations}
        names={judgment.names}
        current={judgment.current}
      />
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
  const { entries } = state.proposal;
  // An empty draft with nothing to report leaves the room to the screens:
  // the tray folds to a strip until an operation arrives or the user opens
  // it. The user's choice holds until the draft becomes empty or stops
  // being so.
  const idle =
    entries.length === 0 &&
    state.outcome === null &&
    state.applied === null &&
    state.uncertain === null &&
    state.problem === null &&
    state.stale === null &&
    state.picking === null &&
    state.pending === null &&
    state.notices.length === 0;
  const [choice, setChoice] = useState<boolean | null>(null);
  const [wasIdle, setWasIdle] = useState(idle);
  if (wasIdle !== idle) {
    setWasIdle(idle);
    setChoice(null);
  }
  const collapsed = choice ?? idle;
  if (collapsed) {
    return (
      <aside className="tray collapsed" aria-label="Draft" data-testid="tray">
        <button
          type="button"
          className="tray-strip"
          onClick={() => setChoice(false)}
          aria-label="expand the draft tray"
          title="expand the draft tray"
        >
          <span aria-hidden="true">◂</span>
          <span className="strip">
            Draft:{" "}
            <span data-testid="draft-count">
              {entries.length} operation{entries.length === 1 ? "" : "s"}
            </span>
            {!idle && `, ${statusWord(state)}`}
          </span>
        </button>
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
      <TrayHeader screen={screen} onCollapse={() => setChoice(true)} />
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
