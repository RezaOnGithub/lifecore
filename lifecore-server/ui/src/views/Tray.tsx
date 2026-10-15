import { useState } from "react";
import type {
  Format,
  OperationEntry,
  ProposalEntry,
  ProposalReference,
} from "../contract.ts";
import { useData } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import {
  type Attachment,
  attachFindings,
  countFindings,
} from "../draft/findings.ts";
import {
  applicable,
  type DraftState,
  type Evaluation,
  isUnset,
  type Problem,
  referenceIssue,
  requestOf,
  validName,
} from "../draft/model.ts";
import { IdLink, OperationLink, useOpen } from "../links.tsx";
import type { Screen } from "../navigation.ts";
import { CheckStatus, FindingList } from "./Findings.tsx";

function Reference({
  reference,
  entries,
  index,
}: {
  reference: ProposalReference;
  entries: readonly ProposalEntry[];
  index: number;
}) {
  if ("existing" in reference) {
    return isUnset(reference) ? (
      <span className="muted" title="not picked yet">
        ?
      </span>
    ) : (
      <IdLink id={reference.existing} />
    );
  }
  const issue = referenceIssue(entries, index, reference);
  return (
    <span
      className={issue === null ? undefined : "ref-issue"}
      title={
        issue === "missing"
          ? "no operation has this name"
          : issue === "later"
            ? "refers to a later operation"
            : undefined
      }
    >
      <OperationLink name={reference.added} />
    </span>
  );
}

function ReferenceChips({
  entry,
  entries,
  index,
}: {
  entry: ProposalEntry;
  entries: readonly ProposalEntry[];
  index: number;
}) {
  if (entry.kind === "node") {
    if (entry.predecessor === undefined) {
      return null;
    }
    return (
      <ul className="chips">
        <li>
          {entry.intent ?? "revises?"}{" "}
          <Reference
            reference={entry.predecessor}
            entries={entries}
            index={index}
          />
        </li>
      </ul>
    );
  }
  return (
    <ul className="chips">
      <li>
        from{" "}
        <Reference reference={entry.from} entries={entries} index={index} />
      </li>
      <li>
        to <Reference reference={entry.to} entries={entries} index={index} />
      </li>
    </ul>
  );
}

function flagged(problem: Problem | null, name: string): boolean {
  switch (problem?.kind) {
    case "invalid_proposal":
      return problem.location?.name === name;
    case "incomplete":
      return problem.operation === name;
    default:
      return false;
  }
}

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

function ProblemBanner({ problem }: { problem: Problem }) {
  const { dispatch, actions } = useDraft();
  const { bumpRefresh } = useData();
  const dismiss = (
    <button
      type="button"
      onClick={() => dispatch({ kind: "problem", problem: null })}
    >
      Dismiss
    </button>
  );
  switch (problem.kind) {
    case "uncertain":
      return (
        <div className="banner uncertain" role="alert" data-testid="uncertain">
          <strong>The change may have been applied.</strong> {problem.message}
          {problem.outcome?.kind === "applied" && (
            <>
              <p className="note">
                Outcome: graph revision{" "}
                <code>{problem.outcome.graph_revision}</code>, assigned ids:
              </p>
              <AssignedIds operations={problem.outcome.report.operations} />
            </>
          )}
          <p className="note">
            Refresh and inspect the stored state. The draft is kept: Clear it if
            the entries are there; a stale base on Check also means it was
            applied.
          </p>
          <div className="row">
            <button type="button" onClick={bumpRefresh}>
              Refresh
            </button>
            {dismiss}
          </div>
        </div>
      );
    case "busy":
      return (
        <div className="banner busy" role="alert">
          Busy: {problem.message}
          <div className="row">
            <button
              type="button"
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
        <div className="banner problem" role="alert">
          No answer to the check: {problem.message}
          <div className="row">
            <button type="button" onClick={() => void actions.check()}>
              Retry check
            </button>
            {dismiss}
          </div>
        </div>
      );
    case "invalid_proposal":
      return (
        <div className="banner problem" role="alert">
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
        <div className="banner problem" role="alert">
          Malformed: {problem.decode.message}
          {problem.decode.path && <code> at {problem.decode.path}</code>}
          {problem.decode.line !== undefined && (
            <span className="muted">
              {" "}
              (line {problem.decode.line}
              {problem.decode.column !== undefined &&
                `, column ${problem.decode.column}`}
              )
            </span>
          )}
        </div>
      );
    case "incomplete":
      return (
        <div className="banner problem" role="alert">
          {problem.message}: <OperationLink name={problem.operation} />
        </div>
      );
    case "foreign_database":
    case "failed":
      return (
        <div className="banner problem" role="alert">
          {problem.message} {dismiss}
        </div>
      );
  }
}

