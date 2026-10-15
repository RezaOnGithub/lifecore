import type { NodeEntry } from "../contract.ts";
import { entryKey } from "./labels.ts";

export interface LineageRow<N extends NodeEntry> {
  node: N;
  depth: number;
}

// The revision tree of `lineage` (append order, root first) as indented rows:
// each node under its predecessor, siblings in append order. A node whose
// predecessor is not in the list is shown as a root rather than dropped.
export function lineageRows<N extends NodeEntry>(
  lineage: readonly N[],
): LineageRow<N>[] {
  const keys = new Set(lineage.map((node) => entryKey(node.id)));
  const children = new Map<string, N[]>();
  const roots: N[] = [];
  for (const node of lineage) {
    const parent =
      node.predecessor === undefined ? null : entryKey(node.predecessor);
    if (parent === null || !keys.has(parent)) {
      roots.push(node);
      continue;
    }
    const siblings = children.get(parent);
    if (siblings === undefined) {
      children.set(parent, [node]);
    } else {
      siblings.push(node);
    }
  }
  const rows: LineageRow<N>[] = [];
  const visit = (node: N, depth: number) => {
    rows.push({ node, depth });
    for (const child of children.get(entryKey(node.id)) ?? []) {
      visit(child, depth + 1);
    }
  };
  for (const root of roots) {
    visit(root, 0);
  }
  return rows;
}
