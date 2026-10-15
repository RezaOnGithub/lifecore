import { useState } from "react";
import type {
  EdgeRecord,
  EntryId,
  IncidentEdge,
  NodeRecord,
} from "../contract.ts";
import { latest, useAsOf, useCall } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { CurrentBadge, IdLink, NodeLink, useOpen } from "../links.tsx";
import { entryKey, LABEL_LIMIT, rowLabel, typeName } from "../model/labels.ts";
import { truncate } from "../model/payload.ts";
import {
  groupByRelation,
  type Neighbour,
  type RelationGroup,
  relationCells,
  relationKey,
  relationLabel,
} from "../model/relations.ts";
import type { ExtraColumn } from "./EntryTable.tsx";

// A neighbour's label linking to its inspector, compact enough for a table
// cell: the position, type, and currentness are in the tooltip, and a
// superseded node or a historical edge is styled.
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
      {truncate(label, LABEL_LIMIT)}
    </button>
  );
}

// The neighbours in one relationship cell: the first `limit`, then "+N more"
// expanding in place.
export function NeighbourList({
  neighbours,
  limit = 3,
}: {
  neighbours: readonly Neighbour<NodeRecord, EdgeRecord>[];
  limit?: number;
}) {
  const [all, setAll] = useState(false);
  const shown = all ? neighbours : neighbours.slice(0, limit);
  const more = neighbours.length - shown.length;
  return (
    <span className={all ? "neighbours all" : "neighbours"}>
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
      {more > 0 && (
        <>
          {" "}
          <button
            type="button"
            className="link more"
            onClick={() => setAll(true)}
          >
            +{more} more
          </button>
        </>
      )}
      {all && neighbours.length > limit && (
        <>
          {" "}
          <button
            type="button"
            className="link more"
            onClick={() => setAll(false)}
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
export function relationColumns<R extends NodeRecord>(
  rows: readonly R[],
  edges: readonly EdgeRecord[],
  nodes: ReadonlyMap<string, NodeRecord>,
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
      cell: (row) => <NeighbourList neighbours={neighbours(row)} />,
      sortValue: (row) => {
        const first = neighbours(row)[0];
        return first === undefined ? undefined : rowLabel(first.node);
      },
      className: "rel-cell",
    };
  });
}

// Groups with more edges than this start collapsed.
export const COLLAPSE_ABOVE = 20;

// The relationships of one node as an outline: a group per edge type and
// direction, and under each neighbour, on request, that neighbour's own
// groups. `path` holds the keys of the nodes from the inspected one down to
// `owner`; a neighbour on it is marked and not expanded again, so the
// outline never loops.
export function RelationshipOutline({
  owner,
  edges,
  path,
  testId,
}: {
  owner: NodeRecord;
  edges: readonly IncidentEdge[];
  path: readonly string[];
  testId?: string;
}) {
  const groups = groupByRelation(edges, ({ edge, direction }) => ({
    ty_name: edge.ty_name,
    direction,
  }));
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
          group={group}
          // Every edge of a superseded node is historical, so hiding them
          // would leave nothing; a current node's history waits for a click.
          historical={!owner.current}
          path={path}
        />
      ))}
    </ul>
  );
}

function OutlineGroup({
  group,
  historical: historicalByDefault,
  path,
}: {
  group: RelationGroup<IncidentEdge>;
  historical: boolean;
  path: readonly string[];
}) {
  const current = group.items.filter(({ edge }) => edge.current);
  const historical = group.items.length - current.length;
  const [withHistorical, setWithHistorical] = useState(historicalByDefault);
  const shown = withHistorical ? group.items : current;
  const [open, setOpen] = useState(shown.length <= COLLAPSE_ABOVE);
  const label = relationLabel(group.relation);
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
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          <span className="twisty">{open ? "▾" : "▸"}</span>
          <span className="rel-name">{label}</span>{" "}
          <span className="counts" data-testid="relation-count">
            {shown.length}
          </span>
        </button>
        {historical > 0 && (
          <button
            type="button"
            className="link small"
            onClick={() => {
              setWithHistorical(!withHistorical);
              setOpen(true);
            }}
          >
            {withHistorical
              ? `hide historical (${historical})`
              : `show historical (${historical})`}
          </button>
        )}
      </div>
      {open && shown.length > 0 && (
        <ul className="rel-items">
          {shown.map((incident) => (
            <NeighbourRow
              key={entryKey(incident.edge.id)}
              incident={incident}
              path={path}
            />
          ))}
        </ul>
      )}
    </li>
  );
}

function NeighbourRow({
  incident,
  path,
}: {
  incident: IncidentEdge;
  path: readonly string[];
}) {
  const { edge, other } = incident;
  const key = entryKey(other.id);
  const onPath = path.includes(key);
  const [expanded, setExpanded] = useState(false);
  const label = rowLabel(other);
  return (
    <li
      className={edge.current ? "rel-item" : "rel-item historical"}
      data-testid="neighbour"
    >
      <div className="rel-row">
        {onPath ? (
          <span className="twisty" />
        ) : (
          <button
            type="button"
            className="twisty"
            aria-expanded={expanded}
            aria-label={`${expanded ? "collapse" : "expand"} ${label}`}
            title={`${expanded ? "hide" : "show"} the relationships of ${label}`}
            onClick={() => setExpanded(!expanded)}
          >
            {expanded ? "▾" : "▸"}
          </button>
        )}
        <NodeLink node={other} />
        <span className="ty">{typeName(other.ty_name)}</span>
        {!other.current && <CurrentBadge current={false} kind="node" />}
        {onPath && <span className="muted on-path">(on this path)</span>}
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
      {expanded && <NeighbourOutline id={other.id} path={[...path, key]} />}
    </li>
  );
}

// A neighbour's own relationships, read when first expanded, under the
// global as-of boundary like every other read.
function NeighbourOutline({
  id,
  path,
}: {
  id: EntryId;
  path: readonly string[];
}) {
  const asOf = useAsOf();
  const load = useCall("entries.inspect", { ...asOf, id });
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
  return (
    <RelationshipOutline
      owner={result.entry}
      edges={result.edges}
      path={path}
    />
  );
}
