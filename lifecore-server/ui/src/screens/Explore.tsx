import {
  type FormEvent,
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
} from "react";
import type { EntryId, NodeRecord } from "../contract.ts";
import { latest, useAsOf, useCall, useData, useLabels } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { useEdit, useOpen } from "../links.tsx";
import { entryKey, rowLabel, viewWords } from "../model/labels.ts";
import {
  type ExploreAction,
  type ExploreForm,
  type ExploreState,
  exploreStep,
  formChanged,
  type QueriedRow,
  querySummary,
  seedName,
  shape,
  toParams,
} from "../model/query.ts";
import type { ParentRelation } from "../model/thread.ts";
import {
  applyExploreStep,
  type Display,
  defaultTableView,
  type ExploreScreen as ExploreStepScreen,
  type ExploreView,
  type TableView,
} from "../navigation.ts";
import type { ExtraColumn } from "../views/EntryTable.tsx";
import {
  EdgesFieldset,
  EdgeTypeList,
  ExpandFieldset,
  SeedsFieldset,
} from "../views/ExploreForm.tsx";
import { RowActions, TypeTable } from "../views/ExploreTable.tsx";
import { GraphView } from "../views/Graph.tsx";
import { SaveView } from "../views/SaveView.tsx";
import { ThreadView } from "../views/Thread.tsx";

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