function EvaluationChoice({
  evaluation,
  onChange,
}: {
  evaluation: Evaluation;
  onChange: (evaluation: Evaluation) => void;
}) {
  return (
    <div className="row">
      <label>
        evaluation time{" "}
        <select
          value={evaluation.kind}
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
        <input
          aria-label="evaluation timestamp"
          placeholder="2026-10-15T08:30:00Z"
          size={28}
          value={evaluation.time}
          onChange={(event) =>
            onChange({ kind: "at", time: event.target.value })
          }
        />
      )}
    </div>
  );
}

// The exported text as of the click; a later edit is flagged, not re-encoded.
function ExportPanel({ state }: { state: DraftState }) {
  const { actions } = useDraft();
  const [format, setFormat] = useState<Format>("toml");
  const [exported, setExported] = useState<{
    format: Format;
    text: string;
    request: string;
  } | null>(null);
  const run = async () => {
    const text = await actions.encode(format);
    if (text !== null) {
      setExported({ format, text, request: requestOf(state) });
    }
  };
  return (
    <section className="export">
      <div className="row">
        <span className="toggle">
          {(["toml", "json"] as const).map((option) => (
            <button
              key={option}
              type="button"
              className={format === option ? "on" : undefined}
              onClick={() => setFormat(option)}
            >
              {option.toUpperCase()}
            </button>
          ))}
        </span>
        <button
          type="button"
          disabled={
            state.proposal.entries.length === 0 || state.pending !== null
          }
          onClick={() => void run()}
        >
          Export
        </button>
      </div>
      {exported !== null && (
        <>
          {exported.request !== requestOf(state) && (
            <p className="muted">the draft changed since this export</p>
          )}
          <textarea
            className="export"
            aria-label="exported proposal"
            readOnly
            rows={10}
            value={exported.text}
          />
          <div className="row">
            <button
              type="button"
              onClick={() => void navigator.clipboard.writeText(exported.text)}
            >
              Copy
            </button>
            <a
              download={`draft.${exported.format}`}
              href={`data:text/plain;charset=utf-8,${encodeURIComponent(exported.text)}`}
            >
              Download
            </a>
          </div>
        </>
      )}
    </section>
  );
}

function statusWord(state: DraftState, matching: boolean): string {
  if (state.pending === "check") {
    return "checking…";
  }
  if (state.pending === "apply") {
    return "applying…";
  }
  if (state.stale !== null) {
    return "stale base";
  }
  if (state.outcome === null) {
    return "not checked";
  }
  if (!matching) {
    return "changed since check";
  }
  return state.outcome.accepted ? "accepted" : "rejected";
}

function OperationItem({
  entry,
  index,
  entries,
  attachment,
  selected,
}: {
  entry: ProposalEntry;
  index: number;
  entries: readonly ProposalEntry[];
  attachment: Attachment | null;
  selected: boolean;
}) {
  const { state, dispatch } = useDraft();
  const open = useOpen();
  const findings = attachment?.operations.get(entry.name);
  const classes = [
    "op",
    selected && "selected",
    flagged(state.problem, entry.name) && "invalid",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <li className={classes} data-testid="operation">
      <div className="row">
        <button
          type="button"
          className="link"
          onClick={() => {
            dispatch({ kind: "select", name: entry.name });
            open({ kind: "operation", name: entry.name });
          }}
        >
          <strong>{entry.name}</strong>
        </button>
        <span className="badge kind">{entry.kind}</span>
        <span>{entry.ty_name === "" ? "(no type)" : entry.ty_name}</span>
        {!validName(entry.name) && (
          <span
            className="badge warn"
            title="names match [A-Za-z_][A-Za-z0-9_-]*"
          >
            invalid name
          </span>
        )}
        {findings !== undefined && (
          <CheckStatus counts={countFindings(findings)} />
        )}
      </div>
      <ReferenceChips entry={entry} entries={entries} index={index} />
      {findings !== undefined && findings.length > 0 && (
        <FindingList findings={findings} />
      )}
    </li>
  );
}

