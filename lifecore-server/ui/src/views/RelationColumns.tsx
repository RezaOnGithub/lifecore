import type {
  Currentness,
  EdgeRecord,
  EntryId,
  NodeRecord,
} from "../contract.ts";
import { useExplore, useOpen } from "../links.tsx";
import { entryKey, rowLabel, typeName } from "../model/labels.ts";
import { COLLAPSE_ABOVE } from "../model/outline.ts";
import { truncate } from "../model/payload.ts";
import { neighboursForm } from "../model/query.ts";
import {
  countByType,
  countWords,
  type Neighbour,
  type Relation,
  relationCells,
  relationKey,
  relationLabel,
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
