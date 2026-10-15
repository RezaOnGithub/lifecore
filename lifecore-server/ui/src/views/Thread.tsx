import { memo, type ReactNode, type SyntheticEvent, useMemo } from "react";
import type { EdgeRecord, NodeRecord } from "../contract.ts";
import { CurrentBadge, NodeLink } from "../links.tsx";
import { entryKey, LABEL_LIMIT, rowLabel, typeName } from "../model/labels.ts";
import { truncate } from "../model/payload.ts";
import { edgeWords, relationLabel } from "../model/relations.ts";
import {
  buildThread,
  type ParentRelation,
  preferredDirection,
  stripeMark,
} from "../model/thread.ts";

// The select's value for every edge type; a type name cannot clash with it
// because the select lists names only as `type:<name>`.
const ALL = "all";

// Every row of a node shares its highlight while the pointer is over one of
// them. The rows are marked in the DOM, outside React, so a hover renders
// nothing: `data-hover` on the rows whose `data-key` is the hovered one's.
function highlight(event: SyntheticEvent<HTMLOListElement>) {
  const list = event.currentTarget;
  const item = (event.target as Element).closest("li[data-key]");
  const key = item?.getAttribute("data-key") ?? null;
  if (list.getAttribute("data-hover") === key) {
    return;
  }
  for (const marked of list.querySelectorAll("li[data-hover]")) {
    marked.removeAttribute("data-hover");
  }
  if (key === null) {
    list.removeAttribute("data-hover");
    return;
  }
  list.setAttribute("data-hover", key);
  for (const same of list.querySelectorAll(
    `li[data-key="${CSS.escape(key)}"]`,
  )) {
    same.setAttribute("data-hover", "");
  }
}

function unhighlight(event: SyntheticEvent<HTMLOListElement>) {
  const list = event.currentTarget;
  list.removeAttribute("data-hover");
  for (const marked of list.querySelectorAll("li[data-hover]")) {
    marked.removeAttribute("data-hover");
  }
}

