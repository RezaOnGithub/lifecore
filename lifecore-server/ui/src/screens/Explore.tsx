import {
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
  useState,
} from "react";
import type {
  EdgeRecord,
  NodeRecord,
  Provenance,
  ViewQuery,
} from "../contract.ts";
import {
  latest,
  schemaOf,
  useAsOf,
  useCall,
  usePlaces,
  useTypes,
} from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { IdLink, NodeLink, TypeLink, useOpen } from "../links.tsx";
import { compareStrings } from "../model/columns.ts";
import { entryKey, rowLabel, sameId, typeName } from "../model/labels.ts";
import { truncate } from "../model/payload.ts";
import {
  type ExploreAction,
  type ExploreForm,
  type ExploreState,
  formChanged,
  querySummary,
  toParams,
} from "../model/query.ts";
import { EntryTable, type ExtraColumn } from "../views/EntryTable.tsx";
import {
  EdgesFieldset,
  EdgeTypeList,
  ExpandFieldset,
  SeedsFieldset,
} from "../views/ExploreForm.tsx";
import { GraphView } from "../views/Graph.tsx";
import { relationColumns } from "../views/Relationships.tsx";
import { ThreadView } from "../views/Thread.tsx";

type QueriedRow = NodeRecord & { provenance: Provenance };

// How a node was reached: from the provenance node over the edge, drawn in
// the edge's own direction, `—PartOf→` when the edge goes from the
// provenance node to this one and `←PartOf—` when it comes from this one.
// The edge may be absent from the result's edges (the edge filter is
// separate); then only its identity is known.
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
  const forward = edge !== undefined && sameId(edge.from, provenance.from);
  return (
    <span className="via">
      <span className="hop" title={`reached at hop ${provenance.hop}`}>
        {provenance.hop}
      </span>{" "}
      from{" "}
      {from ? (
        <NodeLink node={from} position={false} />
      ) : (
        <IdLink id={provenance.from} />
      )}{" "}
      {edge === undefined ? (
        <>
          via <IdLink id={provenance.via} />
        </>
      ) : (
        <EdgeArrow edge={edge} forward={forward} />
      )}
    </span>
  );
}

// The traversed edge, drawn in its own direction and linking to its
// inspector.
function EdgeArrow({ edge, forward }: { edge: EdgeRecord; forward: boolean }) {
  const open = useOpen();
  const name = typeName(edge.ty_name);
  return (
    <button
      type="button"
      className="link via-edge"
      title={`edge @${edge.id.position} ${name}, ${
        forward ? "from that node to this one" : "from this node to that one"
      }`}
      onClick={() => open({ kind: "inspector", id: edge.id })}
    >
      {forward ? `—${name}→` : `←${name}—`}
    </button>
  );
}

// "focus" makes the node the only seed; "+ seed" adds it. Both run.
function RowActions({
  node,
  form,
  dispatch,
}: {
  node: NodeRecord;
  form: ExploreForm;
  dispatch: (action: ExploreAction) => void;
}) {
  const seeded = form.ids.some((id) => sameId(id, node.id));
  return (
    <span className="row-actions">
      <button
        type="button"
        className="mini"
        title="explore from this node alone, with the same expansion"
        onClick={() => dispatch({ kind: "focus", id: node.id })}
      >
        focus
      </button>
      <button
        type="button"
        className="mini"
        disabled={seeded}
        title={seeded ? "already a seed" : "add this node to the seeds"}
        onClick={() => dispatch({ kind: "seed", id: node.id })}
      >
        + seed
      </button>
    </span>
  );
}

function GroupedTables({
  result,
  relationships,
  actions,
}: {
  result: ViewQuery;
  relationships: boolean;
  actions: (node: NodeRecord) => ReactNode;
}) {
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
  const lead: ExtraColumn<QueriedRow>[] = [
    { key: "actions", header: "", cell: actions, className: "actions" },
  ];
  const extra: ExtraColumn<QueriedRow>[] = [
    {
      key: "via",
      header: "via",
      cell: (row) => (
        <Via provenance={row.provenance} edges={edges} nodes={nodes} />
      ),
      className: "via-cell",
      sortValue: (row) => ({
        kind: "integer",
        value: String(row.provenance.kind === "seed" ? 0 : row.provenance.hop),
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
              lead={lead}
              // The relationships are those among the result's edges, which
              // the edge filter selects.
              extra={
                relationships
                  ? [...extra, ...relationColumns(rows, result.edges, nodes)]
                  : extra
              }
            />
          </section>
        );
      })}
    </>
  );
}

