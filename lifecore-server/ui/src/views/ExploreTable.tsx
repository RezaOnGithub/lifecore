import { memo, useCallback, useMemo } from "react";
import type {
  Currentness,
  EdgeRecord,
  NodeRecord,
  ViewQuery,
} from "../contract.ts";
import { schemaOf, useTypes } from "../data.tsx";
import { IdLink, NodeLink, TypeLink, useOpen } from "../links.tsx";
import { entryKey, rowLabel } from "../model/labels.ts";
import type { ExploreAction, QueriedRow, Shaped } from "../model/query.ts";
import { directionAt, edgeWords, relationLabel } from "../model/relations.ts";
import type { TableView } from "../navigation.ts";
import { EntryTable, type ExtraColumn } from "./EntryTable.tsx";
import { relationColumns } from "./Relationships.tsx";

// How a node was reached: at a hop, over an edge read from this node
// (`→ PartOf` when the edge goes from it, `← PartOf` when it comes to it),
// from the node it was reached from. The edge may be absent from the
// result's edges (the edge filter is separate); then only its identity is
// known.
function Via({
  row,
  edges,
  nodes,
}: {
  row: QueriedRow;
  edges: ReadonlyMap<string, EdgeRecord>;
  nodes: ReadonlyMap<string, NodeRecord>;
}) {
  const { provenance } = row;
  if (provenance.kind === "seed") {
    return <span>seed</span>;
  }
  const edge = edges.get(entryKey(provenance.via));
  const from = nodes.get(entryKey(provenance.from));
  return (
    <span className="via">
      <span className="hop" title={`reached at hop ${provenance.hop}`}>
        {provenance.hop}
      </span>{" "}
      via{" "}
      {edge === undefined ? (
        <IdLink id={provenance.via} />
      ) : (
        <EdgeArrow edge={edge} row={row} from={from} />
      )}{" "}
      from{" "}
      {from ? (
        <NodeLink node={from} position={false} />
      ) : (
        <IdLink id={provenance.from} />
      )}
    </span>
  );
}

// The traversed edge in the one arrow notation, read from the row's node,
// linking to its inspector.
function EdgeArrow({
  edge,
  row,
  from,
}: {
  edge: EdgeRecord;
  row: NodeRecord;
  from: NodeRecord | undefined;
}) {
  const open = useOpen();
  const direction = directionAt(edge, row.id);
  const other = from === undefined ? `@${edge.from.position}` : rowLabel(from);
  const words =
    direction === "outgoing"
      ? edgeWords(rowLabel(row), edge.ty_name, other)
      : edgeWords(other, edge.ty_name, rowLabel(row));
  return (
    <button
      type="button"
      className="link via-edge"
      title={`edge @${edge.id.position}: ${words}`}
      onClick={() => open({ kind: "inspector", id: edge.id })}
    >
      {relationLabel({ ty_name: edge.ty_name, direction })}
    </button>
  );
}

// "focus" makes the node the only seed; "+ seed" adds it. Both run.
export function RowActions({
  node,
  seeded,
  act,
}: {
  node: NodeRecord;
  seeded: boolean;
  act: (action: ExploreAction) => void;
}) {
  return (
    <span className="row-actions">
      <button
        type="button"
        className="mini"
        title="explore from this node alone, with the same expansion"
        onClick={() => act({ kind: "focus", id: node.id })}
      >
        focus
      </button>
      <button
        type="button"
        className="mini"
        disabled={seeded}
        title={seeded ? "already a seed" : "add this node to the seeds"}
        onClick={() => act({ kind: "seed", id: node.id })}
      >
        + seed
      </button>
    </span>
  );
}

export const TypeTable = memo(function TypeTable({
  ty_name,
  rows,
  shaped,
  result,
  relationships,
  currentness,
  lead,
  view,
  onView,
}: {
  ty_name: string;
  rows: QueriedRow[];
  shaped: Shaped;
  result: ViewQuery;
  relationships: boolean;
  currentness: Currentness;
  lead: ExtraColumn<QueriedRow>[];
  view: TableView;
  onView: (ty_name: string, update: (view: TableView) => TableView) => void;
}) {
  const types = useTypes();
  const extra = useMemo(() => {
    const via: ExtraColumn<QueriedRow> = {
      key: "via",
      header: "via",
      cell: (row) => (
        <Via row={row} edges={shaped.edges} nodes={shaped.nodes} />
      ),
      className: "via-cell",
      sortValue: (row) => ({
        kind: "integer",
        value: String(row.provenance.kind === "seed" ? 0 : row.provenance.hop),
      }),
    };
    // The relationships are those among the result's edges, which the edge
    // filter selects.
    return relationships
      ? [via, ...relationColumns(rows, result.edges, shaped.nodes, currentness)]
      : [via];
  }, [rows, shaped, result, relationships, currentness]);
  const onTable = useCallback(
    (update: (view: TableView) => TableView) => onView(ty_name, update),
    [onView, ty_name],
  );
  return (
    <section>
      <h3>
        <TypeLink name={ty_name} />{" "}
        <span className="muted">
          {rows.length} node{rows.length === 1 ? "" : "s"}
        </span>
      </h3>
      <EntryTable
        rows={rows}
        schema={schemaOf(types, ty_name)}
        ty_name={ty_name}
        lead={lead}
        extra={extra}
        showCurrent={rows.some((row) => !row.current)}
        view={view}
        onView={onTable}
      />
    </section>
  );
});