// The result as a mailing-list thread along one edge type, or every type,
// and a direction. `actions` adds controls after each row's label;
// `chosen` is the relation the user picked, null for the default.
function ThreadViewOf({
  nodes,
  edges,
  actions,
  chosen,
  onChoose,
}: {
  nodes: readonly NodeRecord[];
  edges: readonly EdgeRecord[];
  actions?: (node: NodeRecord) => ReactNode;
  chosen: ParentRelation | null;
  onChoose: (relation: ParentRelation) => void;
}) {
  const edgeTypes = useMemo(
    () => [...new Set(edges.map((edge) => edge.ty_name))],
    [edges],
  );
  // Default: every edge type when the result has several, else its one
  // type, in the direction that nests the most. A chosen type the result no
  // longer has falls back to the default.
  const fallback = edgeTypes.length > 1 ? null : (edgeTypes[0] ?? null);
  const preferred = useMemo(
    () => (chosen === null ? preferredDirection(nodes, edges, fallback) : null),
    [chosen, nodes, edges, fallback],
  );
  const relation: ParentRelation =
    chosen === null
      ? { ty_name: fallback, direction: preferred ?? "outgoing" }
      : chosen.ty_name === null || edgeTypes.includes(chosen.ty_name)
        ? chosen
        : { ...chosen, ty_name: fallback };
  const byKey = useMemo(
    () => new Map(nodes.map((node) => [entryKey(node.id), node])),
    [nodes],
  );
  const { ty_name, direction } = relation;
  const thread = useMemo(
    () => buildThread(nodes, edges, { ty_name, direction }),
    [nodes, edges, ty_name, direction],
  );
  const occurrences = useMemo(() => {
    const counts = new Map<string, number>();
    for (const row of thread.rows) {
      counts.set(row.key, (counts.get(row.key) ?? 0) + 1);
    }
    return counts;
  }, [thread]);
  // Only the all-types mode names the edge types on each row; with one type
  // the select says it. The arrow is the child's own: with outgoing, the
  // parent is the edge's `from`, so the edge comes to the child.
  const prefixed = relation.ty_name === null;
  const childDirection = direction === "outgoing" ? "incoming" : "outgoing";
  const seen = new Map<string, number>();
  return (
    <section>
      <div className="row" data-testid="thread-controls">
        <label>
          parent → child by{" "}
          <select
            data-testid="thread-relation"
            value={relation.ty_name === null ? ALL : `type:${relation.ty_name}`}
            disabled={edgeTypes.length === 0}
            onChange={(event) =>
              onChoose({
                ...relation,
                ty_name:
                  event.target.value === ALL
                    ? null
                    : event.target.value.slice("type:".length),
              })
            }
          >
            {edgeTypes.length === 0 && <option value={ALL}>no edges</option>}
            {edgeTypes.length > 0 && (
              <option value={ALL}>all edge types</option>
            )}
            {edgeTypes.map((name) => (
              <option key={name} value={`type:${name}`}>
                {typeName(name)}
              </option>
            ))}
          </select>
        </label>
        <label>
          direction{" "}
          <select
            data-testid="thread-direction"
            value={relation.direction}
            disabled={edgeTypes.length === 0}
            onChange={(event) =>
              onChoose({
                ...relation,
                direction: event.target.value as ParentRelation["direction"],
              })
            }
          >
            <option value="outgoing">outgoing: from → to</option>
            <option value="incoming">incoming: to → from</option>
          </select>
        </label>
      </div>
      <ol
        className="thread"
        data-testid="thread"
        onMouseOver={highlight}
        onFocus={highlight}
        onMouseLeave={unhighlight}
        onBlur={unhighlight}
      >
        {thread.rows.map((row) => {
          const node = byKey.get(row.key);
          if (node === undefined) {
            return null;
          }
          // The nth occurrence of a node is the row's identity.
          const occurrence = (seen.get(row.key) ?? 0) + 1;
          seen.set(row.key, occurrence);
          const stripe = thread.stripes.get(row.key);
          const mark = stripe === undefined ? null : stripeMark(stripe);
          const classes = [row.kind, mark && `stripe-${mark.color}`]
            .filter(Boolean)
            .join(" ");
          const parent =
            row.kind === "repeat" && row.under !== null
              ? byKey.get(row.under)
              : undefined;
          return (
            <li
              key={`${row.key}#${occurrence}`}
              className={classes}
              style={{ paddingLeft: `${row.depth * 1.5 + 0.5}rem` }}
              data-depth={row.depth}
              data-position={node.id.position}
              data-key={row.key}
            >
              {mark && (
                <span
                  className="stripe-mark"
                  title={`appears ${occurrences.get(row.key)} times`}
                >
                  {mark.suffix}
                </span>
              )}
              {prefixed && row.via.length > 0 && (
                <span
                  className="via-type"
                  data-testid="via-type"
                  title={
                    childDirection === "incoming"
                      ? `edges from the parent: ${edgeWords("parent", row.via.join(", "), rowLabel(node))}`
                      : `edges to the parent: ${edgeWords(rowLabel(node), row.via.join(", "), "parent")}`
                  }
                >
                  {row.via
                    .map((name) =>
                      relationLabel({
                        ty_name: name,
                        direction: childDirection,
                      }),
                    )
                    .join(", ")}
                </span>
              )}
              <NodeLink node={node} />{" "}
              <CurrentBadge current={node.current} kind="node" />
              {row.kind === "repeat" && (
                <span className="muted">
                  {" "}
                  also under{" "}
                  {parent
                    ? truncate(rowLabel(parent), LABEL_LIMIT)
                    : "the roots"}
                </span>
              )}
              {row.kind === "cycle" && <span className="muted"> (cycle)</span>}
              {actions && <span className="row-actions">{actions(node)}</span>}
            </li>
          );
        })}
      </ol>
    </section>
  );
}

export const ThreadView = memo(ThreadViewOf);
