import { createContext, type ReactNode, useContext, useMemo } from "react";
import type {
  Currentness,
  DecimalString,
  EdgeRecord,
  EntryId,
  EntryInspection,
  NodeRecord,
} from "../contract.ts";
import { latest, useCall } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import {
  CurrentBadge,
  IdLink,
  NodeLink,
  useExplore,
  useOpen,
} from "../links.tsx";
import { entryKey, rowLabel, typeName } from "../model/labels.ts";
import {
  COLLAPSE_ABOVE,
  groupId,
  groupState,
  type OutlineView,
  onPath,
  rowPath,
  setExpanded,
  setFilter,
  setGroupOpen,
  setHistorical,
} from "../model/outline.ts";
import { truncate } from "../model/payload.ts";
import { neighboursForm } from "../model/query.ts";
import {
  countByType,
  countWords,
  groupByRelation,
  type Incident,
  incidents,
  type Neighbour,
  type Relation,
  type RelationGroup,
  relationCells,
  relationKey,
  relationLabel,
  typeTags,
} from "../model/relations.ts";
import type { ExtraColumn } from "./EntryTable.tsx";

// Neighbour labels in a cell are cut shorter than elsewhere, so a row with
// several relationship columns still fits beside its label.
export const NEIGHBOUR_LIMIT = 30;
// A cell lists this many neighbours before "+N more".
const CELL_FIRST = 3;

// A neighbour's label linking to its inspector, compact enough for a table
// cell: the full label, position, type, and currentness are in the
// tooltip, and a superseded node or a historical edge is styled.
function NeighbourLink({ node, edge }: { node: NodeRecord; edge: EdgeRecord }) {
  const open = useOpen();
  const label = rowLabel(node);
  const notes = [
    `@${node.id.position} ${typeName(node.ty_name)}`,
    node.current ? null : "superseded",
    `edge @${edge.id.position}${edge.current ? "" : " historical"}`,
  ].filter(Boolean);
  return (
    <button
      type="button"
      className={[
        "link",
        "neighbour",
        node.current ? null : "superseded",
        edge.current ? null : "historical",
      ]
        .filter(Boolean)
        .join(" ")}
      title={`${label}\n${notes.join(", ")}`}
      onClick={() => open({ kind: "inspector", id: node.id })}
    >
      {truncate(label, NEIGHBOUR_LIMIT)}
    </button>
  );
}

// The neighbours in one relationship cell. Beyond three, the count by type
// comes first and the rest wait: up to COLLAPSE_ABOVE expand in place,
// more open in Explore as the row's neighbours over this relation.
export function NeighbourList({
  owner,
  relation,
  currentness,
  neighbours,
  open,
  setOpen,
}: {
  owner: EntryId;
  relation: Relation;
  currentness: Currentness;
  neighbours: readonly Neighbour<NodeRecord, EdgeRecord>[];
  open: boolean;
  setOpen: (open: boolean) => void;
}) {
  const explore = useExplore();
  const hub = neighbours.length > COLLAPSE_ABOVE;
  const all = open && !hub;
  const shown = all ? neighbours : neighbours.slice(0, CELL_FIRST);
  const more = neighbours.length - shown.length;
  return (
    <span className={all ? "neighbours all" : "neighbours"}>
      {neighbours.length > CELL_FIRST && (
        <span className="cell-count" data-testid="cell-count">
          {countWords(countByType(neighbours.map(({ node }) => node)))}:{" "}
        </span>
      )}
      {/* The comma stays with its label, so a wrapped list never starts a
          line with one. */}
      {shown.map(({ node, edge }, index) => (
        <span key={entryKey(edge.id)}>
          <span className="neighbour-item">
            <NeighbourLink node={node} edge={edge} />
            {index < shown.length - 1 && ","}
          </span>
          {index < shown.length - 1 && " "}
        </span>
      ))}
      {more > 0 && !hub && (
        <>
          {" "}
          <button
            type="button"
            className="link more"
            onClick={() => setOpen(true)}
          >
            +{more} more
          </button>
        </>
      )}
      {more > 0 && hub && (
        <>
          {" "}
          <button
            type="button"
            className="link more"
            title={`open the ${neighbours.length} neighbours over ${relationLabel(
              relation,
            )} in Explore`}
            onClick={() =>
              explore(
                neighboursForm(
                  owner,
                  relation.ty_name,
                  relation.direction,
                  currentness,
                ),
              )
            }
          >
            +{more} in Explore
          </button>
        </>
      )}
      {all && neighbours.length > CELL_FIRST && (
        <>
          {" "}
          <button
            type="button"
            className="link more"
            onClick={() => setOpen(false)}
          >
            less
          </button>
        </>
      )}
    </span>
  );
}

