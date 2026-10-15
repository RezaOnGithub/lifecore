import { useEffect, useRef, useState } from "react";
import type { EntryId, ProposalEntry } from "../contract.ts";
import { latest, useCall, useTypes } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { attachFindings } from "../draft/findings.ts";
import { isUnset } from "../draft/model.ts";
import { keptBy, reconnectable, reconnection } from "../draft/reconnect.ts";
import { OperationEditor, type PickRole } from "../editors/OperationEditor.tsx";
import { FailureView } from "../failure.tsx";
import { IdLink, NodeLink, useOpen, useReplace } from "../links.tsx";
import { entryKey } from "../model/labels.ts";
import { FindingList } from "../views/Findings.tsx";

// The current relationships of the predecessor, read without `as_of`: the
// draft applies to the current state. Each checkbox is the presence of the
// kept-edge operation in the draft, so it survives renames and reloads.
type NodeOperation = Extract<ProposalEntry, { kind: "node" }>;

function Reconnection({
  operation,
  predecessor,
}: {
  operation: NodeOperation;
  predecessor: EntryId;
}) {
  const { state, dispatch } = useDraft();
  const load = useCall("entries.inspect", { id: predecessor });
  const result = latest(load);
  if (load.status === "failed") {
    return <FailureView failure={load.failure} compact />;
  }
  if (result === null) {
    return <p className="muted">reading the current edges…</p>;
  }
  if (result.kind !== "node") {
    return null;
  }
  const { entries } = state.proposal;
  const edges = reconnectable(result.edges);
  return (
    <section className="reconnect" data-testid="reconnect">
      <h3>Reconnection</h3>
      <p className="muted">
        Revising <IdLink id={predecessor} /> makes its edges historical;
        reconnection is explicit (SPEC §5). A checked edge becomes a new edge
        operation of the same type with this operation at{" "}
        {operation.intent === "branch" ? "the branch's" : "the revision's"} end.
      </p>
      {edges.length === 0 ? (
        <p className="muted">no current edges</p>
      ) : (
        <ul className="reconnect-list">
          {edges.map((incident) => {
            const kept = keptBy(entries, operation.name, incident);
            return (
              <li key={entryKey(incident.edge.id)}>
                <label>
                  <input
                    type="checkbox"
                    checked={kept !== null}
                    onChange={(event) => {
                      if (event.target.checked) {
                        if (kept === null) {
                          dispatch({
                            kind: "add",
                            entry: reconnection(operation.name, incident),
                            after: operation.name,
                            select: false,
                          });
                        }
                      } else if (kept !== null) {
                        dispatch({ kind: "remove", name: kept.name });
                      }
                    }}
                  />{" "}
                  {incident.direction === "outgoing" ? "→" : "←"}{" "}
                  {incident.edge.ty_name} <NodeLink node={incident.other} />{" "}
                  <IdLink id={incident.edge.id} />
                  {kept !== null && (
                    <span className="muted"> kept as {kept.name}</span>
                  )}
                </label>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

export function OperationScreen({ name }: { name: string }) {
  const { state, dispatch, actions } = useDraft();
  const types = useTypes();
  const open = useOpen();
  const replace = useReplace();
  // The editor keeps its local state, and the name input its focus, across
  // a rename of its own operation, but starts afresh for another operation.
  const renamedTo = useRef<string | null>(null);
  const [session, setSession] = useState({ name, key: 0 });
  if (session.name !== name) {
    setSession({
      name,
      key: renamedTo.current === name ? session.key : session.key + 1,
    });
  }
  const { entries } = state.proposal;
  const index = entries.findIndex((entry) => entry.name === name);
  const entry = entries[index];
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
  const findings =
    state.outcome === null
      ? []
      : (attachFindings(state.outcome.report).operations.get(name) ?? []);
  const { problem } = state;
  const invalid =
    problem?.kind === "invalid_proposal" && problem.location?.name === name
      ? problem.message
      : problem?.kind === "incomplete" && problem.operation === name
        ? problem.message
        : null;
  const onChange = (changed: ProposalEntry) => {
    dispatch({ kind: "replace", name, entry: changed });
    // The screen is keyed by name: follow a rename the reducer accepts, one
    // per keystroke, without leaving a Back step for each.
    if (
      changed.name !== name &&
      !entries.some((other) => other.name === changed.name)
    ) {
      renamedTo.current = changed.name;
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
        </span>
      </h2>
      {invalid !== null && (
        <p className="banner problem" role="alert">
          {invalid}
        </p>
      )}
      {findings.length > 0 && (
        <section>
          <h3>Findings</h3>
          <FindingList findings={findings} />
        </section>
      )}
      {revision !== null && (
        <Reconnection
          operation={revision.operation}
          predecessor={revision.predecessor}
        />
      )}
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
        <button
          type="button"
          onClick={() => {
            dispatch({ kind: "remove", name });
            open({ kind: "database" });
          }}
        >
          Remove
        </button>
      </div>
    </article>
  );
}
