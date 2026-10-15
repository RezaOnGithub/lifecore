import type { EdgeEntry, NodeEntry } from "../contract.ts";
import { compareStrings } from "./columns.ts";
import { entryKey, typeName } from "./labels.ts";

// One kind of relationship seen from a node: an edge type, and whether the
// node is the edge's `from` (outgoing) or its `to` (incoming).
export interface Relation {
  ty_name: string;
  direction: "outgoing" | "incoming";
}

export function relationKey(relation: Relation): string {
  return `${relation.direction}:${relation.ty_name}`;
}

// `→ PartOf` for outgoing, `← PartOf` for incoming.
export function relationLabel(relation: Relation): string {
  const arrow = relation.direction === "outgoing" ? "→" : "←";
  return `${arrow} ${typeName(relation.ty_name)}`;
}

// Outgoing before incoming, then by type name, so a node's relationships
// appear in the same places wherever it is shown.
export function compareRelations(a: Relation, b: Relation): number {
  if (a.direction !== b.direction) {
    return a.direction === "outgoing" ? -1 : 1;
  }
  return compareStrings(a.ty_name, b.ty_name);
}

export interface RelationGroup<T> {
  relation: Relation;
  items: T[];
}

// Items grouped by relation, groups in relation order, items in input order.
export function groupByRelation<T>(
  items: readonly T[],
  relationOf: (item: T) => Relation,
): RelationGroup<T>[] {
  const groups = new Map<string, RelationGroup<T>>();
  for (const item of items) {
    const relation = relationOf(item);
    const key = relationKey(relation);
    const group = groups.get(key);
    if (group === undefined) {
      groups.set(key, { relation, items: [item] });
    } else {
      group.items.push(item);
    }
  }
  return [...groups.values()].sort((a, b) =>
    compareRelations(a.relation, b.relation),
  );
}

export interface Neighbour<N, E> {
  node: N;
  edge: E;
}

export interface RelationCells<N, E> {
  // Every relation some row has, in relation order.
  relations: Relation[];
  // Row key, then relation key, to the neighbours in edge order.
  cells: Map<string, Map<string, Neighbour<N, E>[]>>;
}

// The neighbours of each row over `edges`, by relation. An edge counts for
// each end that is a row, so a self-loop is both outgoing and incoming; an
// edge whose other end is not in `nodes` is skipped.
export function relationCells<N extends NodeEntry, E extends EdgeEntry>(
  rows: readonly NodeEntry[],
  edges: readonly E[],
  nodes: ReadonlyMap<string, N>,
): RelationCells<N, E> {
  const rowKeys = new Set(rows.map((row) => entryKey(row.id)));
  const relations = new Map<string, Relation>();
  const cells = new Map<string, Map<string, Neighbour<N, E>[]>>();
  const add = (row: string, relation: Relation, other: string, edge: E) => {
    const node = nodes.get(other);
    if (node === undefined) {
      return;
    }
    const key = relationKey(relation);
    relations.set(key, relation);
    let byRelation = cells.get(row);
    if (byRelation === undefined) {
      byRelation = new Map();
      cells.set(row, byRelation);
    }
    const list = byRelation.get(key);
    if (list === undefined) {
      byRelation.set(key, [{ node, edge }]);
    } else {
      list.push({ node, edge });
    }
  };
  for (const edge of edges) {
    const from = entryKey(edge.from);
    const to = entryKey(edge.to);
    if (rowKeys.has(from)) {
      add(from, { ty_name: edge.ty_name, direction: "outgoing" }, to, edge);
    }
    if (rowKeys.has(to)) {
      add(to, { ty_name: edge.ty_name, direction: "incoming" }, from, edge);
    }
  }
  return {
    relations: [...relations.values()].sort(compareRelations),
    cells,
  };
}
