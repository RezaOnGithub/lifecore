import { useState } from "react";
import type { EntryId, IncidentEdge, ProposalEntry } from "../contract.ts";
import { latest, useCall } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { keptBy, reconnectable } from "../draft/reconnect.ts";
import { FailureView } from "../failure.tsx";
import { IdLink, NodeLink } from "../links.tsx";
import { entryKey } from "../model/labels.ts";
import { type Incident, incidents } from "../model/relations.ts";
import { ConfirmButton } from "./ConfirmButton.tsx";

type NodeOperation = Extract<ProposalEntry, { kind: "node" }>;

interface Item {
  incident: Incident;
  kept: ProposalEntry | null;
}

// Edges by direction and type, in the order each group first appears.
function grouped(items: readonly Item[]): [string, Item[]][] {
  const groups = new Map<string, Item[]>();
  for (const item of items) {
    const { direction, edge } = item.incident;
    const key = `${direction === "outgoing" ? "→" : "←"} ${edge.ty_name}`;
    const group = groups.get(key);
    if (group === undefined) {
      groups.set(key, [item]);
    } else {
      group.push(item);
    }
  }
  return [...groups];
}

// The current relationships of the predecessor, read without `as_of`: the
// draft applies to the current state. Each checkbox is the presence of the
// kept-edge operation in the draft, so it survives renames and reloads. A
// long list starts collapsed.
export function Reconnection({
  operation,
  predecessor,
  disabled,
}: {
  operation: NodeOperation;
  predecessor: EntryId;
  disabled: boolean;
}) {
  const { state, dispatch } = useDraft();
  const load = useCall("entries.inspect", { id: predecessor });
  const result = latest(load);
  const [expanded, setExpanded] = useState<boolean | null>(null);
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
  const items: Item[] = reconnectable(incidents(result)).map((incident) => ({
    incident,
    kept: keptBy(entries, operation.name, incident),
  }));
  const kept = items.filter((item) => item.kept !== null);
  const open = expanded ?? items.length <= 10;
  const keep = (edges: IncidentEdge[]) =>
    dispatch({
      kind: "keep",
      operation: operation.name,
      predecessor,
      edges,
    });
  return (
    <section className="reconnect" data-testid="reconnect">
      <h3>
        <button
          type="button"
          className="link"
          aria-expanded={open}
          onClick={() => setExpanded(!open)}
        >
          {open ? "▾" : "▸"} Reconnection
        </button>{" "}
        <span className="counts" data-testid="reconnect-count">
          {kept.length} of {items.length} current edge
          {items.length === 1 ? "" : "s"} kept
        </span>
      </h3>
      <p className="muted">
        {operation.intent === "branch" ? "Branching" : "Revising"}{" "}
        <IdLink id={predecessor} /> makes its edges historical; reconnection is
        explicit (SPEC §5). A checked edge becomes a new edge operation of the
        same type with this operation at the {operation.intent ?? "revise"} end;
        an other end that an earlier operation revises is kept on that
        successor.
      </p>
      <fieldset className="frame" disabled={disabled}>
        <div className="row">
          <button
            type="button"
            disabled={kept.length === items.length}
            onClick={() => keep(items.map((item) => item.incident))}
          >
            Keep all
          </button>
          <ConfirmButton
            confirm={`Remove ${kept.length === 1 ? "the kept edge" : `all ${kept.length} kept edges`}?`}
            disabled={kept.length === 0}
            onConfirm={() =>
              dispatch({
                kind: "remove",
                names: kept.flatMap((item) =>
                  item.kept === null ? [] : [item.kept.name],
                ),
              })
            }
          >
            Keep none
          </ConfirmButton>
        </div>
        {open &&
          (items.length === 0 ? (
            <p className="muted">no current edges</p>
          ) : (
            grouped(items).map(([label, group]) => (
              <div key={label} className="reconnect-group">
                <h4 className="reconnect-head">
                  {label}{" "}
                  <span className="counts">
                    {group.filter((item) => item.kept !== null).length}/
                    {group.length}
                  </span>
                </h4>
                <ul className="reconnect-list">
                  {group.map(({ incident, kept: by }) => (
                    <li key={entryKey(incident.edge.id)}>
                      <label>
                        <input
                          type="checkbox"
                          checked={by !== null}
                          onChange={(event) => {
                            if (event.target.checked) {
                              keep([incident]);
                            } else if (by !== null) {
                              dispatch({ kind: "remove", names: [by.name] });
                            }
                          }}
                        />{" "}
                        {incident.direction === "outgoing" ? "→" : "←"}{" "}
                        {incident.edge.ty_name}{" "}
                        <NodeLink node={incident.node} />{" "}
                        <IdLink id={incident.edge.id} />
                        {by !== null && (
                          <span className="muted"> kept as {by.name}</span>
                        )}
                      </label>
                    </li>
                  ))}
                </ul>
              </div>
            ))
          ))}
      </fieldset>
    </section>
  );
}
