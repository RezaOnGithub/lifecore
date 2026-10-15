import { useState } from "react";
import type { OperationEntry } from "../contract.ts";
import { useData } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { countWords, reportCounts } from "../draft/findings.ts";
import {
  type Applied,
  applicable,
  type Problem,
  type Uncertain,
  type Verdict,
} from "../draft/model.ts";
import { IdLink, OperationLink } from "../links.tsx";

function AssignedIds({ operations }: { operations: OperationEntry[] }) {
  return (
    <ul>
      {operations.map((operation) => (
        <li key={operation.name}>
          {operation.name} → <IdLink id={operation.entry} />
        </li>
      ))}
    </ul>
  );
}

function verdictText(verdict: Verdict): string {
  switch (verdict.kind) {
    case "unchanged":
      return "The graph revision is the one the check saw: nothing was written since, so the draft was not applied.";
    case "absent":
      return "No entries were added after the check's base: the draft was not applied.";
    case "other":
      return "The positions the draft would have taken hold other entries: it was not applied; another writer moved the graph.";
    case "present":
      return "Entries matching every operation are at the positions the draft would have taken: it was applied.";
    case "failed":
      return `Could not read the graph: ${verdict.message}`;
  }
}

// Stays above every other banner until the user decides; Apply and Replan
// wait for that decision.
export function UncertainBanner({ uncertain }: { uncertain: Uncertain }) {
  const { state, dispatch, actions } = useDraft();
  const { bumpRefresh } = useData();
  const [looking, setLooking] = useState(false);
  const applied =
    uncertain.outcome?.kind === "applied" ? uncertain.outcome : null;
  const { verdict } = uncertain;
  const present = applied !== null || verdict?.kind === "present";
  const look = async () => {
    setLooking(true);
    await actions.lookForEntries();
    setLooking(false);
  };
  return (
    <div className="banner uncertain" role="alert" data-testid="uncertain">
      <strong>This draft may already have been applied.</strong>{" "}
      <code>{uncertain.error}</code>: {uncertain.message}
      {applied !== null && (
        <>
          <p className="note">
            The server reports this outcome: graph revision{" "}
            <code>{applied.graph_revision}</code>, assigned ids:
          </p>
          <AssignedIds operations={applied.report.operations} />
        </>
      )}
      <p className="note">
        {uncertain.plan_id !== (state.proposal.plan_id ?? null) && (
          <>
            It was sent with plan <code>{uncertain.plan_id ?? "none"}</code>,
            not the draft's current one.{" "}
          </>
        )}
        Inspect the graph and decide; Apply needs a new check either way. A
        stale base on a later check does not prove the draft was applied: any
        writer moves the graph revision.
      </p>
      {verdict !== null && (
        <p className="note" data-testid="verdict">
          {verdictText(verdict)}
        </p>
      )}
      <div className="row">
        <button type="button" disabled={looking} onClick={() => void look()}>
          {looking ? "Looking…" : "Look in the graph"}
        </button>
        {present && (
          <button
            type="button"
            onClick={() => {
              dispatch({ kind: "decide", present: true });
              bumpRefresh();
            }}
          >
            Entries are present: clear the draft
          </button>
        )}
        <button
          type="button"
          onClick={() => dispatch({ kind: "decide", present: false })}
        >
          Verified not applied: keep editing
        </button>
      </div>
    </div>
  );
}

export function AppliedBanner({ applied }: { applied: Applied }) {
  const { dispatch } = useDraft();
  const counts = applied.report === null ? null : reportCounts(applied.report);
  return (
    <div className="banner applied" role="status" data-testid="applied">
      Applied
      {applied.graph_revision !== null && (
        <>
          {" "}
          as graph revision <code>{applied.graph_revision}</code>
        </>
      )}
      . Assigned ids:
      <AssignedIds operations={applied.operations} />
      {counts !== null && (
        <p className="note" data-testid="applied-findings">
          Findings: {countWords(counts.total)}; introduced by this change:{" "}
          {countWords(counts.introduced)}.
        </p>
      )}
      <button
        type="button"
        onClick={() => dispatch({ kind: "dismiss_applied" })}
      >
        Dismiss
      </button>
    </div>
  );
}

export function StaleBanner({
  stale,
}: {
  stale: { expected: string; actual: string };
}) {
  const { state, actions } = useDraft();
  return (
    <div className="banner stale" role="alert" data-testid="stale">
      The plan ID's base is stale: the graph moved from{" "}
      <code>{stale.expected}</code> to <code>{stale.actual}</code>. The draft is
      kept; Replan mints a new plan ID and clears the check.
      <div className="row">
        <button
          type="button"
          disabled={state.pending !== null || state.uncertain !== null}
          onClick={() => void actions.replan()}
        >
          Replan
        </button>
      </div>
    </div>
  );
}

