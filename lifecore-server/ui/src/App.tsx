import {
  memo,
  type ReactNode,
  type RefObject,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import type { DecimalString, EntryId } from "./contract.ts";
import {
  DataContext,
  LabelBook,
  LabelsContext,
  latest,
  PlacesProvider,
  ResultCache,
  TypesContext,
  useAsOf,
  useCall,
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
} from "./links.tsx";
import { type ExploreForm, exploreStep, fromHere } from "./model/query.ts";
import {
  applyExploreStep,
  defaultInspectorView,
  defaultTableScreenView,
  exploreScreen,
  initialNavigation,
  navigate,
  type Screen,
  type Step,
  sameScreen,
} from "./navigation.ts";
import type { RpcClient } from "./rpc.ts";
import { DatabaseScreen } from "./screens/Database.tsx";
import { ExploreScreen } from "./screens/Explore.tsx";
import { FindingsScreen } from "./screens/Findings.tsx";
import { InspectorScreen } from "./screens/Inspector.tsx";
import { OperationScreen } from "./screens/Operation.tsx";
import { TableScreen } from "./screens/Table.tsx";
import { ImportView } from "./views/Import.tsx";
import { Sidebar } from "./views/Sidebar.tsx";
import { TopBar } from "./views/TopBar.tsx";
import { Trail } from "./views/Trail.tsx";
import { Tray } from "./views/Tray.tsx";

// The draft of the places' database, which skips the names kept there in
// its default names and records there what its applies name. It mounts
// with the places, keyed by the same database.
function NamedDraftProvider({
  database,
  children,
}: {
  database: string | null;
  children: ReactNode;
}) {
  const { latestAliases, recordApplied } = usePlaces();
  return (
    <DraftProvider
      database={database}
      aliases={latestAliases}
      onApplied={recordApplied}
    >
      {children}
    </DraftProvider>
  );
}

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
            <PlacesProvider key={database ?? "unknown"} database={database}>
              <NamedDraftProvider database={database}>
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
              </NamedDraftProvider>
            </PlacesProvider>
          </TypesContext>
        </ExploreContext>
      </ReplaceContext>
    </NavigationContext>
  );
}
