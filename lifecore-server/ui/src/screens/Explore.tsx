import {
  type FormEvent,
  type KeyboardEvent,
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type {
  Currentness,
  EdgeRecord,
  EntryId,
  NodeRecord,
  Provenance,
  ViewQuery,
} from "../contract.ts";
import {
  latest,
  schemaOf,
  useAsOf,
  useCall,
  useData,
  useLabels,
  usePlaces,
  useTypes,
} from "../data.tsx";
import { FailureView } from "../failure.tsx";
import {
  type Edit,
  IdLink,
  NodeLink,
  type Open,
  TypeLink,
  useEdit,
  useOpen,
} from "../links.tsx";
import { compareStrings } from "../model/columns.ts";
import { entryKey, rowLabel, typeName, viewWords } from "../model/labels.ts";
import { truncate } from "../model/payload.ts";
import {
  type ExploreAction,
  type ExploreForm,
  type ExploreState,
  type ExploreStep,
  exploreStep,
  formChanged,
  querySummary,
  seedName,
  toParams,
} from "../model/query.ts";
import { directionAt, edgeWords, relationLabel } from "../model/relations.ts";
import type { ParentRelation } from "../model/thread.ts";
import {
  type Display,
  defaultTableView,
  type ExploreScreen as ExploreStepScreen,
  type ExploreView,
  type TableView,
} from "../navigation.ts";
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

// How a node was reached: at a hop, over an edge read from this node
// (`→ PartOf` when the edge goes from it, `← PartOf` when it comes to it),
// from the node it was reached from. The edge may be absent from the
// result's edges (the edge filter is separate); then only its identity is
// known.
function Via({
  row,
  edges,
  nodes,
}: {
  row: QueriedRow;
  edges: ReadonlyMap<string, EdgeRecord>;
  nodes: ReadonlyMap<string, NodeRecord>;
}) {
  const { provenance } = row;
  if (provenance.kind === "seed") {
    return <span>seed</span>;
  }
  const edge = edges.get(entryKey(provenance.via));
  const from = nodes.get(entryKey(provenance.from));
  return (
    <span className="via">
      <span className="hop" title={`reached at hop ${provenance.hop}`}>
        {provenance.hop}
      </span>{" "}
      via{" "}
      {edge === undefined ? (
        <IdLink id={provenance.via} />
      ) : (
        <EdgeArrow edge={edge} row={row} from={from} />
      )}{" "}
      from{" "}
      {from ? (
        <NodeLink node={from} position={false} />
      ) : (
        <IdLink id={provenance.from} />
      )}
    </span>
  );
}

// The traversed edge in the one arrow notation, read from the row's node,
// linking to its inspector.
function EdgeArrow({
  edge,
  row,
  from,
}: {
  edge: EdgeRecord;
  row: NodeRecord;
  from: NodeRecord | undefined;
}) {
  const open = useOpen();
  const direction = directionAt(edge, row.id);
  const other = from === undefined ? `@${edge.from.position}` : rowLabel(from);
  const words =
    direction === "outgoing"
      ? edgeWords(rowLabel(row), edge.ty_name, other)
      : edgeWords(other, edge.ty_name, rowLabel(row));
  return (
    <button
      type="button"
      className="link via-edge"
      title={`edge @${edge.id.position}: ${words}`}
      onClick={() => open({ kind: "inspector", id: edge.id })}
    >
      {relationLabel({ ty_name: edge.ty_name, direction })}
    </button>
  );
}

// "focus" makes the node the only seed; "+ seed" adds it. Both run.
function RowActions({
  node,
  seeded,
  act,
}: {
  node: NodeRecord;
  seeded: boolean;
  act: (action: ExploreAction) => void;
}) {
  return (
    <span className="row-actions">
      <button
        type="button"
        className="mini"
        title="explore from this node alone, with the same expansion"
        onClick={() => act({ kind: "focus", id: node.id })}
      >
        focus
      </button>
      <button
        type="button"
        className="mini"
        disabled={seeded}
        title={seeded ? "already a seed" : "add this node to the seeds"}
        onClick={() => act({ kind: "seed", id: node.id })}
      >
        + seed
      </button>
    </span>
  );
}

// What the result tables share, derived once per result.
interface Shaped {
  groups: [string, QueriedRow[]][];
  nodes: Map<string, NodeRecord>;
  edges: Map<string, EdgeRecord>;
}

function shape(result: ViewQuery): Shaped {
  const groups = new Map<string, QueriedRow[]>();
  const nodes = new Map<string, NodeRecord>();
  for (const { node, provenance } of result.nodes) {
    nodes.set(entryKey(node.id), node);
    const rows = groups.get(node.ty_name) ?? [];
    rows.push({ ...node, provenance });
    groups.set(node.ty_name, rows);
  }
  return {
    groups: [...groups].sort(([a], [b]) => compareStrings(a, b)),
    nodes,
    edges: new Map(result.edges.map((edge) => [entryKey(edge.id), edge])),
  };
}

const TypeTable = memo(function TypeTable({
  ty_name,
  rows,
  shaped,
  result,
  relationships,
  currentness,
  lead,
  view,
  onView,
}: {
  ty_name: string;
  rows: QueriedRow[];
  shaped: Shaped;
  result: ViewQuery;
  relationships: boolean;
  currentness: Currentness;
  lead: ExtraColumn<QueriedRow>[];
  view: TableView;
  onView: (ty_name: string, update: (view: TableView) => TableView) => void;
}) {
  const types = useTypes();
  const extra = useMemo(() => {
    const via: ExtraColumn<QueriedRow> = {
      key: "via",
      header: "via",
      cell: (row) => (
        <Via row={row} edges={shaped.edges} nodes={shaped.nodes} />
      ),
      className: "via-cell",
      sortValue: (row) => ({
        kind: "integer",
        value: String(row.provenance.kind === "seed" ? 0 : row.provenance.hop),
      }),
    };
    // The relationships are those among the result's edges, which the edge
    // filter selects.
    return relationships
      ? [via, ...relationColumns(rows, result.edges, shaped.nodes, currentness)]
      : [via];
  }, [rows, shaped, result, relationships, currentness]);
  const onTable = useCallback(
    (update: (view: TableView) => TableView) => onView(ty_name, update),
    [onView, ty_name],
  );
  return (
    <section>
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
        extra={extra}
        showCurrent={rows.some((row) => !row.current)}
        view={view}
        onView={onTable}
      />
    </section>
  );
});

// A name for the form about to be saved: its seeds by label where known,
// else as positions and selections.
function suggestedName(
  form: ExploreForm,
  label: (id: EntryId) => string | undefined,
): string {
  const parts = form.ids.map((id) => label(id) ?? `@${id.position}`);
  if (form.nodes.enabled) {
    const text = form.nodes.text.trim();
    parts.push(
      `${form.nodes.ty_name === null ? "any type" : typeName(form.nodes.ty_name)}${
        text === "" ? "" : ` "${text}"`
      }`,
    );
  }
  return truncate(parts.join(" + "), 40);
}

function SaveView({
  form,
  label,
}: {
  form: ExploreForm;
  label: (id: EntryId) => string | undefined;
}) {
  const { places, saveView } = usePlaces();
  const [name, setName] = useState<string | null>(null);
  if (name === null) {
    return (
      <button
        type="button"
        disabled={toParams(form) === null}
        title="keep this form in the sidebar under a name"
        onClick={() => setName(suggestedName(form, label))}
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

// Puts an Explore step into the history: a new query is a new step, and the
// step it leaves shows its own query again.
export function applyExploreStep(
  step: ExploreStep,
  view: ExploreView,
  open: Open,
  edit: Edit,
) {
  switch (step.kind) {
    case "none":
      return;
    case "replace":
      edit((screen) =>
        screen.kind === "explore" ? { ...screen, ...step.state } : screen,
      );
      return;
    case "push":
      open(
        { kind: "explore", ...step.state, view },
        { leaving: { kind: "explore", ...step.leaving, view } },
      );
  }
}

const DISPLAYS: [Display, string][] = [
  ["table", "Table"],
  ["thread", "Thread"],
  ["graph", "Graph"],
];

// Explore shows one Back step: its form, the query it ran, and its view.
// It stays mounted under other screens, showing the last Explore step, so
// its results and drawings survive a detour; `useEdit` changes the step
// only while it is the current one. A run, focus, or + seed of another
// query is a new step. While `hidden` it reads nothing.
export const ExploreScreen = memo(function ExploreScreen({
  screen,
  hidden,
}: {
  screen: ExploreStepScreen;
  hidden: boolean;
}) {
  const edit = useEdit();
  const open = useOpen();
  const asOf = useAsOf();
  const { asOf: boundary } = useData();
  const book = useLabels();
  const { form, ran, view } = screen;
  // Actions read the step through a ref, so the row actions keep their
  // identity and the result tables do not render again for them.
  const latestScreen = useRef(screen);
  latestScreen.current = screen;
  const act = useCallback(
    (action: ExploreAction) => {
      const current = latestScreen.current;
      const state: ExploreState = { form: current.form, ran: current.ran };
      applyExploreStep(exploreStep(state, action), current.view, open, edit);
    },
    [edit, open],
  );
  const setView = useCallback(
    (update: (view: ExploreView) => ExploreView) =>
      edit((screen) =>
        screen.kind === "explore"
          ? { ...screen, view: update(screen.view) }
          : screen,
      ),
    [edit],
  );
  const query = ran === null ? null : toParams(ran.form);
  const params = query === null || hidden ? null : { ...asOf, ...query };
  const live = useCall("views.query", params, ran?.run ?? 0);
  // Hidden, Explore keeps what it showed when it was left, so its results
  // and drawings stay mounted, and asks again on return, at the boundary
  // then selected.
  const held = useRef(live);
  if (!hidden) {
    held.current = live;
  }
  const load = hidden ? held.current : live;
  // A rerun keeps the last result, and with it the display and thread
  // choices, on screen until the new one arrives.
  const result = latest(load);
  const shaped = useMemo(
    () => (result === null ? null : shape(result)),
    [result],
  );
  useEffect(() => {
    if (result !== null) {
      book.noteNodes(result.nodes.map(({ node }) => node));
    }
  }, [book, result]);
  const label = useCallback(
    (id: EntryId) => {
      const node = shaped?.nodes.get(entryKey(id));
      return node === undefined ? book.get(id, boundary) : rowLabel(node);
    },
    [shaped, book, boundary],
  );
  const onChange = (changed: ExploreForm) =>
    act({ kind: "change", form: changed });
  const run = (event: FormEvent) => {
    event.preventDefault();
    act({ kind: "run" });
  };
  const seeded = useMemo(() => new Set(form.ids.map(entryKey)), [form.ids]);
  const lead = useMemo<ExtraColumn<QueriedRow>[]>(
    () => [
      {
        key: "actions",
        header: "",
        cell: (row) => (
          <RowActions
            node={row}
            seeded={seeded.has(entryKey(row.id))}
            act={act}
          />
        ),
        className: "actions",
      },
    ],
    [seeded, act],
  );
  const threadActions = useCallback(
    (node: NodeRecord) => (
      <RowActions
        node={node}
        seeded={seeded.has(entryKey(node.id))}
        act={act}
      />
    ),
    [seeded, act],
  );
  const onTable = useCallback(
    (ty_name: string, update: (view: TableView) => TableView) =>
      setView((current) => ({
        ...current,
        tables: {
          ...current.tables,
          [ty_name]: update(current.tables[ty_name] ?? defaultTableView),
        },
      })),
    [setView],
  );
  const onThread = useCallback(
    (thread: ParentRelation) => setView((current) => ({ ...current, thread })),
    [setView],
  );
  const threadNodes = useMemo(
    () => result?.nodes.map(({ node }) => node) ?? [],
    [result],
  );
  const seeds = useMemo(
    () =>
      result?.nodes
        .filter(({ provenance }) => provenance.kind === "seed")
        .map(({ node }) => node.id.position) ?? [],
    [result],
  );
  // The drawing is of the result on show: its query at its own view.
  const renderParams = useMemo(() => {
    const drawn = ran === null ? null : toParams(ran.form);
    return drawn === null || result === null
      ? null
      : { ...drawn, as_of: result.view.end_exclusive };
  }, [ran, result]);
  const runnable = toParams(form) !== null;
  const changed = formChanged({ form, ran });
  return (
    <section data-testid="explore">
      <h2>
        Explore{" "}
        <button
          type="button"
          className="link small"
          aria-expanded={view.unfolded}
          onClick={() =>
            setView((current) => ({ ...current, unfolded: !current.unfolded }))
          }
        >
          {view.unfolded ? "fold the form" : "unfold the form"}
        </button>
      </h2>
      <form className="explore" onSubmit={run}>
        {/* Folded, the form leaves the results the room; it stays mounted
            so nothing typed is lost. */}
        <div hidden={!view.unfolded}>
          <SeedsFieldset form={form} onChange={onChange} label={label} />
          <ExpandFieldset form={form} onChange={onChange} />
          <EdgesFieldset form={form} onChange={onChange} />
        </div>
        <EdgeTypeList />
        {!view.unfolded && (
          <p className="muted query-summary" data-testid="query-summary">
            {querySummary(form, label)}
          </p>
        )}
        <div className="row">
          <button type="submit" disabled={!runnable}>
            Run
          </button>
          <SaveView form={form} label={label} />
          {!runnable && <span className="muted">add a seed to run</span>}
          {runnable && changed && (
            <span className="muted" data-testid="form-changed">
              form changed; Run to update
            </span>
          )}
        </div>
      </form>
      {load.status === "loading" && (
        <p className="muted">{result === null ? "running…" : "reloading…"}</p>
      )}
      {load.status === "failed" && <FailureView failure={load.failure} />}
      {result !== null && shaped !== null && (
        <section data-testid="results">
          <div className="row">
            <strong>
              {result.nodes.length} node
              {result.nodes.length === 1 ? "" : "s"}, {result.edges.length} edge
              {result.edges.length === 1 ? "" : "s"}
            </strong>
            <span className="toggle">
              {DISPLAYS.map(([value, name]) => (
                <button
                  key={value}
                  type="button"
                  className={view.display === value ? "on" : undefined}
                  onClick={() =>
                    setView((current) => ({ ...current, display: value }))
                  }
                >
                  {name}
                </button>
              ))}
            </span>
            {view.display === "table" && (
              <label title="a column per edge type and direction among the result's edges">
                <input
                  type="checkbox"
                  checked={view.relationships}
                  onChange={(event) => {
                    const relationships = event.target.checked;
                    setView((current) => ({ ...current, relationships }));
                  }}
                />{" "}
                relationships
              </label>
            )}
            <span
              className="muted"
              title={`read as ${viewWords(result.view.end_exclusive)}`}
            >
              view [0, {result.view.end_exclusive})
            </span>
            {ran !== null && ran.form.ids.length > 0 && (
              <span className="muted" title="the seeds of the query on show">
                from {ran.form.ids.map((id) => seedName(id, label)).join(", ")}
              </span>
            )}
          </div>
          {view.display === "table" &&
            shaped.groups.map(([ty_name, rows]) => (
              <TypeTable
                key={ty_name}
                ty_name={ty_name}
                rows={rows}
                shaped={shaped}
                result={result}
                relationships={view.relationships}
                currentness={ran?.form.edges.currentness ?? "current"}
                lead={lead}
                view={view.tables[ty_name] ?? defaultTableView}
                onView={onTable}
              />
            ))}
          {view.display === "thread" && (
            <ThreadView
              nodes={threadNodes}
              edges={result.edges}
              actions={threadActions}
              chosen={view.thread}
              onChoose={onThread}
            />
          )}
          {/* Mounted throughout, so a drawing survives a look at the other
              displays; it is requested only by its Render button. */}
          <div hidden={view.display !== "graph"}>
            <GraphView
              params={renderParams}
              seeds={seeds}
              nodes={result.nodes.length}
              edges={result.edges.length}
            />
          </div>
        </section>
      )}
    </section>
  );
});
