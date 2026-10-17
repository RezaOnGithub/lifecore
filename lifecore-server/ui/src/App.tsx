import {
  type FormEvent,
  memo,
  type RefObject,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type { DecimalString, Description, EntryId } from "./contract.ts";
import {
  DataContext,
  LabelBook,
  LabelsContext,
  type Load,
  latest,
  PlacesProvider,
  ResultCache,
  TypesContext,
  useAsOf,
  useCall,
  useData,
  useLabels,
  usePlaces,
} from "./data.tsx";
import { DraftProvider } from "./draft/context.tsx";
import { FailureView } from "./failure.tsx";
import {
  type Edit,
  EditContext,
  ExploreContext,
  NavigationContext,
  type Open,
  ReplaceContext,
  useOpen,
} from "./links.tsx";
import { pastBoundary, sameId, typeName, viewWords } from "./model/labels.ts";
import { truncate } from "./model/payload.ts";
import {
  type ExploreForm,
  exploreStep,
  fromHere,
  seedName,
} from "./model/query.ts";
import {
  defaultInspectorView,
  defaultTableScreenView,
  exploreScreen,
  initialNavigation,
  type Navigation,
  type NavigationAction,
  navigate,
  type Screen,
  type Step,
  sameScreen,
} from "./navigation.ts";
import type { RpcClient } from "./rpc.ts";
import { DatabaseScreen } from "./screens/Database.tsx";
import { applyExploreStep, ExploreScreen } from "./screens/Explore.tsx";
import { FindingsScreen } from "./screens/Findings.tsx";
import { InspectorScreen } from "./screens/Inspector.tsx";
import { OperationScreen } from "./screens/Operation.tsx";
import { TableScreen } from "./screens/Table.tsx";
import { ImportView } from "./views/Import.tsx";
import { Sidebar } from "./views/Sidebar.tsx";
import { Tray } from "./views/Tray.tsx";

export function App({ client }: { client: RpcClient }) {
  const [asOf, setAsOf] = useState<DecimalString | null>(null);
  const [refresh, setRefresh] = useState(0);
  const bumpRefresh = useCallback(() => setRefresh((count) => count + 1), []);
  // What was read of one database; another database's answers start afresh.
  const [{ cache, labels }, setScope] = useState(() => ({
    cache: new ResultCache(),
    labels: new LabelBook(),
  }));
  const known = useRef<string | null>(null);
  // The first database to answer adopts the scope its reads already filled.
  // Another one, as after an `init` or a restore under a running server,
  // replaces it, and the boundary, which may be past its end, is cleared.
  const onDatabase = useCallback((database: string) => {
    const last = known.current;
    known.current = database;
    if (last !== null && last !== database) {
      setScope({ cache: new ResultCache(), labels: new LabelBook() });
      setAsOf(null);
    }
  }, []);
  // A restore or re-init of the same database may leave the boundary past
  // its new end, where every read fails. Only a description's answer judges
  // it: a boundary set from a later read may well be past an older answer.
  const onHead = useCallback((head: DecimalString) => {
    setAsOf((asOf) =>
      asOf !== null && BigInt(head) < BigInt(asOf) ? null : asOf,
    );
  }, []);
  const source = useMemo(
    () => ({ client, asOf, setAsOf, refresh, bumpRefresh, cache }),
    [client, asOf, refresh, bumpRefresh, cache],
  );
  return (
    <DataContext value={source}>
      <LabelsContext value={labels}>
        <Shell
          asOf={asOf}
          onAsOf={setAsOf}
          onRefresh={bumpRefresh}
          onDatabase={onDatabase}
          onHead={onHead}
        />
      </LabelsContext>
    </DataContext>
  );
}

// The tray and the sidebar render again only when another screen is
// current, not when the current one changes what it remembers.
const StableTray = memo(Tray);

function useStableScreen(screen: Screen): Screen {
  const kept = useRef(screen);
  if (!sameScreen(kept.current, screen)) {
    kept.current = screen;
  }
  return kept.current;
}

// The main pane's scroll offset follows the steps: a step returned to by
// Back scrolls to where it was left, a new one starts at the top. When the
// content is not tall enough yet, as while a step's data loads, the offset
// waits for it until the user scrolls.
function useStepScroll(
  main: RefObject<HTMLElement | null>,
  content: RefObject<HTMLElement | null>,
  step: Step,
) {
  const pending = useRef<number | null>(null);
  const apply = useCallback(() => {
    const box = main.current;
    const target = pending.current;
    if (box === null || target === null) {
      return;
    }
    box.scrollTop = target;
    if (box.scrollHeight - box.clientHeight >= target) {
      pending.current = null;
    }
  }, [main]);
  // Keyed by the step, not only its offset: two steps left at the top
  // still differ.
  // biome-ignore lint/correctness/useExhaustiveDependencies: see above
  useLayoutEffect(() => {
    pending.current = step.scroll;
    apply();
  }, [step.id, step.scroll, apply]);
  useEffect(() => {
    const box = main.current;
    const inner = content.current;
    if (box === null || inner === null) {
      return;
    }
    const observer = new ResizeObserver(apply);
    observer.observe(inner);
    const cancel = () => {
      pending.current = null;
    };
    for (const kind of ["wheel", "pointerdown", "keydown", "touchstart"]) {
      box.addEventListener(kind, cancel, { passive: true });
    }
    return () => {
      observer.disconnect();
      for (const kind of ["wheel", "pointerdown", "keydown", "touchstart"]) {
        box.removeEventListener(kind, cancel);
      }
    };
  }, [main, content, apply]);
}

function Shell({
  asOf,
  onAsOf,
  onRefresh,
  onDatabase,
  onHead,
}: {
  asOf: DecimalString | null;
  onAsOf: (asOf: DecimalString | null) => void;
  onRefresh: () => void;
  onDatabase: (database: string) => void;
  onHead: (head: DecimalString) => void;
}) {
  const [nav, dispatch] = useReducer(navigate, initialNavigation);
  const latestNav = useRef(nav);
  latestNav.current = nav;
  const main = useRef<HTMLElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const open = useCallback<Open>((screen, options) => {
    const scroll = main.current?.scrollTop ?? 0;
    const leaving = options?.leaving;
    dispatch(
      leaving === undefined
        ? { kind: "open", screen, scroll }
        : { kind: "open", screen, scroll, leaving },
    );
  }, []);
  const replace = useCallback(
    (screen: Screen) => dispatch({ kind: "replace", screen }),
    [],
  );
  const step = nav.current;
  const screen = step.screen;
  const edit = useCallback<Edit>(
    (change) => dispatch({ kind: "edit", step: step.id, edit: change }),
    [step.id],
  );
  // Explore stays mounted, so its edits go to whichever step is current
  // when it is an Explore step; the function never changes, so a hidden
  // Explore does not render again when the current step does.
  const editExplore = useCallback<Edit>((change) => {
    const current = latestNav.current.current;
    if (current.screen.kind === "explore") {
      dispatch({ kind: "edit", step: current.id, edit: change });
    }
  }, []);
  // A query shown from outside Explore, or on it from the sidebar, goes
  // through the same steps as Explore's own Run.
  const explore = useCallback(
    (form: ExploreForm) => {
      const { current, explore: last } = latestNav.current;
      const shown = current.screen;
      if (shown.kind === "explore") {
        applyExploreStep(
          exploreStep(shown, { kind: "load", form }),
          shown.view,
          open,
          editExplore,
        );
      } else {
        // The last step's display choices carry over, but a drawing is of
        // one query and made on request, so a new query from elsewhere
        // shows as a table rather than as a stale drawing.
        const { display } = last.view;
        open(
          exploreScreen(form, {
            ...last.view,
            display: display === "graph" ? "table" : display,
          }),
        );
      }
    },
    [open, editExplore],
  );
  const exploreFrom = useCallback(
    (id: EntryId) => explore(fromHere(id)),
    [explore],
  );
  useStepScroll(main, content, step);
  const description = useCall("database.describe", {});
  const types = useCall("types.list", useAsOf());
  // The draft is per database: the provider mounts once the id is known and
  // remounts, restoring that database's draft, only when another database
  // answers. A failed or reloading description keeps the last id, so an
  // outage never empties the draft or orphans an apply in flight.
  const [database, setDatabase] = useState<string | null>(null);
  const described = latest(description);
  if (described !== null && described.database !== database) {
    setDatabase(described.database);
  }
  useEffect(() => {
    if (database !== null) {
      onDatabase(database);
    }
  }, [database, onDatabase]);
  const answered = description.status === "ready" ? description.result : null;
  useEffect(() => {
    if (answered !== null) {
      onHead(answered.view.end_exclusive);
    }
  }, [answered, onHead]);
  const head = described?.view.end_exclusive ?? null;
  const stable = useStableScreen(screen);
  const tray = <StableTray screen={stable} head={head} />;
  // Explore stays mounted, hidden under other screens and showing the last
  // Explore step, so its results survive a detour through the inspector
  // and Back. Other screens mount per step, so each starts from what its
  // step remembers. The operation screen is not keyed by step: a rename
  // must not remount the editor under the user's cursor.
  const shownExplore = screen.kind === "explore" ? screen : nav.explore;
  return (
    <NavigationContext value={open}>
      <ReplaceContext value={replace}>
        <ExploreContext value={explore}>
          <TypesContext value={latest(types)?.types ?? null}>
            <DraftProvider key={database ?? "unknown"} database={database}>
              <PlacesProvider key={database ?? "unknown"} database={database}>
                <div className="app">
                  <TopBar
                    description={description}
                    nav={nav}
                    dispatch={dispatch}
                    asOf={asOf}
                    onAsOf={onAsOf}
                    onRefresh={onRefresh}
                  />
                  <Trail nav={nav} dispatch={dispatch} />
                  {description.status === "failed" ? (
                    <>
                      <main className="full">
                        <FailureView failure={description.failure} />
                      </main>
                      {tray}
                    </>
                  ) : (
                    <>
                      <Sidebar
                        types={types}
                        screen={stable}
                        explore={nav.explore}
                      />
                      <main ref={main}>
                        <div ref={content}>
                          <EditContext value={edit}>
                            {screen.kind === "database" &&
                              (description.status === "ready" ? (
                                <DatabaseScreen
                                  description={description.result}
                                />
                              ) : (
                                <p className="muted">
                                  Loading the database description…
                                </p>
                              ))}
                            {screen.kind === "table" && (
                              <TableScreen
                                key={step.id}
                                ty_name={screen.ty_name}
                                view={screen.view ?? defaultTableScreenView}
                              />
                            )}
                            {screen.kind === "inspector" && (
                              <InspectorScreen
                                key={step.id}
                                id={screen.id}
                                view={screen.view ?? defaultInspectorView}
                                onExplore={exploreFrom}
                              />
                            )}
                            {screen.kind === "operation" && (
                              <OperationScreen
                                name={screen.name}
                                onBack={() => dispatch({ kind: "back" })}
                              />
                            )}
                            {screen.kind === "import" && <ImportView />}
                            {screen.kind === "findings" && <FindingsScreen />}
                          </EditContext>
                          <div hidden={screen.kind !== "explore"}>
                            <EditContext value={editExplore}>
                              <ExploreScreen
                                screen={shownExplore}
                                hidden={screen.kind !== "explore"}
                              />
                            </EditContext>
                          </div>
                        </div>
                      </main>
                      {tray}
                    </>
                  )}
                </div>
              </PlacesProvider>
            </DraftProvider>
          </TypesContext>
        </ExploreContext>
      </ReplaceContext>
    </NavigationContext>
  );
}

const BOUNDARY = /^(0|[1-9][0-9]*)$/;

function TopBar({
  description,
  nav,
  dispatch,
  asOf,
  onAsOf,
  onRefresh,
}: {
  description: Load<Description>;
  nav: Navigation;
  dispatch: (action: NavigationAction) => void;
  asOf: DecimalString | null;
  onAsOf: (asOf: DecimalString | null) => void;
  onRefresh: () => void;
}) {
  const open = useOpen();
  const [draft, setDraft] = useState(asOf ?? "");
  // A boundary set elsewhere, such as the inspector's "As of here", or
  // cleared for another database, shows in the field.
  const [shown, setShown] = useState(asOf);
  if (shown !== asOf) {
    setShown(asOf);
    setDraft(asOf ?? "");
  }
  const valid = BOUNDARY.test(draft);
  const visible =
    description.status === "ready"
      ? description.result.view.end_exclusive
      : null;
  // The server refuses a boundary past the end as well; checking here only
  // spares a request that is bound to fail.
  const beyond = valid && visible !== null && BigInt(draft) > BigInt(visible);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (valid && !beyond) {
      onAsOf(draft);
    }
  };
  return (
    <header className="top">
      <h1>LifeCore</h1>
      {description.status === "ready" && (
        <>
          <button
            type="button"
            className="link"
            title={`database ${description.result.database}`}
            onClick={() => open({ kind: "database" })}
          >
            <code className="db-id" data-testid="database">
              {description.result.database}
            </code>
          </button>
          {asOf === null ? (
            <span title="the whole history">
              entries{" "}
              <strong data-testid="entries">
                {description.result.view.end_exclusive}
              </strong>
            </span>
          ) : (
            <span
              className="badge asof"
              data-testid="as-of"
              title={`Every screen shows ${viewWords(asOf)}; "current" shows the whole history again.`}
            >
              showing the first {asOf} of{" "}
              <strong data-testid="entries">
                {description.result.view.end_exclusive}
              </strong>{" "}
              entries, view [0, {asOf})
            </span>
          )}
        </>
      )}
      <form className="row" onSubmit={submit}>
        <label title="Show the database as it was after this many entries were appended. The inspector's “As of here” sets it at an entry.">
          as of{" "}
          <input
            name="boundary"
            inputMode="numeric"
            size={8}
            placeholder="entry count"
            value={draft}
            onChange={(event) => setDraft(event.target.value.trim())}
          />
        </label>
        <button type="submit" disabled={!valid || beyond}>
          Set
        </button>
        {beyond && (
          <span className="muted">at most {visible}, the whole history</span>
        )}
        {asOf !== null && (
          <button
            type="button"
            title="show the whole history again"
            onClick={() => onAsOf(null)}
          >
            current
          </button>
        )}
      </form>
      <span className="spacer" />
      <button
        type="button"
        disabled={nav.history.length === 0}
        onClick={() => dispatch({ kind: "back" })}
      >
        ← Back
      </button>
      <button type="button" onClick={onRefresh}>
        Refresh
      </button>
    </header>
  );
}

