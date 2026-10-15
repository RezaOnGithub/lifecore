import { useState } from "react";
import type {
  EdgeRecord,
  EntryId,
  EntryInspection,
  NodeRecord,
  TypeResolution,
  TypeSchema,
  TypeSummary,
  ViewId,
} from "../contract.ts";
import { latest, useAsOf, useCall, useTypes } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { FailureView } from "../failure.tsx";
import {
  CurrentBadge,
  IdLink,
  NodeLink,
  OperationLink,
  Position,
  TypeLink,
  useOpen,
} from "../links.tsx";
import { entryKey, rowLabel, sameId } from "../model/labels.ts";
import { lineageRows } from "../model/lineage.ts";
import { parseDeclaration } from "../model/schema.ts";
import { PayloadTable, RawJson, SchemaView } from "../views/Payload.tsx";
import { ResolutionBadge } from "../views/widgets.tsx";

export function InspectorScreen({
  id,
  onExplore,
}: {
  id: EntryId;
  onExplore: (id: EntryId) => void;
}) {
  const asOf = useAsOf();
  const load = useCall("entries.inspect", { ...asOf, id });
  // The screen is keyed by the entry, so a previous result is this entry
  // under another boundary; it stays until the reload answers.
  const result = latest(load);
  if (load.status === "failed") {
    return <FailureView failure={load.failure} />;
  }
  if (result === null) {
    return <p className="muted">loading…</p>;
  }
  return (
    <>
      {load.status === "loading" && <p className="muted">reloading…</p>}
      {result.kind === "node" ? (
        <NodeInspection inspection={result} onExplore={onExplore} />
      ) : (
        <EdgeInspection inspection={result} />
      )}
    </>
  );
}

function TypeNote({ type }: { type: TypeResolution }) {
  switch (type.kind) {
    case "builtin":
      return <span className="muted">built-in</span>;
    case "declared":
      return (
        <span className="muted">
          declared by <IdLink id={type.declaration} />
        </span>
      );
    case "missing":
    case "ambiguous":
      return <ResolutionBadge resolution={type} />;
    case "malformed":
      return (
        <>
          <ResolutionBadge resolution={type} />{" "}
          <span className="muted">{type.message}</span>
        </>
      );
  }
}

// The schema of a `Type` node's own payload: the server's parse when this node
// is a current declaration, else a client-side reading of the word list.
function ownDeclaration(
  types: TypeSummary[] | null,
  node: NodeRecord,
): TypeSchema | null {
  for (const type of types ?? []) {
    if (
      type.resolution.kind === "declared" &&
      sameId(type.resolution.declaration, node.id)
    ) {
      return type.resolution.schema;
    }
  }
  return parseDeclaration(node.payload);
}

// While an operation waits for a node, every node inspection offers itself;
// an edge inspection says it cannot be picked.
function PickBanner({ id }: { id: EntryId | null }) {
  const { state, actions } = useDraft();
  const open = useOpen();
  const { picking } = state;
  if (picking === null) {
    return null;
  }
  return (
    <div className="banner picking" role="status" data-testid="pick-banner">
      {id === null ? (
        <>
          Edges cannot be picked as the {picking.role} of{" "}
          <OperationLink name={picking.operation} />; open a node.
        </>
      ) : (
        <>
          Use this node as the {picking.role} of{" "}
          <OperationLink name={picking.operation} />?{" "}
          <button
            type="button"
            onClick={() => {
              const operation = actions.resolvePick(id);
              if (operation !== null) {
                open({ kind: "operation", name: operation });
              }
            }}
          >
            Use as {picking.role}
          </button>
        </>
      )}{" "}
      <button type="button" onClick={() => actions.cancelPick()}>
        Cancel
      </button>
    </div>
  );
}