// A name for the form about to be saved: its seeds as the result labels
// them when the form is the one on show, else as positions and selections.
function suggestedName(form: ExploreForm, result: ViewQuery | null): string {
  const labels = new Map(
    (result?.nodes ?? []).map(({ node }) => [entryKey(node.id), node]),
  );
  const parts = form.ids.map((id) => {
    const node = labels.get(entryKey(id));
    return node === undefined ? `@${id.position}` : rowLabel(node);
  });
  if (form.nodes.enabled) {
    const text = form.nodes.text.trim();
    parts.push(
      `${form.nodes.ty_name === "" ? "any type" : typeName(form.nodes.ty_name)}${
        text === "" ? "" : ` "${text}"`
      }`,
    );
  }
  return truncate(parts.join(" + "), 40);
}

function SaveView({
  form,
  result,
}: {
  form: ExploreForm;
  result: ViewQuery | null;
}) {
  const { places, saveView } = usePlaces();
  const [name, setName] = useState<string | null>(null);
  if (name === null) {
    return (
      <button
        type="button"
        disabled={toParams(form) === null}
        title="keep this form in the sidebar under a name"
        onClick={() => setName(suggestedName(form, result))}
      >
        Save view
      </button>
    );
  }
  const trimmed = name.trim();
  const exists = places.views.some((view) => view.name === trimmed);
  const save = () => {
    if (trimmed !== "") {
      saveView(trimmed, form);
      setName(null);
    }
  };
  // Enter saves rather than submitting the Explore form around it.
  const onKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      save();
    } else if (event.key === "Escape") {
      setName(null);
    }
  };
  return (
    <span className="save-view">
      <input
        aria-label="view name"
        value={name}
        size={24}
        // biome-ignore lint/a11y/noAutofocus: the field opens on request
        autoFocus
        onChange={(event) => setName(event.target.value)}
        onKeyDown={onKey}
      />
      <button type="button" disabled={trimmed === ""} onClick={save}>
        {exists ? "Replace" : "Save"}
      </button>
      <button type="button" onClick={() => setName(null)}>
        Cancel
      </button>
    </span>
  );
}

type Display = "table" | "thread" | "graph";

export function ExploreScreen({
  state,
  dispatch,
}: {
  state: ExploreState;
  dispatch: (action: ExploreAction) => void;
}) {
  const asOf = useAsOf();
  const [display, setDisplay] = useState<Display>("table");
  const [relationships, setRelationships] = useState(true);
  const [unfolded, setUnfolded] = useState(true);
  const { form, submitted } = state;
  const params = submitted === null ? null : { ...asOf, ...submitted.params };
  const load = useCall("views.query", params, submitted?.run ?? 0);
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
  const actions = (node: NodeRecord) => (
    <RowActions node={node} form={form} dispatch={dispatch} />
  );
  const displays: [Display, string][] = [
    ["table", "Table"],
    ["thread", "Thread"],
    ["graph", "Graph"],
  ];
  return (
    <section>
      <h2>
        Explore{" "}
        <button
          type="button"
          className="link small"
          aria-expanded={unfolded}
          onClick={() => setUnfolded(!unfolded)}
        >
          {unfolded ? "fold the form" : "unfold the form"}
        </button>
      </h2>
      <form className="explore" onSubmit={run}>
        {/* Folded, the form leaves the results the room; it stays mounted
            so nothing typed is lost. */}
        <div hidden={!unfolded}>
          <SeedsFieldset form={form} onChange={onChange} />
          <ExpandFieldset form={form} onChange={onChange} />
          <EdgesFieldset form={form} onChange={onChange} />
        </div>
        <EdgeTypeList />
        {!unfolded && (
          <p className="muted query-summary" data-testid="query-summary">
            {querySummary(form)}
          </p>
        )}
        <div className="row">
          <button type="submit" disabled={query === null}>
            Run
          </button>
          <SaveView form={form} result={result} />
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
              {displays.map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  className={display === value ? "on" : undefined}
                  onClick={() => setDisplay(value)}
                >
                  {label}
                </button>
              ))}
            </span>
            {display === "table" && (
              <label title="a column per edge type and direction among the result's edges">
                <input
                  type="checkbox"
                  checked={relationships}
                  onChange={(event) => setRelationships(event.target.checked)}
                />{" "}
                relationships
              </label>
            )}
            <span className="muted">view [0, {result.view.end_exclusive})</span>
          </div>
          {display === "table" && (
            <GroupedTables
              result={result}
              relationships={relationships}
              actions={actions}
            />
          )}
          {display === "thread" && (
            <ThreadView
              nodes={result.nodes.map((queried) => queried.node)}
              edges={result.edges}
              actions={actions}
            />
          )}
          {/* Mounted throughout, so a drawing survives a look at the other
              displays; it is requested only by its Render button. */}
          <div hidden={display !== "graph"}>
            <GraphView
              params={params}
              focus={result.nodes
                .filter(({ provenance }) => provenance.kind === "seed")
                .map(({ node }) => node.id.position)}
            />
          </div>
        </section>
      )}
    </section>
  );
}