function DecodeLocation({
  decode,
}: {
  decode: Extract<Problem, { kind: "malformed_proposal" }>["decode"];
}) {
  return (
    <>
      {decode.path && <code> at {decode.path}</code>}
      {decode.line !== undefined && (
        <span className="muted">
          {" "}
          (line {decode.line}
          {decode.column !== undefined && `, column ${decode.column}`})
        </span>
      )}
    </>
  );
}

export function ProblemBanner({ problem }: { problem: Problem }) {
  const { state, dispatch, actions } = useDraft();
  const dismiss = (
    <button
      type="button"
      onClick={() => dispatch({ kind: "problem", problem: null })}
    >
      Dismiss
    </button>
  );
  switch (problem.kind) {
    case "busy":
      return (
        <div className="banner busy" role="alert" data-testid="problem">
          Busy: {problem.message}
          <div className="row">
            <button
              type="button"
              disabled={
                problem.action === "apply"
                  ? !applicable(state)
                  : state.pending !== null
              }
              title={
                problem.action === "apply" && !applicable(state)
                  ? "the draft or the evaluation changed since the check"
                  : undefined
              }
              onClick={() =>
                problem.action === "check"
                  ? void actions.check()
                  : void actions.apply()
              }
            >
              Retry {problem.action}
            </button>
            {dismiss}
          </div>
        </div>
      );
    case "retryable":
      return (
        <div className="banner problem" role="alert" data-testid="problem">
          No answer to the check: {problem.message}
          <div className="row">
            <button
              type="button"
              disabled={state.pending !== null}
              onClick={() => void actions.check()}
            >
              Retry check
            </button>
            {dismiss}
          </div>
        </div>
      );
    case "invalid_proposal":
      return (
        <div className="banner problem" role="alert" data-testid="problem">
          Invalid: {problem.message}
          {problem.location && (
            <>
              {" "}
              at <OperationLink name={problem.location.name} />
              {problem.location.field && <code> {problem.location.field}</code>}
            </>
          )}
        </div>
      );
    case "malformed_proposal":
      return (
        <div className="banner problem" role="alert" data-testid="problem">
          Malformed: {problem.decode.message}
          <DecodeLocation decode={problem.decode} />
          {problem.operation !== null && (
            <>
              {" "}
              in <OperationLink name={problem.operation} />
            </>
          )}
        </div>
      );
    case "incomplete":
      return (
        <div className="banner problem" role="alert" data-testid="problem">
          {problem.message}: <OperationLink name={problem.operation} />
        </div>
      );
    case "foreign_database":
      return (
        <div className="banner problem" role="alert" data-testid="problem">
          {problem.message}
          <p className="note">
            The plan ID names database <code>{problem.expected}</code>, but the
            server holds <code>{problem.actual}</code>. Replan mints a plan ID
            for this database; the draft's existing references may name entries
            that are not here.
          </p>
          <div className="row">
            <button
              type="button"
              disabled={state.pending !== null || state.uncertain !== null}
              onClick={() => void actions.replan()}
            >
              Replan
            </button>
            {dismiss}
          </div>
        </div>
      );
    case "failed":
      return (
        <div className="banner problem" role="alert" data-testid="problem">
          {problem.action === "apply"
            ? "Apply failed before publication; nothing was written: "
            : "Check failed: "}
          {problem.message} {dismiss}
        </div>
      );
  }
}

export function Notices() {
  const { state, dispatch } = useDraft();
  if (state.notices.length === 0) {
    return null;
  }
  return (
    <ul className="notices" data-testid="notices">
      {state.notices.map((notice) => (
        <li key={notice.id} className="notice" data-testid="notice">
          {notice.text}{" "}
          {notice.offer !== null && (
            <button
              type="button"
              disabled={state.pending === "apply"}
              onClick={() => {
                if (notice.offer !== null) {
                  dispatch({ kind: "remove", names: notice.offer.remove });
                }
                dispatch({ kind: "dismiss_notice", id: notice.id });
              }}
            >
              {notice.offer.label}
            </button>
          )}{" "}
          <button
            type="button"
            className="link"
            onClick={() => dispatch({ kind: "dismiss_notice", id: notice.id })}
          >
            ok
          </button>
        </li>
      ))}
    </ul>
  );
}