function NodeInspection({
  inspection,
  onExplore,
}: {
  inspection: Extract<EntryInspection, { kind: "node" }>;
  onExplore: (id: EntryId) => void;
}) {
  const { entry, type, lineage, edges, view } = inspection;
  const [raw, setRaw] = useState(false);
  const types = useTypes();
  const { actions } = useDraft();
  const open = useOpen();
  const declaration =
    entry.ty_name === "Type" ? ownDeclaration(types, entry) : null;
  const revise = (intent: "revise" | "branch") => {
    open({ kind: "operation", name: actions.revise(entry, intent) });
  };
  return (
    <article data-testid="inspector">
      <PickBanner id={entry.id} />
      <h2>
        {rowLabel(entry)} <Position id={entry.id} />
      </h2>
      <div className="row">
        <TypeLink name={entry.ty_name} />
        <TypeNote type={type} />
        <CurrentBadge current={entry.current} kind="node" />
        {entry.predecessor && (
          <span className="muted">
            revises <IdLink id={entry.predecessor} />
          </span>
        )}
        <span className="muted">view [0, {view.end_exclusive})</span>
      </div>
      <div className="row">
        <button type="button" onClick={() => onExplore(entry.id)}>
          Explore from here
        </button>
        <button
          type="button"
          title="a new revision with this payload; current edges are kept"
          onClick={() => revise("revise")}
        >
          Revise
        </button>
        <button
          type="button"
          title="a new branch with this payload; no edges are kept"
          onClick={() => revise("branch")}
        >
          Branch
        </button>
        <button
          type="button"
          title="a new edge from this node"
          onClick={() =>
            open({ kind: "operation", name: actions.connect(entry.id) })
          }
        >
          Connect
        </button>
      </div>
      <section>
        <h3>
          Payload{" "}
          <label className="muted">
            <input
              type="checkbox"
              checked={raw}
              onChange={(event) => setRaw(event.target.checked)}
            />{" "}
            raw JSON
          </label>
        </h3>
        {raw ? (
          <RawJson value={entry.payload} />
        ) : (
          <>
            {declaration !== null && (
              <>
                <h4>declares</h4>
                <SchemaView schema={declaration} />
                <h4>payload</h4>
              </>
            )}
            <PayloadTable
              payload={entry.payload}
              schema={type.kind === "declared" ? type.schema : null}
            />
          </>
        )}
      </section>
      <section>
        <h3>
          Lineage{" "}
          <span className="muted">
            {lineage.length} revision{lineage.length === 1 ? "" : "s"}
          </span>
        </h3>
        <ol className="lineage" data-testid="lineage">
          {lineageRows(lineage).map(({ node, depth }) => {
            const inspected = sameId(node.id, entry.id);
            return (
              <li
                key={entryKey(node.id)}
                className={[node.current && "current", inspected && "inspected"]
                  .filter(Boolean)
                  .join(" ")}
                style={{ paddingLeft: `${depth * 1.5 + 0.5}rem` }}
                data-depth={depth}
              >
                {inspected ? (
                  <>
                    <strong>{rowLabel(node)}</strong> <Position id={node.id} />
                  </>
                ) : (
                  <NodeLink node={node} />
                )}{" "}
                <CurrentBadge current={node.current} kind="node" />
                {inspected && <span className="muted"> (this)</span>}
              </li>
            );
          })}
        </ol>
      </section>
      <section>
        <h3>
          Relationships{" "}
          <span className="muted">
            {edges.length} edge{edges.length === 1 ? "" : "s"}
          </span>
        </h3>
        {edges.length === 0 ? (
          <p className="muted">no edges</p>
        ) : (
          <table className="entries" data-testid="relationships">
            <thead>
              <tr>
                <th scope="col">dir</th>
                <th scope="col">edge type</th>
                <th scope="col">other node</th>
                <th scope="col">current</th>
                <th scope="col">edge</th>
              </tr>
            </thead>
            <tbody>
              {edges.map(({ edge, direction, other }) => (
                <tr
                  key={entryKey(edge.id)}
                  className={edge.current ? undefined : "historical"}
                >
                  <td title={direction}>
                    {direction === "outgoing" ? "→" : "←"}
                  </td>
                  <td>{edge.ty_name}</td>
                  <td>
                    <NodeLink node={other} />
                    {!other.current && (
                      <>
                        {" "}
                        <CurrentBadge current={false} kind="node" />
                      </>
                    )}
                  </td>
                  <td>
                    <CurrentBadge current={edge.current} kind="edge" />
                  </td>
                  <td>
                    <IdLink id={edge.id} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </article>
  );
}

function Endpoint({ node }: { node: NodeRecord }) {
  return (
    <>
      <NodeLink node={node} />{" "}
      <CurrentBadge current={node.current} kind="node" />
    </>
  );
}

function EdgeInspection({
  inspection,
}: {
  inspection: Extract<EntryInspection, { kind: "edge" }>;
}) {
  const {
    entry,
    from,
    to,
    view,
  }: {
    entry: EdgeRecord;
    from: NodeRecord;
    to: NodeRecord;
    view: ViewId;
  } = inspection;
  return (
    <article data-testid="inspector">
      <PickBanner id={null} />
      <h2>
        {entry.ty_name} <Position id={entry.id} />
      </h2>
      <div className="row">
        <span className="muted">edge</span>
        <CurrentBadge current={entry.current} kind="edge" />
        <span className="muted">view [0, {view.end_exclusive})</span>
      </div>
      <table className="payload">
        <tbody>
          <tr>
            <th scope="row">type</th>
            <td>{entry.ty_name}</td>
          </tr>
          <tr>
            <th scope="row">from</th>
            <td>
              <Endpoint node={from} />
            </td>
          </tr>
          <tr>
            <th scope="row">to</th>
            <td>
              <Endpoint node={to} />
            </td>
          </tr>
        </tbody>
      </table>
    </article>
  );
}
