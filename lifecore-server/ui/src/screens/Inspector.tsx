import { useCallback, useEffect, useMemo } from "react";
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
import {
  latest,
  useAsOf,
  useCall,
  useData,
  useLabels,
  usePlaces,
  useTypes,
} from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { frozen } from "../draft/model.ts";
import { FailureView } from "../failure.tsx";
import {
  CurrentBadge,
  IdLink,
  NodeLink,
  OperationLink,
  Position,
  TypeLink,
  useEdit,
  useOpen,
} from "../links.tsx";
import {
  entryKey,
  rowLabel,
  sameId,
  typeName,
  viewWords,
} from "../model/labels.ts";
import { lineageRows } from "../model/lineage.ts";
import type { OutlineView } from "../model/outline.ts";
import { isPinned, type Place } from "../model/places.ts";
import { edgeWords } from "../model/relations.ts";
import { parseDeclaration } from "../model/schema.ts";
import { defaultInspectorView, type InspectorView } from "../navigation.ts";
import { EntryFindings } from "../views/EntryFindings.tsx";
import { PayloadTable, RawJson, SchemaView } from "../views/Payload.tsx";
import { RelationshipOutline } from "../views/Relationships.tsx";
import { ResolutionBadge } from "../views/widgets.tsx";

// What the sidebar keeps of an inspected entry: a node by its row label, an
// edge read between its endpoints' labels, `Kitchen —Within→ Wash dishes`.
function placeOf(inspection: EntryInspection): Place {
  const { entry } = inspection;
  return {
    id: entry.id,
    kind: entry.kind,
    label:
      inspection.kind === "node"
        ? rowLabel(inspection.entry)
        : edgeWords(
            rowLabel(inspection.from),
            inspection.entry.ty_name,
            rowLabel(inspection.to),
          ),
    ty_name: entry.ty_name,
  };
}

// ☆ pins the entry to the sidebar, ★ unpins it.
function PinButton({ inspection }: { inspection: EntryInspection }) {
  const { places, togglePin } = usePlaces();
  const { entry } = inspection;
  const pinned = isPinned(places, entry.id);
  return (
    <button
      type="button"
      className="pin"
      aria-pressed={pinned}
      aria-label={pinned ? "unpin" : "pin"}
      title={pinned ? "unpin from the sidebar" : "pin to the sidebar"}
      onClick={() => togglePin(placeOf(inspection))}
    >
      {pinned ? "★" : "☆"}
    </button>
  );
}

// The view the inspection was read at, and "As of here", which sets the
// global boundary just after the entry, so that it is the last one in view.
function ViewNote({ id, view }: { id: EntryId; view: ViewId }) {
  const { setAsOf } = useData();
  const boundary = String(BigInt(id.position) + 1n);
  const last = view.end_exclusive === boundary;
  return (
    <>
      <span
        className="muted"
        title={`read as ${viewWords(view.end_exclusive)}`}
      >
        view [0, {view.end_exclusive})
      </span>
      <button
        type="button"
        disabled={last}
        title={
          last
            ? "this entry is already the last one in view"
            : `set as-of to ${boundary}, showing ${viewWords(boundary)}`
        }
        onClick={() => setAsOf(boundary)}
      >
        As of here
      </button>
    </>
  );
}

