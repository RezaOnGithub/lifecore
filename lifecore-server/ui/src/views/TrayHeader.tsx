import { useId, useState } from "react";
import { useData } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import {
  applicable,
  applyingElsewhere,
  type DraftState,
  type Evaluation,
  frozen,
  requestOf,
  sameEvaluation,
} from "../draft/model.ts";
import { OperationLink, useOpen } from "../links.tsx";
import { timestampHint } from "../model/values.ts";
import type { Screen } from "../navigation.ts";
import { ConfirmButton } from "./ConfirmButton.tsx";
import {
  AppliedBanner,
  Notices,
  ProblemBanner,
  StaleBanner,
  UncertainBanner,
} from "./DraftBanners.tsx";
import { ExportPanel } from "./ExportPanel.tsx";

export function statusWord(state: DraftState): string {
  const { outcome } = state;
  if (state.detached) {
    return "waiting for the database";
  }
  if (state.pending === "check") {
    return "checking…";
  }
  if (state.pending === "apply") {
    return "applying…";
  }
  if (applyingElsewhere(state)) {
    return "applying in another tab";
  }
  if (state.uncertain !== null) {
    return "may be applied";
  }
  if (state.stale !== null) {
    return "stale base";
  }
  if (outcome === null) {
    return "not checked";
  }
  if (outcome.request !== requestOf(state)) {
    return "changed since check";
  }
  if (outcome.spent) {
    return "check used";
  }
  return outcome.accepted ? "accepted" : "rejected";
}

// Why Apply is off, or which evaluation inputs it will send.
function applyNote(state: DraftState): string {
  const { outcome } = state;
  if (state.detached) {
    return "edits wait until the database answers";
  }
  if (applyingElsewhere(state)) {
    return "another tab is applying this draft";
  }
  if (state.uncertain !== null) {
    return "waiting for your decision about the last apply";
  }
  if (outcome === null) {
    return "Check first";
  }
  if (outcome.source === "apply") {
    return "the last apply was rejected; Check again";
  }
  if (!outcome.accepted) {
    return "the check rejected the draft";
  }
  if (outcome.spent) {
    return "an apply of uncertain fate used this check; Check again";
  }
  if (outcome.request !== requestOf(state)) {
    return "the draft changed since the check";
  }
  if (!sameEvaluation(outcome.evaluation, state.evaluation)) {
    return "the evaluation choice changed since the check";
  }
  if (state.stale !== null) {
    return "the base is stale: Replan";
  }
  const time = outcome.report.analysis.inputs.evaluation_time;
  if (time === null) {
    return "applies without an evaluation time, as checked";
  }
  return outcome.evaluation.kind === "now"
    ? `applies at ${time}, the instant the check captured`
    : `applies at ${time}, as checked`;
}

const INSTANT = "an RFC 3339 instant such as 2026-10-15T08:30:00Z";

// The form an explicit time is expected in, said as it is typed. The
// recognizer is loose and the server parses the time, so the hint never
// blocks a call.
function evaluationTimeHint(time: string): string | null {
  if (time.trim() === "") {
    return `enter ${INSTANT}`;
  }
  return timestampHint(time) === null
    ? null
    : `expected ${INSTANT}; the server reports what it cannot read`;
}

export function EvaluationChoice({
  evaluation,
  disabled,
  onChange,
}: {
  evaluation: Evaluation;
  disabled: boolean;
  onChange: (evaluation: Evaluation) => void;
}) {
  const hintId = useId();
  const hint =
    evaluation.kind === "at" ? evaluationTimeHint(evaluation.time) : null;
  return (
    <div className="row">
      <label>
        evaluation time{" "}
        <select
          value={evaluation.kind}
          disabled={disabled}
          onChange={(event) => {
            switch (event.target.value) {
              case "now":
                onChange({ kind: "now" });
                break;
              case "at":
                onChange({ kind: "at", time: "" });
                break;
              default:
                onChange({ kind: "none" });
            }
          }}
        >
          <option value="none">none</option>
          <option value="now">now</option>
          <option value="at">explicit…</option>
        </select>
      </label>
      {evaluation.kind === "at" && (
        <>
          <input
            aria-label="evaluation timestamp"
            placeholder="2026-10-15T08:30:00Z"
            size={28}
            disabled={disabled}
            value={evaluation.time}
            aria-invalid={hint !== null && evaluation.time.trim() !== ""}
            aria-describedby={hint === null ? undefined : hintId}
            onChange={(event) =>
              onChange({ kind: "at", time: event.target.value })
            }
          />
          {hint !== null && (
            <span id={hintId} className="edit-hint" data-testid="time-hint">
              {hint}
            </span>
          )}
        </>
      )}
    </div>
  );
}