// Steps the trail shows before folding older ones into "…".
const TRAIL = 6;
const TRAIL_LABEL = 28;

// A step as the trail names it: the type of a table, the label of an entry
// as last seen, the seeds of a query.
function stepName(
  screen: Screen,
  label: (id: EntryId) => string | undefined,
): string {
  switch (screen.kind) {
    case "database":
      return "Database";
    case "table":
      return `${typeName(screen.ty_name)} table`;
    case "inspector":
      return label(screen.id) ?? `@${screen.id.position}`;
    case "explore": {
      const ids = screen.ran?.form.ids ?? [];
      const [first] = ids;
      if (first === undefined) {
        return screen.ran === null ? "Explore" : "Explore: node selection";
      }
      return `Explore ${seedName(first, label)}${
        ids.length > 1 ? ` +${ids.length - 1}` : ""
      }`;
    }
    case "findings":
      return "Findings";
    case "operation":
      return `+${screen.name}`;
    case "import":
      return "Import";
  }
}

// The Back stack as a trail under the top bar: the last steps by name, the
// current one last; a click goes back to that step, dropping the ones after
// it.
function Trail({
  nav,
  dispatch,
}: {
  nav: Navigation;
  dispatch: (action: NavigationAction) => void;
}) {
  const book = useLabels();
  const { asOf } = useData();
  // Labels are noted as screens read entries; the trail follows the notes.
  useSyncExternalStore(book.subscribe, book.version);
  const { places } = usePlaces();
  // Past the boundary, a step names its entry by position alone.
  const label = (id: EntryId) =>
    pastBoundary(id, asOf)
      ? undefined
      : (book.get(id, asOf) ??
        [...places.recent, ...places.pinned].find((place) =>
          sameId(place.id, id),
        )?.label);
  const { history } = nav;
  const first = Math.max(0, history.length - TRAIL);
  return (
    <ol className="trail" aria-label="Back trail" data-testid="trail">
      {first > 0 && (
        <li className="muted" title={`${first} earlier steps`}>
          …
        </li>
      )}
      {history.slice(first).map((step, offset) => {
        const name = stepName(step.screen, label);
        return (
          <li key={step.id}>
            <button
              type="button"
              className="link"
              title={`back to ${name}`}
              onClick={() => dispatch({ kind: "back", to: first + offset })}
            >
              {truncate(name, TRAIL_LABEL)}
            </button>
          </li>
        );
      })}
      <li aria-current="page">
        <strong>
          {truncate(stepName(nav.current.screen, label), TRAIL_LABEL)}
        </strong>
      </li>
    </ol>
  );
}
