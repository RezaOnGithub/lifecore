import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  Currentness,
  NodeRecord,
  Seed,
  TypeSummary,
  ViewQueryParams,
} from "../contract.ts";
import {
  latest,
  schemaOf,
  useAsOf,
  useCall,
  useLabels,
  useTypes,
} from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { frozen } from "../draft/model.ts";
import { FailureView } from "../failure.tsx";
import { IdLink, useEdit, useOpen } from "../links.tsx";
import { entryKey, typeName } from "../model/labels.ts";
import {
  defaultTableScreenView,
  type TableScreenView,
  type TableView,
} from "../navigation.ts";
import { EntryTable } from "../views/EntryTable.tsx";
import { relationColumns } from "../views/Relationships.tsx";
import { CurrentnessSelect, ResolutionBadge } from "../views/widgets.tsx";

// Nothing until the type list has loaded: an unknown name is only known to
// be unknown once the list is there.
function ResolutionNote({
  types,
  ty_name,
}: {
  types: TypeSummary[] | null;
  ty_name: string;
}) {
  if (types === null) {
    return null;
  }
  const summary = types.find((type) => type.name === ty_name);
  if (summary === undefined) {
    return <p className="muted">This type name is unknown to the view.</p>;
  }
  const { resolution } = summary;
  switch (resolution.kind) {
    case "builtin":
      return (
        <p className="muted">The built-in metatype; rows are declarations.</p>
      );
    case "declared":
      return (
        <p className="muted">
          Declared by <IdLink id={resolution.declaration} />.
        </p>
      );
    case "missing":
      return (
        <p className="muted">
          <ResolutionBadge resolution={resolution} /> No current declaration;
          nodes use the name anyway.
        </p>
      );
    case "ambiguous":
      return (
        <p className="muted">
          <ResolutionBadge resolution={resolution} />{" "}
          {resolution.declarations.length} current declarations:{" "}
          {resolution.declarations.map((id, index) => (
            <span key={id.position}>
              {index > 0 && ", "}
              <IdLink id={id} />
            </span>
          ))}
        </p>
      );
    case "malformed":
      return (
        <p className="muted">
          <ResolutionBadge resolution={resolution} /> Declaration{" "}
          <IdLink id={resolution.declaration} /> does not parse:{" "}
          {resolution.message}
        </p>
      );
  }
}

export function TableScreen({
  ty_name,
  view,
}: {
  ty_name: string;
  view: TableScreenView;
}) {
  const asOf = useAsOf();
  const types = useTypes();
  const book = useLabels();
  const { state, actions } = useDraft();
  const open = useOpen();
  const edit = useEdit();
  const setView = useCallback(
    (update: (view: TableScreenView) => TableScreenView) =>
      edit((screen) =>
        screen.kind === "table"
          ? { ...screen, view: update(screen.view ?? defaultTableScreenView) }
          : screen,
      ),
    [edit],
  );
  const onTable = useCallback(
    (update: (view: TableView) => TableView) =>
      setView((current) => ({ ...current, table: update(current.table) })),
    [setView],
  );
  const { currentness, text, relationships } = view;
  // The filter goes to the backend; a short pause keeps typing from sending
  // a request per keystroke. A step returned to applies its text at once.
  const [applied, setApplied] = useState(text.trim());
  useEffect(() => {
    const timer = setTimeout(() => setApplied(text.trim()), 300);
    return () => clearTimeout(timer);
  }, [text]);
  const seed: Seed = { kind: "nodes", ty_name, currentness };
  if (applied !== "") {
    seed.text = applied;
  }
  // The rows are the seeds; one hop both ways brings their neighbours, and
  // the edges among the result include every edge incident to a row. The
  // edges followed match the rows: current rows meet current edges, while a
  // superseded row's edges are all historical.
  const followed: Currentness = currentness === "current" ? "current" : "all";
  const params: ViewQueryParams = {
    ...asOf,
    seeds: [seed],
    expand: [{ hops: 1, direction: "both", currentness: followed }],
    edges: { currentness: followed },
  };
  const load = useCall("views.query", params);
  // A reload keeps the last rows, and the sort, on screen until the new ones
  // arrive.
  const result = latest(load);
  const { rows, nodes } = useMemo(() => {
    const rows: NodeRecord[] = [];
    const nodes = new Map<string, NodeRecord>();
    for (const { node, provenance } of result?.nodes ?? []) {
      nodes.set(entryKey(node.id), node);
      if (provenance.kind === "seed") {
        rows.push(node);
      }
    }
    return { rows, nodes };
  }, [result]);
  useEffect(() => {
    book.noteNodes(nodes.values());
  }, [book, nodes]);
  const columns = useMemo(
    () =>
      result !== null && relationships
        ? relationColumns(rows, result.edges, nodes, followed)
        : [],
    [result, relationships, rows, nodes, followed],
  );
  return (
    <section data-testid="table">
      <h2>
        {typeName(ty_name)}{" "}
        <button
          type="button"
          disabled={frozen(state)}
          onClick={() => {
            const name = actions.addNode(ty_name);
            if (name !== null) {
              open({ kind: "operation", name });
            }
          }}
        >
          New node of this type
        </button>
      </h2>
      <ResolutionNote types={types} ty_name={ty_name} />
      <div className="row">
        <label htmlFor="table-currentness">show</label>
        <CurrentnessSelect
          id="table-currentness"
          value={currentness}
          onChange={(currentness) =>
            setView((current) => ({ ...current, currentness }))
          }
        />
        <label>
          containing{" "}
          <input
            type="search"
            value={text}
            onChange={(event) => {
              const text = event.target.value;
              setView((current) => ({ ...current, text }));
            }}
            placeholder="any text"
          />
        </label>
        <label title="a column per edge type and direction, listing each row's neighbours">
          <input
            type="checkbox"
            checked={relationships}
            onChange={(event) => {
              const relationships = event.target.checked;
              setView((current) => ({ ...current, relationships }));
            }}
          />{" "}
          relationships
        </label>
        {result !== null && (
          <>
            <span className="muted">
              {rows.length} row{rows.length === 1 ? "" : "s"}
            </span>
            <span className="muted">view [0, {result.view.end_exclusive})</span>
          </>
        )}
        {load.status === "loading" && (
          <span className="muted">
            {result === null ? "loading…" : "reloading…"}
          </span>
        )}
      </div>
      {load.status === "failed" && <FailureView failure={load.failure} />}
      {result !== null && (
        <EntryTable
          rows={rows}
          schema={schemaOf(types, ty_name)}
          ty_name={ty_name}
          extra={columns}
          showCurrent={currentness !== "current"}
          view={view.table}
          onView={onTable}
        />
      )}
    </section>
  );
}
