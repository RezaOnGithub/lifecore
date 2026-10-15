import { type FormEvent, useState } from "react";
import type {
  EdgeRecord,
  NodeRecord,
  Provenance,
  ViewQuery,
} from "../contract.ts";
import { latest, schemaOf, useAsOf, useCall, useTypes } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { IdLink, NodeLink, TypeLink } from "../links.tsx";
import { compareStrings } from "../model/columns.ts";
import { entryKey } from "../model/labels.ts";
import {
  type ExploreAction,
  type ExploreForm,
  type ExploreState,
  formChanged,
  toParams,
} from "../model/query.ts";
import { EntryTable, type ExtraColumn } from "../views/EntryTable.tsx";
import {
  EdgesFieldset,
  EdgeTypeList,
  ExpandFieldset,
  SeedsFieldset,
} from "../views/ExploreForm.tsx";
import { ThreadView } from "../views/Thread.tsx";

type QueriedRow = NodeRecord & { provenance: Provenance };

// The edge a node was reached over may be absent from the result's edges
// (the edge filter is separate), so the identity is always a link and the
// type name is shown when known.
function Via({
  provenance,
  edges,
  nodes,
}: {
  provenance: Provenance;
  edges: ReadonlyMap<string, EdgeRecord>;
  nodes: ReadonlyMap<string, NodeRecord>;
}) {
  if (provenance.kind === "seed") {
    return <span>seed</span>;
  }
  const edge = edges.get(entryKey(provenance.via));
  const from = nodes.get(entryKey(provenance.from));
  return (
    <span>
      hop {provenance.hop} ← {edge ? edge.ty_name : "edge"}{" "}
      <IdLink id={provenance.via} /> from{" "}
      {from ? <NodeLink node={from} /> : <IdLink id={provenance.from} />}
    </span>
  );
}

function GroupedTables({ result }: { result: ViewQuery }) {
  const types = useTypes();
  const groups = new Map<string, QueriedRow[]>();
  const nodes = new Map<string, NodeRecord>();
  for (const { node, provenance } of result.nodes) {
    nodes.set(entryKey(node.id), node);
    const rows = groups.get(node.ty_name) ?? [];
    rows.push({ ...node, provenance });
    groups.set(node.ty_name, rows);
  }
  const edges = new Map(result.edges.map((edge) => [entryKey(edge.id), edge]));
  const incident = new Map<string, number>();
  for (const edge of result.edges) {
    for (const end of [edge.from, edge.to]) {
      const key = entryKey(end);
      incident.set(key, (incident.get(key) ?? 0) + 1);
    }
  }
  const extra: ExtraColumn<QueriedRow>[] = [
    {
      key: "via",
      header: "via",
      cell: (row) => (
        <Via provenance={row.provenance} edges={edges} nodes={nodes} />
      ),
      sortValue: (row) => ({
        kind: "integer",
        value: String(row.provenance.kind === "seed" ? 0 : row.provenance.hop),
      }),
    },
    {
      key: "edges",
      header: (
        <span title="incident edges among the result's edges, which the edge filter selects">
          edges
        </span>
      ),
      cell: (row) => incident.get(entryKey(row.id)) ?? 0,
      sortValue: (row) => ({
        kind: "integer",
        value: String(incident.get(entryKey(row.id)) ?? 0),
      }),
    },
  ];
  return (
    <>
      {[...groups.keys()].sort(compareStrings).map((ty_name) => {
        const rows = groups.get(ty_name) ?? [];
        return (
          <section key={ty_name}>
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
              extra={extra}
            />
          </section>
        );
      })}
    </>
  );
}

export function ExploreScreen({
  state,
  dispatch,
}: {
  state: ExploreState;
  dispatch: (action: ExploreAction) => void;
}) {
  const asOf = useAsOf();
  const [display, setDisplay] = useState<"table" | "thread">("table");
  const { form, submitted } = state;
  const load = useCall(
    "views.query",
    submitted === null ? null : { ...asOf, ...submitted.params },
    submitted?.run ?? 0,
  );
  // A rerun keeps the last result, and with it the display and thread
  // choices, on screen until the new one arrives.
  const result = latest(load);
  const query = toParams(form);
  const onChange = (changed: ExploreForm) =>
    dispatch({ kind: "change", form: changed });
  const run = (event: FormEvent) => {
    event.preventDefault();
    dispatch({ kind: "run" });
  };
  return (
    <section>
      <h2>Explore</h2>
      <form className="explore" onSubmit={run}>
        <SeedsFieldset form={form} onChange={onChange} />
        <ExpandFieldset form={form} onChange={onChange} />
        <EdgesFieldset form={form} onChange={onChange} />
        <EdgeTypeList />
        <div className="row">
          <button type="submit" disabled={query === null}>
            Run
          </button>
          {query === null && <span className="muted">add a seed to run</span>}
          {query !== null && formChanged(state) && (
            <span className="muted" data-testid="form-changed">
              form changed; Run to update
            </span>
          )}
        </div>
      </form>
      {load.status === "loading" && (
        <p className="muted">{result === null ? "running…" : "rerunning…"}</p>
      )}
      {load.status === "failed" && <FailureView failure={load.failure} />}
      {result !== null && (
        <section data-testid="results">
          <div className="row">
            <strong>
              {result.nodes.length} node
              {result.nodes.length === 1 ? "" : "s"}, {result.edges.length} edge
              {result.edges.length === 1 ? "" : "s"}
            </strong>
            <span className="toggle">
              <button
                type="button"
                className={display === "table" ? "on" : undefined}
                onClick={() => setDisplay("table")}
              >
                Table
              </button>
              <button
                type="button"
                className={display === "thread" ? "on" : undefined}
                onClick={() => setDisplay("thread")}
              >
                Thread
              </button>
            </span>
            <span className="muted">view [0, {result.view.end_exclusive})</span>
          </div>
          {display === "table" ? (
            <GroupedTables result={result} />
          ) : (
            <ThreadView
              nodes={result.nodes.map((queried) => queried.node)}
              edges={result.edges}
            />
          )}
        </section>
      )}
    </section>
  );
}