export function Tray({ screen }: { screen: Screen }) {
  const { state, dispatch, actions } = useDraft();
  const { asOf, bumpRefresh } = useData();
  const open = useOpen();
  const [collapsed, setCollapsed] = useState(false);
  const { entries } = state.proposal;
  const attachment =
    state.outcome === null ? null : attachFindings(state.outcome.report);
  const matching =
    state.outcome !== null && state.outcome.request === requestOf(state);
  const status = statusWord(state, matching);
  const selected =
    screen.kind === "operation" ? screen.name : (state.selected ?? null);
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
          {status}
        </span>
      </aside>
    );
  }
  return (
    <aside className="tray" aria-label="Draft" data-testid="tray">
      <h2 className="tray-title">
        Draft{" "}
        <span className="counts" data-testid="draft-count">
          {entries.length} operation{entries.length === 1 ? "" : "s"}
        </span>{" "}
        <span className="badge" data-testid="draft-status">
          {status}
        </span>
        <span className="tray-spacer" />
        <button
          type="button"
          onClick={() => setCollapsed(true)}
          aria-label="collapse the draft tray"
          title="collapse the draft tray"
        >
          ▾
        </button>
      </h2>
      {asOf !== null && (
        <p className="banner asof">
          drafts apply to the current state, not to the boundary you are viewing
        </p>
      )}
      {state.applied !== null && (
        <div className="banner applied" role="status" data-testid="applied">
          Applied as graph revision <code>{state.applied.graph_revision}</code>.
          Assigned ids:
          <AssignedIds operations={state.applied.operations} />
          <button
            type="button"
            onClick={() => dispatch({ kind: "dismiss_applied" })}
          >
            Dismiss
          </button>
        </div>
      )}
      {state.stale !== null && (
        <div className="banner stale" role="alert" data-testid="stale">
          The plan ID's base is stale: the graph moved from{" "}
          <code>{state.stale.expected}</code> to{" "}
          <code>{state.stale.actual}</code>. The draft is kept; Replan mints a
          new plan ID and clears the check.
          <div className="row">
            <button
              type="button"
              disabled={state.pending !== null}
              onClick={() => void actions.replan()}
            >
              Replan
            </button>
          </div>
        </div>
      )}
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
      {state.notice !== null && (
        <p className="muted" data-testid="notice">
          {state.notice}{" "}
          <button
            type="button"
            className="link"
            onClick={() => dispatch({ kind: "notice", notice: null })}
          >
            ok
          </button>
        </p>
      )}
      {state.pending === "mint" && <p className="muted">minting a plan ID…</p>}
      {state.proposal.plan_id !== undefined && (
        <p
          className="plan"
          title={state.proposal.plan_id}
          data-testid="plan-id"
        >
          plan <code>{state.proposal.plan_id}</code>
        </p>
      )}
      {entries.length === 0 ? (
        <p className="muted">
          No operations. Use "New node of this type" on a table, or Revise,
          Branch, and Connect in the inspector, or Import.
        </p>
      ) : (
        <ol className="operations" data-testid="operations">
          {entries.map((entry, index) => (
            <OperationItem
              key={entry.name}
              entry={entry}
              index={index}
              entries={entries}
              attachment={attachment}
              selected={entry.name === selected}
            />
          ))}
        </ol>
      )}
      <div className="row">
        <button
          type="button"
          className="link"
          onClick={() => actions.addEdge()}
        >
          + edge operation
        </button>
      </div>
      <EvaluationChoice
        evaluation={state.evaluation}
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
          disabled={!applicable(state)}
          title={
            applicable(state)
              ? undefined
              : "enabled after an accepted check of this exact draft"
          }
          onClick={() => void actions.apply()}
        >
          Apply
        </button>
        <button type="button" onClick={() => open({ kind: "import" })}>
          Import
        </button>
        <button
          type="button"
          disabled={entries.length === 0}
          onClick={() => {
            dispatch({ kind: "clear" });
            if (screen.kind === "operation") {
              open({ kind: "database" });
            }
          }}
        >
          Clear
        </button>
      </div>
      <ExportPanel state={state} />
      {state.outcome !== null && attachment !== null && (
        <section data-testid="report">
          <h3>
            {state.outcome.source === "check" ? "Check" : "Apply"}:{" "}
            {state.outcome.accepted ? "accepted" : "rejected"}
          </h3>
          <p className="muted">
            base [0, {state.outcome.report.base.end_exclusive}), evaluation time{" "}
            {state.outcome.report.analysis.inputs.evaluation_time ?? "none"}
            {!matching && "; the draft changed since"}
          </p>
          {attachment.unplaced.length > 0 && (
            <>
              <h4>for operations no longer in the draft</h4>
              <FindingList findings={attachment.unplaced} />
            </>
          )}
          <h4>already in history</h4>
          {attachment.history.length === 0 ? (
            <p className="muted">none</p>
          ) : (
            <FindingList findings={attachment.history} />
          )}
          <h4>initialization</h4>
          {attachment.initialization.length === 0 ? (
            <p className="muted">none</p>
          ) : (
            <FindingList findings={attachment.initialization} />
          )}
          <button type="button" className="link" onClick={bumpRefresh}>
            refresh the screens
          </button>
        </section>
      )}
    </aside>
  );
}
