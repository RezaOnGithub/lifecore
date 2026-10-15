import type { EntryId } from "../contract.ts";
import { entryKey } from "./labels.ts";
import { type Relation, relationKey } from "./relations.ts";

// What the inspector's relationship outline remembers. A neighbour row is
// named by the edges leading to it from the inspected node, joined by `/`,
// so the same node reached another way is another row; a group by its
// owner row's path and its relation. Groups and historical toggles hold
// only choices that differ from their defaults, and filters only non-empty
// text.
export interface OutlineView {
  expanded: readonly string[];
  groups: Readonly<Record<string, boolean>>;
  historical: Readonly<Record<string, boolean>>;
  filters: Readonly<Record<string, string>>;
}

export const emptyOutline: OutlineView = {
  expanded: [],
  groups: {},
  historical: {},
  filters: {},
};

// Groups with more edges than this start collapsed and offer a filter.
export const COLLAPSE_ABOVE = 20;

// The path of the row reached over `edge` from the row at `parent`; the
// inspected node's own groups hang at the empty path.
export function rowPath(parent: string, edge: EntryId): string {
  const key = entryKey(edge);
  return parent === "" ? key : `${parent}/${key}`;
}

export function groupId(path: string, relation: Relation): string {
  return `${path}|${relationKey(relation)}`;
}

// `nodes` holds the keys of the nodes from the inspected one down to the
// group's owner. A neighbour among them is marked "(on this path)" and not
// expanded again, so the outline never loops.
export function onPath(nodes: readonly string[], neighbour: EntryId): boolean {
  return nodes.includes(entryKey(neighbour));
}

// Every edge of a superseded node is historical, so hiding them would leave
// nothing; a current node's historical edges wait for a click.
export function historicalByDefault(owner: { current: boolean }): boolean {
  return !owner.current;
}

export interface GroupState<T> {
  withHistorical: boolean;
  historical: number;
  // The items on show: current ones, or all, then those matching the filter.
  shown: T[];
  // Before the filter, for the count and the default of `open`.
  total: number;
  open: boolean;
  filter: string;
  filterable: boolean;
}

export function groupState<T extends { edge: { current: boolean } }>(
  view: OutlineView,
  id: string,
  owner: { current: boolean },
  items: readonly T[],
  label: (item: T) => string,
): GroupState<T> {
  const historical = items.filter(({ edge }) => !edge.current).length;
  const withHistorical = view.historical[id] ?? historicalByDefault(owner);
  const listed = withHistorical
    ? [...items]
    : items.filter(({ edge }) => edge.current);
  const filterable = listed.length > COLLAPSE_ABOVE;
  const filter = filterable ? (view.filters[id] ?? "") : "";
  const needle = filter.trim().toLowerCase();
  const shown =
    needle === ""
      ? listed
      : listed.filter((item) => label(item).toLowerCase().includes(needle));
  return {
    withHistorical,
    historical,
    shown,
    total: listed.length,
    open: view.groups[id] ?? listed.length <= COLLAPSE_ABOVE,
    filter,
    filterable,
  };
}

function setOrDrop<V>(
  record: Readonly<Record<string, V>>,
  key: string,
  value: V | undefined,
): Record<string, V> {
  const next = { ...record };
  if (value === undefined) {
    delete next[key];
  } else {
    next[key] = value;
  }
  return next;
}

export function setExpanded(
  view: OutlineView,
  path: string,
  expanded: boolean,
): OutlineView {
  const others = view.expanded.filter((open) => open !== path);
  return { ...view, expanded: expanded ? [...others, path] : others };
}

// A group's open state, kept only while it differs from the default.
export function setGroupOpen(
  view: OutlineView,
  id: string,
  open: boolean,
  byDefault: boolean,
): OutlineView {
  return {
    ...view,
    groups: setOrDrop(view.groups, id, open === byDefault ? undefined : open),
  };
}

export function setHistorical(
  view: OutlineView,
  id: string,
  shown: boolean,
  owner: { current: boolean },
): OutlineView {
  return {
    ...view,
    historical: setOrDrop(
      view.historical,
      id,
      shown === historicalByDefault(owner) ? undefined : shown,
    ),
  };
}

export function setFilter(
  view: OutlineView,
  id: string,
  text: string,
): OutlineView {
  return {
    ...view,
    filters: setOrDrop(view.filters, id, text === "" ? undefined : text),
  };
}