// A column per relation the rows have over `edges`, headed `→ PartOf` or
// `← DependsOn`, listing each row's neighbours; sorted by the first one.
// `currentness` is that of the edges, for a hub's query in Explore.
export function relationColumns<R extends NodeRecord>(
  rows: readonly R[],
  edges: readonly EdgeRecord[],
  nodes: ReadonlyMap<string, NodeRecord>,
  currentness: Currentness,
): ExtraColumn<R>[] {
  const { relations, cells } = relationCells(rows, edges, nodes);
  return relations.map((relation) => {
    const key = relationKey(relation);
    const neighbours = (row: R) => cells.get(entryKey(row.id))?.get(key) ?? [];
    const name = typeName(relation.ty_name);
    return {
      key: `relation:${key}`,
      header: (
        <span
          title={
            relation.direction === "outgoing"
              ? `${name} edges from the row; the cell lists their "to" nodes`
              : `${name} edges to the row; the cell lists their "from" nodes`
          }
        >
          {relationLabel(relation)}
        </span>
      ),
      cell: (row, open, setOpen) => (
        <NeighbourList
          owner={row.id}
          relation={relation}
          currentness={currentness}
          neighbours={neighbours(row)}
          open={open}
          setOpen={setOpen}
        />
      ),
      sortValue: (row) => {
        const first = neighbours(row)[0];
        return first === undefined ? undefined : rowLabel(first.node);
      },
      className: "rel-cell",
    };
  });
}

// The outline's state, its setter, and the boundary every expansion reads
// at: the inspected view's, so the whole outline is one view.
interface OutlineControl {
  view: OutlineView;
  setView: (update: (view: OutlineView) => OutlineView) => void;
  asOf: DecimalString;
}

const OutlineContext = createContext<OutlineControl | null>(null);

function useOutline(): OutlineControl {
  const control = useContext(OutlineContext);
  if (control === null) {
    throw new Error("the relationship outline needs its RelationshipOutline");
  }
  return control;
}

type NodeInspection = Extract<EntryInspection, { kind: "node" }>;

// The relationships of one node as an outline: a group per edge type and
// direction, and under each neighbour, on request, that neighbour's own
// groups, read at `asOf`.
export function RelationshipOutline({
  inspection,
  view,
  setView,
  asOf,
}: {
  inspection: NodeInspection;
  view: OutlineView;
  setView: (update: (view: OutlineView) => OutlineView) => void;
  asOf: DecimalString;
}) {
  const control = useMemo(
    () => ({ view, setView, asOf }),
    [view, setView, asOf],
  );
  return (
    <OutlineContext value={control}>
      <OutlineGroups
        inspection={inspection}
        path=""
        nodes={[entryKey(inspection.entry.id)]}
        testId="relationships"
      />
    </OutlineContext>
  );
}

// `path` names the row whose groups these are; `nodes` holds the keys of
// the nodes from the inspected one down to `owner`.
function OutlineGroups({
  inspection,
  path,
  nodes,
  testId,
}: {
  inspection: NodeInspection;
  path: string;
  nodes: readonly string[];
  testId?: string;
}) {
  const owner = inspection.entry;
  const groups = useMemo(
    () =>
      groupByRelation(incidents(inspection), ({ edge, direction }) => ({
        ty_name: edge.ty_name,
        direction,
      })),
    [inspection],
  );
  if (groups.length === 0) {
    return (
      <p className="muted outline-empty" data-testid={testId}>
        no edges
      </p>
    );
  }
  return (
    <ul className="outline" data-testid={testId}>
      {groups.map((group) => (
        <OutlineGroup
          key={relationKey(group.relation)}
          owner={owner}
          group={group}
          path={path}
          nodes={nodes}
        />
      ))}
    </ul>
  );
}

const neighbourLabel = ({ node }: Incident) => rowLabel(node);

