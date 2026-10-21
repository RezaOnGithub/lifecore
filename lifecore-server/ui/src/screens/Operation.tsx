import { useEffect, useState } from "react";
import type { ProposalEntry } from "../contract.ts";
import { usePlaces, useTypes } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { frozen, isUnset, type Problem } from "../draft/model.ts";
import { derivedKept } from "../draft/operations.ts";
import { OperationEditor, type PickRole } from "../editors/OperationEditor.tsx";
import { useOpen, useReplace } from "../links.tsx";
import { takenName } from "../model/aliases.ts";
import { ConfirmButton } from "../views/ConfirmButton.tsx";
import { FindingList, judged, OperationStatus } from "../views/Findings.tsx";
import { operationWarnings } from "../views/OperationRow.tsx";
import { Reconnection } from "../views/Reconnection.tsx";

// What a content problem from the server says about this operation.
function problemFor(problem: Problem | null, name: string): string | null {
  switch (problem?.kind) {
    case "invalid_proposal":
      return problem.location?.name === name ? problem.message : null;
    case "malformed_proposal":
      return problem.operation === name ? problem.decode.message : null;
    case "incomplete":
      return problem.operation === name ? problem.message : null;
    default:
      return null;
  }
}

// The operation's header and findings, then its editor, then for a revision
// of an existing node the reconnection assistant. Nothing here edits while
// an apply is in flight. Remove leaves through `onBack`, to the step before.
export function OperationScreen({
  name,
  onBack,
}: {
  name: string;
  onBack: () => void;
}) {
  const { state, dispatch, actions } = useDraft();
  const types = useTypes();
  const { places } = usePlaces();
  const open = useOpen();
  const replace = useReplace();
  // The editor keeps its local state, and the name input its focus, across
  // a rename of its own operation, which moves the session along in the
  // same handler; any other change of name is another operation and
  // remounts it.
  const [session, setSession] = useState({ name, key: 0 });
  if (session.name !== name) {
    setSession({ name, key: session.key + 1 });
  }
  const { entries } = state.proposal;
  const index = entries.findIndex((entry) => entry.name === name);
  const entry = entries[index];
  const locked = frozen(state);
  // Opening the screen from a chip selects the operation in the tray too.
  useEffect(() => {
    if (entry !== undefined && state.selected !== name) {
      dispatch({ kind: "select", name });
    }
  }, [entry, name, state.selected, dispatch]);
  if (entry === undefined) {
    return (
      <section data-testid="operation-screen">
        <h2>{name}</h2>
        <p className="muted">
          No operation named {name} is in the draft: it was removed, renamed, or
          applied.
        </p>
      </section>
    );
  }
  const judgment = judged(state);
  const findings = judgment?.findings(name) ?? [];
  const invalid = problemFor(state.problem, name);
  const warnings = operationWarnings(entries, index, state.kept);
  // A kept edge that still keeps its edge names nothing when applied.
  const taken =
    derivedKept(entries, state.kept, name) === null
      ? takenName(places.aliases, entry)
      : null;
  const onChange = (changed: ProposalEntry) => {
    if (locked) {
      return;
    }
    dispatch({ kind: "replace", name, entry: changed });
    // The screen is keyed by name: follow a rename the reducer accepts
    // without leaving a Back step.
    if (
      changed.name !== name &&
      !entries.some((other) => other.name === changed.name)
    ) {
      setSession({ name: changed.name, key: session.key });
      replace({ kind: "operation", name: changed.name });
    }
  };
  const onPick = (role: PickRole) => {
    actions.startPick(name, role);
    // A predecessor is of the operation's own type, so its table is the
    // place to pick from; an edge end can be any node.
    if (role === "predecessor" && entry.ty_name !== "") {
      open({ kind: "table", ty_name: entry.ty_name });
    }
  };
  const move = (by: -1 | 1) => dispatch({ kind: "move", name, by });
  // The assistant applies to a revision of an existing node; a predecessor
  // still to be picked, or an added one, has no current edges to read.
  const revision =
    entry.kind === "node" &&
    entry.predecessor !== undefined &&
    "existing" in entry.predecessor &&
    !isUnset(entry.predecessor)
      ? { operation: entry, predecessor: entry.predecessor.existing }
      : null;
  return (
    <article data-testid="operation-screen">
      <h2>
        {entry.name} <span className="badge kind">{entry.kind}</span>{" "}
        <span className="muted">
          operation {index + 1} of {entries.length}
        </span>{" "}
        <OperationStatus name={name} />
      </h2>
      {locked && (
        <p className="banner busy">
          Applying: the draft is frozen until the answer.
        </p>
      )}
      {invalid !== null && (
        <p className="banner problem" role="alert">
          {invalid}
        </p>
      )}
      {warnings.map((warning) => (
        <p key={warning} className="op-warn" data-testid="op-warning">
          {warning}
        </p>
      ))}
      {taken !== null && (
        <p className="op-warn" data-testid="name-warning">
          {taken.name} names @{taken.id.position} in this browser; applying this
          operation gives the name to its entry instead.
        </p>
      )}
      {judgment !== null && findings.length > 0 && (
        <section>
          <h3>Findings</h3>
          <FindingList
            findings={findings}
            names={judgment.names}
            current={judgment.current}
          />
        </section>
      )}
      <fieldset className="frame" disabled={locked}>
        <OperationEditor
          key={session.key}
          entry={entry}
          operations={entries}
          index={index}
          types={types}
          onChange={onChange}
          onPick={onPick}
        />
        <div className="row">
          <button type="button" disabled={index === 0} onClick={() => move(-1)}>
            Move up
          </button>
          <button
            type="button"
            disabled={index === entries.length - 1}
            onClick={() => move(1)}
          >
            Move down
          </button>
          <button
            type="button"
            onClick={() => dispatch({ kind: "duplicate", name })}
          >
            Duplicate
          </button>
          <ConfirmButton
            confirm={`Remove ${name}?`}
            onConfirm={() => {
              dispatch({ kind: "remove", names: [name] });
              onBack();
            }}
          >
            Remove
          </ConfirmButton>
        </div>
      </fieldset>
      {revision !== null && (
        <Reconnection
          operation={revision.operation}
          predecessor={revision.predecessor}
          disabled={locked}
        />
      )}
    </article>
  );
}
