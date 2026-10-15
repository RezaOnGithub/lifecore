import type { EdgeEntry, NodeEntry } from "../contract.ts";
import { entryKey } from "./labels.ts";

// Which edges define parent → child: `outgoing` reads an edge from its `from`
// (parent) to its `to` (child); `incoming` the reverse.
export interface ParentRelation {
  ty_name: string;
  direction: "outgoing" | "incoming";
}

export type ThreadRow =
  // First occurrence, expanded.
  | { kind: "node"; key: string; depth: number }
  // A later occurrence, collapsed; `under` is the parent of the first
  // occurrence, null when that was a root.
  | { kind: "repeat"; key: string; depth: number; under: string | null }
  // A revisit on the current path.
  | { kind: "cycle"; key: string; depth: number };

export interface Thread {
  rows: ThreadRow[];
  // Stripe index of every node that occurs more than once, by first occurrence.
  stripes: Map<string, number>;
}

export const STRIPE_COLORS = 8;

// Beyond the palette the colors return with a numeric suffix.
export function stripeMark(index: number): { color: number; suffix: string } {
  return {
    color: index % STRIPE_COLORS,
    suffix:
      index < STRIPE_COLORS
        ? ""
        : String(Math.floor(index / STRIPE_COLORS) + 1),
  };
}

export function buildThread(
  nodes: readonly NodeEntry[],
  edges: readonly EdgeEntry[],
  relation: ParentRelation,
): Thread {
  const order = new Map<string, number>();
  nodes.forEach((node, index) => {
    order.set(entryKey(node.id), index);
  });
  const children = new Map<string, Set<string>>();
  const hasParent = new Set<string>();
  for (const edge of edges) {
    if (edge.ty_name !== relation.ty_name) {
      continue;
    }
    const [parent, child] =
      relation.direction === "outgoing"
        ? [edge.from, edge.to]
        : [edge.to, edge.from];
    const parentKey = entryKey(parent);
    const childKey = entryKey(child);
    if (!order.has(parentKey) || !order.has(childKey)) {
      continue;
    }
    hasParent.add(childKey);
    let set = children.get(parentKey);
    if (set === undefined) {
      set = new Set();
      children.set(parentKey, set);
    }
    set.add(childKey);
  }
  const byAppend = (a: string, b: string) =>
    (order.get(a) ?? 0) - (order.get(b) ?? 0);
  const rows: ThreadRow[] = [];
  const firstParent = new Map<string, string | null>();
  const visit = (
    key: string,
    depth: number,
    parent: string | null,
    path: ReadonlySet<string>,
  ) => {
    if (path.has(key)) {
      rows.push({ kind: "cycle", key, depth });
      return;
    }
    const under = firstParent.get(key);
    if (under !== undefined) {
      rows.push({ kind: "repeat", key, depth, under });
      return;
    }
    firstParent.set(key, parent);
    rows.push({ kind: "node", key, depth });
    const next = new Set(path);
    next.add(key);
    for (const child of [...(children.get(key) ?? [])].sort(byAppend)) {
      visit(child, depth + 1, key, next);
    }
  };
  const keys = nodes.map((node) => entryKey(node.id));
  for (const key of keys) {
    if (!hasParent.has(key)) {
      visit(key, 0, null, new Set());
    }
  }
  // A component where every node has a parent is a cycle without a root; it
  // starts at its first node in append order so it is still shown.
  for (const key of keys) {
    if (!firstParent.has(key)) {
      visit(key, 0, null, new Set());
    }
  }
  const counts = new Map<string, number>();
  for (const row of rows) {
    counts.set(row.key, (counts.get(row.key) ?? 0) + 1);
  }
  const stripes = new Map<string, number>();
  for (const row of rows) {
    if ((counts.get(row.key) ?? 0) > 1 && !stripes.has(row.key)) {
      stripes.set(row.key, stripes.size);
    }
  }
  return { rows, stripes };
}