export function InspectorScreen({
  id,
  view,
  onExplore,
}: {
  id: EntryId;
  view: InspectorView;
  onExplore: (id: EntryId) => void;
}) {
  const asOf = useAsOf();
  const { visit } = usePlaces();
  const book = useLabels();
  const edit = useEdit();
  const setView = useCallback(
    (update: (view: InspectorView) => InspectorView) =>
      edit((screen) =>
        screen.kind === "inspector"
          ? { ...screen, view: update(screen.view ?? defaultInspectorView) }
          : screen,
      ),
    [edit],
  );
  const load = useCall("entries.inspect", { ...asOf, id });
  // The screen is keyed by the entry, so a previous result is this entry
  // under a narrower boundary or before a refresh; it stays until the reload
  // answers.
  const result = latest(load);
  // Each answer records the visit, refreshing the label kept for it.
  const ready = load.status === "ready" ? load.result : null;
  const place = useMemo(
    () => (ready === null ? null : placeOf(ready)),
    [ready],
  );
  useEffect(() => {
    if (place !== null) {
      visit(place);
      book.note(place.id, place.label);
    }
  }, [place, visit, book]);
  useEffect(() => {
    if (result?.kind === "node") {
      book.noteNodes(result.edges.map(({ other }) => other));
    }
  }, [result, book]);
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
        <NodeInspection
          inspection={result}
          view={view}
          setView={setView}
          onExplore={onExplore}
        />
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
            disabled={frozen(state)}
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
  view: { raw, outline },
  setView,
  onExplore,
}: {
  inspection: Extract<EntryInspection, { kind: "node" }>;
  view: InspectorView;
  setView: (update: (view: InspectorView) => InspectorView) => void;
  onExplore: (id: EntryId) => void;
}) {
  const { entry, type, lineage, edges, view } = inspection;
  const historical = edges.filter(({ edge }) => !edge.current).length;
  const setOutline = useCallback(
    (update: (outline: OutlineView) => OutlineView) =>
      setView((current) => ({ ...current, outline: update(current.outline) })),
    [setView],
  );
  const types = useTypes();
  const { state, actions } = useDraft();
  const { asOf } = useData();
  const open = useOpen();
  const locked = frozen(state);
  // A revise needs its predecessor current (FILES §2.4); a superseded node
  // can only be branched. Under an as-of boundary `current` describes that
  // view, not the current state drafts apply to, so Revise waits until the
  // boundary is cleared. Branch stays: it copies this revision's own
  // payload, which no boundary changes.
  const bounded = asOf !== null;
  const revisable = entry.current && !bounded;
  const declaration =
    entry.ty_name === "Type" ? ownDeclaration(types, entry) : null;
  const revise = (intent: "revise" | "branch") => {
    const name = actions.revise(entry, intent);
    if (name !== null) {
      open({ kind: "operation", name });
    }
  };
  return (
    <article data-testid="inspector">
      <PickBanner id={entry.id} />
      <h2>
        <PinButton inspection={inspection} /> {rowLabel(entry)}{" "}
        <Position id={entry.id} />
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
        <ViewNote id={entry.id} view={view} />
      </div>
      <div className="row">
        <button type="button" onClick={() => onExplore(entry.id)}>
          Explore from here
        </button>
        <button
          type="button"
          disabled={locked || !revisable}
          title={
            bounded
              ? "drafts apply to the current state; clear the as-of boundary (current) to revise"
              : revisable
                ? "a new revision with this payload; current edges are kept"
                : "only a current node can be revised; Branch instead"
          }
          onClick={() => revise("revise")}
        >
          Revise
        </button>
        <button
          type="button"
          disabled={locked}
          title={
            bounded
              ? "a new branch with this revision's own payload, which is the same at every boundary; no edges are kept, and the draft applies to the current state"
              : "a new branch with this payload; no edges are kept"
          }
          onClick={() => revise("branch")}
        >
          Branch
        </button>
        <button
          type="button"
          disabled={locked}
          title="a new edge from this node"
          onClick={() => {
            const name = actions.connect(entry.id);
            if (name !== null) {
              open({ kind: "operation", name });
            }
          }}
        >
          Connect
        </button>
      </div>
      {/* Relationships first, where a wide window puts them beside the
          payload and lineage; the findings about the entry stay just above
          them. */}
      <div className="inspector-body">
        <div className="inspector-relations">
          <EntryFindings id={entry.id} />
          <section>
            <h3>
              Relationships{" "}
              <span className="muted">
                {edges.length} edge{edges.length === 1 ? "" : "s"}
                {historical > 0 && `, ${historical} historical`}
              </span>
            </h3>
            <RelationshipOutline
              owner={entry}
              edges={edges}
              view={outline}
              setView={setOutline}
              asOf={view.end_exclusive}
            />
          </section>
        </div>
        <div className="inspector-data">
          <section>
            <h3>
              Payload{" "}
              <label className="muted">
                <input
                  type="checkbox"
                  checked={raw}
                  onChange={(event) => {
                    const raw = event.target.checked;
                    setView((current) => ({ ...current, raw }));
                  }}
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
                    className={[
                      node.current && "current",
                      inspected && "inspected",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    style={{ paddingLeft: `${depth * 1.5 + 0.5}rem` }}
                    data-depth={depth}
                  >
                    {inspected ? (
                      <>
                        <strong>{rowLabel(node)}</strong>{" "}
                        <Position id={node.id} />
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
        </div>
      </div>
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
        <PinButton inspection={inspection} /> {typeName(entry.ty_name)}{" "}
        <Position id={entry.id} />
      </h2>
      <div className="row">
        <span className="muted">edge</span>
        <CurrentBadge current={entry.current} kind="edge" />
        <ViewNote id={entry.id} view={view} />
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
      <EntryFindings id={entry.id} />
    </article>
  );
}
