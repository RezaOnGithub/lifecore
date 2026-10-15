import { type ReactNode, useState } from "react";
import type { EdgeRecord, NodeRecord } from "../contract.ts";
import { CurrentBadge, NodeLink } from "../links.tsx";
import { entryKey, LABEL_LIMIT, rowLabel, typeName } from "../model/labels.ts";
import { truncate } from "../model/payload.ts";
import {
  buildThread,
  type ParentRelation,
  preferredDirection,
  stripeMark,
} from "../model/thread.ts";

// The select's value for every edge type; a type name cannot clash with it
// because the select lists names only as `type:<name>`.
const ALL = "all";

// The result as a mailing-list thread along one edge type, or every type,
// and a direction. `actions` adds controls after each row's label.
export function ThreadView({
  nodes,
  edges,
  actions,
}: {
  nodes: readonly NodeRecord[];
  edges: readonly EdgeRecord[];
  actions?: (node: NodeRecord) => ReactNode;
}) {
  const edgeTypes = [...new Set(edges.map((edge) => edge.ty_name))];
  const [chosen, setChosen] = useState<ParentRelation | null>(null);
  const [hover, setHover] = useState<string | null>(null);
  // Default: every edge type when the result has several, else its one
  // type, in the direction that nests the most. A chosen type the result no
  // longer has falls back to the default.
  const fallback = edgeTypes.length > 1 ? null : (edgeTypes[0] ?? null);
  const relation: ParentRelation =
    chosen === null
      ? {
          ty_name: fallback,
          direction: preferredDirection(nodes, edges, fallback),
        }
      : chosen.ty_name === null || edgeTypes.includes(chosen.ty_name)
        ? chosen
        : { ...chosen, ty_name: fallback };
  const byKey = new Map(nodes.map((node) => [entryKey(node.id), node]));
  const thread = buildThread(nodes, edges, relation);
  const occurrences = new Map<string, number>();
  for (const row of thread.rows) {
    occurrences.set(row.key, (occurrences.get(row.key) ?? 0) + 1);
  }
  // Only the all-types mode names the edge type on each row; with one type
  // the select says it.
  const prefixed = relation.ty_name === null;
  const arrow = relation.direction === "outgoing" ? "→" : "←";
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
              setChosen({
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
              setChosen({
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
      <ol className="thread" data-testid="thread">
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
          const classes = [
            row.kind,
            mark && `stripe-${mark.color}`,
            hover === row.key && "hl",
          ]
            .filter(Boolean)
            .join(" ");
          const under =
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
              onMouseEnter={() => setHover(row.key)}
              onMouseLeave={() => setHover(null)}
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
                    relation.direction === "outgoing"
                      ? "edge from the parent to this node"
                      : "edge from this node to the parent"
                  }
                >
                  {row.via.map(typeName).join(", ")} {arrow}
                </span>
              )}
              <NodeLink node={node} />{" "}
              <CurrentBadge current={node.current} kind="node" />
              {row.kind === "repeat" && (
                <span className="muted">
                  {" "}
                  also under{" "}
                  {under ? truncate(rowLabel(under), LABEL_LIMIT) : "the roots"}
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
