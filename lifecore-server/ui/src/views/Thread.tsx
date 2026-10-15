import { useState } from "react";
import type { EdgeRecord, NodeRecord } from "../contract.ts";
import { CurrentBadge, NodeLink } from "../links.tsx";
import { entryKey, LABEL_LIMIT, rowLabel } from "../model/labels.ts";
import { truncate } from "../model/payload.ts";
import {
  buildThread,
  type ParentRelation,
  stripeMark,
} from "../model/thread.ts";

// The result as a mailing-list thread along one edge type and direction.
export function ThreadView({
  nodes,
  edges,
}: {
  nodes: readonly NodeRecord[];
  edges: readonly EdgeRecord[];
}) {
  const edgeTypes = [...new Set(edges.map((edge) => edge.ty_name))];
  const [chosen, setChosen] = useState<ParentRelation | null>(null);
  const [hover, setHover] = useState<string | null>(null);
  // Default: the first edge type in append order, read from → to.
  const relation: ParentRelation =
    chosen !== null && edgeTypes.includes(chosen.ty_name)
      ? chosen
      : { ty_name: edgeTypes[0] ?? "", direction: "outgoing" };
  const byKey = new Map(nodes.map((node) => [entryKey(node.id), node]));
  const thread = buildThread(nodes, edges, relation);
  const occurrences = new Map<string, number>();
  for (const row of thread.rows) {
    occurrences.set(row.key, (occurrences.get(row.key) ?? 0) + 1);
  }
  const seen = new Map<string, number>();
  return (
    <section>
      <div className="row" data-testid="thread-controls">
        <label>
          parent → child by{" "}
          <select
            data-testid="thread-relation"
            value={relation.ty_name}
            disabled={edgeTypes.length === 0}
            onChange={(event) =>
              setChosen({ ...relation, ty_name: event.target.value })
            }
          >
            {edgeTypes.length === 0 && <option value="">no edges</option>}
            {edgeTypes.map((name) => (
              <option key={name} value={name}>
                {name}
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
            </li>
          );
        })}
      </ol>
    </section>
  );
}