// The part of the tray that never scrolls away: status, the calls, the
// draft-wide buttons, and every banner.
export function TrayHeader({
  screen,
  onCollapse,
}: {
  screen: Screen;
  onCollapse: () => void;
}) {
  const { state, dispatch, actions } = useDraft();
  const { asOf } = useData();
  const open = useOpen();
  const [exporting, setExporting] = useState(false);
  const { entries } = state.proposal;
  const locked = frozen(state);
  const ready = applicable(state);
  return (
    <div className="tray-head">
      <h2 className="tray-title">
        Draft{" "}
        <span className="counts" data-testid="draft-count">
          {entries.length} operation{entries.length === 1 ? "" : "s"}
        </span>{" "}
        <span className="badge" data-testid="draft-status">
          {statusWord(state)}
        </span>
        <span className="tray-spacer" />
        <button
          type="button"
          onClick={onCollapse}
          aria-label="collapse the draft tray"
          title="collapse the draft tray"
        >
          ▾
        </button>
      </h2>
      {state.uncertain !== null && (
        <UncertainBanner uncertain={state.uncertain} />
      )}
      {applyingElsewhere(state) && (
        <p
          className="banner busy"
          role="status"
          data-testid="applying-elsewhere"
        >
          Another tab is applying this draft: nothing edits it here until that
          tab records the answer. If that tab was closed first, reload this
          page; the draft then comes back as possibly applied.
        </p>
      )}
      {state.proposal.plan_id !== undefined && (
        <p
          className="plan"
          title={state.proposal.plan_id}
          data-testid="plan-id"
        >
          plan <code>{state.proposal.plan_id}</code>
        </p>
      )}
      <EvaluationChoice
        evaluation={state.evaluation}
        disabled={locked}
        onChange={(evaluation) => dispatch({ kind: "evaluation", evaluation })}
      />
      <div className="row">
        <button
          type="button"
          disabled={
            entries.length === 0 ||
            state.pending !== null ||
            (state.evaluation.kind === "at" &&
              state.evaluation.time.trim() === "")
          }
          onClick={() => void actions.check()}
        >
          Check
        </button>
        <button
          type="button"
          disabled={!ready}
          onClick={() => void actions.apply()}
        >
          Apply
        </button>
        <span className="muted apply-note" data-testid="apply-note">
          {state.pending === "apply" ? "applying…" : applyNote(state)}
        </span>
      </div>
      <div className="row">
        <button
          type="button"
          disabled={locked}
          onClick={() => open({ kind: "import" })}
        >
          Import
        </button>
        <button
          type="button"
          aria-expanded={exporting}
          onClick={() => setExporting(!exporting)}
        >
          Export
        </button>
        <button
          type="button"
          disabled={locked}
          onClick={() => {
            const name = actions.addEdge();
            if (name !== null) {
              open({ kind: "operation", name });
            }
          }}
        >
          New edge
        </button>
        <ConfirmButton
          confirm="Clear the whole draft?"
          disabled={
            locked || (entries.length === 0 && state.uncertain === null)
          }
          title={
            state.uncertain === null
              ? undefined
              : "also forgets that the last apply may have been published"
          }
          onConfirm={() => {
            dispatch({ kind: "clear" });
            if (screen.kind === "operation") {
              open({ kind: "database" });
            }
          }}
        >
          Clear
        </ConfirmButton>
      </div>
      {exporting && <ExportPanel />}
      {asOf !== null && (
        <p className="banner asof">
          drafts apply to the current state, not to the boundary you are viewing
        </p>
      )}
      {state.applied !== null && <AppliedBanner applied={state.applied} />}
      {state.stale !== null && <StaleBanner stale={state.stale} />}
      {state.problem !== null && <ProblemBanner problem={state.problem} />}
      {state.picking !== null && (
        <div className="banner picking" role="status" data-testid="picking">
          Picking the {state.picking.role} of{" "}
          <OperationLink name={state.picking.operation} />: open a node in the
          inspector and use it.{" "}
          <button type="button" onClick={() => actions.cancelPick()}>
            Cancel
          </button>
        </div>
      )}
      <Notices />
      {state.pending === "mint" && <p className="muted">minting a plan ID…</p>}
    </div>
  );
}