function OutlineGroup({
  owner,
  group,
  path,
  nodes,
}: {
  owner: NodeRecord;
  group: RelationGroup<Incident>;
  path: string;
  nodes: readonly string[];
}) {
  const { view, setView } = useOutline();
  const id = groupId(path, group.relation);
  const state = groupState(view, id, owner, group.items, neighbourLabel);
  const label = relationLabel(group.relation);
  const listed = state.withHistorical
    ? group.items
    : group.items.filter(({ edge }) => edge.current);
  const tags = typeTags(countByType(listed.map(({ node }) => node)));
  const toggleHistorical = () =>
    setView((current) => {
      const next = setHistorical(current, id, !state.withHistorical, owner);
      const total = state.withHistorical
        ? group.items.length - state.historical
        : group.items.length;
      return setGroupOpen(next, id, true, total <= COLLAPSE_ABOVE);
    });
  return (
    <li
      className="rel-group"
      data-testid="relation-group"
      data-relation={label}
    >
      <div className="rel-head">
        <button
          type="button"
          className="rel-toggle"
          aria-expanded={state.open}
          onClick={() =>
            setView((current) =>
              setGroupOpen(
                current,
                id,
                !state.open,
                state.total <= COLLAPSE_ABOVE,
              ),
            )
          }
        >
          <span className="twisty">{state.open ? "▾" : "▸"}</span>
          <span className="rel-name">{label}</span>{" "}
          <span className="counts" data-testid="relation-count">
            {state.total}
          </span>
          {tags !== "" && (
            <span className="rel-types" data-testid="relation-types">
              · {tags}
            </span>
          )}
        </button>
        {state.historical > 0 && (
          <button
            type="button"
            className="link small"
            onClick={toggleHistorical}
          >
            {state.withHistorical
              ? `hide historical (${state.historical})`
              : `show historical (${state.historical})`}
          </button>
        )}
        {state.open && state.filterable && (
          <input
            type="search"
            className="rel-filter"
            aria-label={`filter ${label}`}
            placeholder="filter by label"
            value={state.filter}
            onChange={(event) =>
              setView((current) => setFilter(current, id, event.target.value))
            }
          />
        )}
        {state.open && state.filter !== "" && (
          <span className="muted small">
            {state.shown.length} of {state.total}
          </span>
        )}
      </div>
      {state.open && state.shown.length > 0 && (
        <ul className="rel-items">
          {state.shown.map((incident) => (
            <NeighbourRow
              key={entryKey(incident.edge.id)}
              incident={incident}
              path={path}
              nodes={nodes}
            />
          ))}
        </ul>
      )}
    </li>
  );
}

function NeighbourRow({
  incident,
  path: parent,
  nodes,
}: {
  incident: Incident;
  path: string;
  nodes: readonly string[];
}) {
  const { view, setView } = useOutline();
  const { edge, node: other } = incident;
  const path = rowPath(parent, edge.id);
  const marked = onPath(nodes, other.id);
  const expanded = !marked && view.expanded.includes(path);
  const label = rowLabel(other);
  return (
    <li
      className={edge.current ? "rel-item" : "rel-item historical"}
      data-testid="neighbour"
    >
      <div className="rel-row">
        {marked ? (
          <span className="twisty" />
        ) : (
          <button
            type="button"
            className="twisty"
            aria-expanded={expanded}
            aria-label={`${expanded ? "collapse" : "expand"} ${label}`}
            title={`${expanded ? "hide" : "show"} the relationships of ${label}`}
            onClick={() =>
              setView((current) => setExpanded(current, path, !expanded))
            }
          >
            {expanded ? "▾" : "▸"}
          </button>
        )}
        <NodeLink node={other} />
        <span className="ty">{typeName(other.ty_name)}</span>
        {!other.current && <CurrentBadge current={false} kind="node" />}
        {marked && <span className="muted on-path">(on this path)</span>}
        <span className="edge-ref">
          edge <IdLink id={edge.id} />
          {!edge.current && (
            <>
              {" "}
              <CurrentBadge current={false} kind="edge" />
            </>
          )}
        </span>
      </div>
      {expanded && (
        <NeighbourOutline
          id={other.id}
          path={path}
          nodes={[...nodes, entryKey(other.id)]}
        />
      )}
    </li>
  );
}

// A neighbour's own relationships, read when first expanded at the
// inspected view's boundary.
function NeighbourOutline({
  id,
  path,
  nodes,
}: {
  id: EntryId;
  path: string;
  nodes: readonly string[];
}): ReactNode {
  const { asOf } = useOutline();
  const load = useCall("entries.inspect", { as_of: asOf, id });
  const result = latest(load);
  if (load.status === "failed") {
    return <FailureView failure={load.failure} compact />;
  }
  if (result === null) {
    return <p className="muted outline-empty">loading…</p>;
  }
  if (result.kind !== "node") {
    return null;
  }
  return <OutlineGroups inspection={result} path={path} nodes={nodes} />;
}
