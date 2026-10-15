import {
  type FormEvent,
  useCallback,
  useMemo,
  useReducer,
  useState,
} from "react";
import type {
  DecimalString,
  Description,
  EntryId,
  TypeList,
} from "./contract.ts";
import {
  DataContext,
  type Load,
  latest,
  PlacesProvider,
  TypesContext,
  useAsOf,
  useCall,
  useData,
  usePlaces,
} from "./data.tsx";
import { DraftProvider } from "./draft/context.tsx";
import { FailureView } from "./failure.tsx";
import { NavigationContext, ReplaceContext, useOpen } from "./links.tsx";
import { entryKey, LABEL_LIMIT, sameId, typeName } from "./model/labels.ts";
import { truncate } from "./model/payload.ts";
import type { Place } from "./model/places.ts";
import {
  type ExploreAction,
  exploreReducer,
  initialExplore,
} from "./model/query.ts";
import {
  initialNavigation,
  type Navigation,
  type NavigationAction,
  navigate,
  type Screen,
} from "./navigation.ts";
import type { RpcClient } from "./rpc.ts";
import { DatabaseScreen } from "./screens/Database.tsx";
import { ExploreScreen } from "./screens/Explore.tsx";
import { FindingsScreen } from "./screens/Findings.tsx";
import { InspectorScreen } from "./screens/Inspector.tsx";
import { OperationScreen } from "./screens/Operation.tsx";
import { TableScreen } from "./screens/Table.tsx";
import { ImportView } from "./views/Import.tsx";
import { Tray } from "./views/Tray.tsx";
import { ResolutionBadge } from "./views/widgets.tsx";

export function App({ client }: { client: RpcClient }) {
  const [asOf, setAsOf] = useState<DecimalString | null>(null);
  const [refresh, setRefresh] = useState(0);
  const bumpRefresh = useCallback(() => setRefresh((count) => count + 1), []);
  const source = useMemo(
    () => ({ client, asOf, refresh, bumpRefresh }),
    [client, asOf, refresh, bumpRefresh],
  );
  return (
    <DataContext value={source}>
      <Shell asOf={asOf} onAsOf={setAsOf} onRefresh={bumpRefresh} />
    </DataContext>
  );
}

function Shell({
  asOf,
  onAsOf,
  onRefresh,
}: {
  asOf: DecimalString | null;
  onAsOf: (asOf: DecimalString | null) => void;
  onRefresh: () => void;
}) {
  const [nav, dispatch] = useReducer(navigate, initialNavigation);
  const [explore, dispatchExplore] = useReducer(exploreReducer, initialExplore);
  const open = useCallback(
    (screen: Screen) => dispatch({ kind: "open", screen }),
    [],
  );
  const replace = useCallback(
    (screen: Screen) => dispatch({ kind: "replace", screen }),
    [],
  );
  const description = useCall("database.describe", {});
  const types = useCall("types.list", useAsOf());
  const exploreFrom = (id: EntryId) => {
    dispatchExplore({ kind: "seed", id });
    open({ kind: "explore" });
  };
  const screen = nav.current;
  // The draft is per database: the provider mounts once the id is known and
  // remounts, restoring that database's draft, only when another database
  // answers. A failed or reloading description keeps the last id, so an
  // outage never empties the draft or orphans an apply in flight.
  const [database, setDatabase] = useState<string | null>(null);
  const described = latest(description);
  if (described !== null && described.database !== database) {
    setDatabase(described.database);
  }
  const head = described?.view.end_exclusive ?? null;
  const tray = <Tray screen={screen} head={head} />;
  // Explore stays mounted, hidden under other screens, so its results
  // survive a detour through the inspector and Back. The operation screen
  // is not keyed by name: a rename must not remount the editor under the
  // user's cursor.
  return (
    <NavigationContext value={open}>
      <ReplaceContext value={replace}>
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
                      screen={screen}
                      dispatchExplore={dispatchExplore}
                    />
                    <main>
                      {screen.kind === "database" &&
                        (description.status === "ready" ? (
                          <DatabaseScreen description={description.result} />
                        ) : (
                          <p className="muted">
                            Loading the database description…
                          </p>
                        ))}
                      {screen.kind === "table" && (
                        <TableScreen
                          key={screen.ty_name}
                          ty_name={screen.ty_name}
                        />
                      )}
                      {screen.kind === "inspector" && (
                        <InspectorScreen
                          key={entryKey(screen.id)}
                          id={screen.id}
                          onExplore={exploreFrom}
                        />
                      )}
                      {screen.kind === "operation" && (
                        <OperationScreen name={screen.name} />
                      )}
                      {screen.kind === "import" && <ImportView />}
                      {screen.kind === "findings" && <FindingsScreen />}
                      <div hidden={screen.kind !== "explore"}>
                        <ExploreScreen
                          state={explore}
                          dispatch={dispatchExplore}
                        />
                      </div>
                    </main>
                    {tray}
                  </>
                )}
              </div>
            </PlacesProvider>
          </DraftProvider>
        </TypesContext>
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
  const [draft, setDraft] = useState("");
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
            onClick={() =>
              dispatch({ kind: "open", screen: { kind: "database" } })
            }
          >
            <code className="db-id" data-testid="database">
              {description.result.database}
            </code>
          </button>
          {asOf === null ? (
            <span>
              entries{" "}
              <strong data-testid="entries">
                {description.result.view.end_exclusive}
              </strong>
            </span>
          ) : (
            <span className="badge asof" data-testid="as-of">
              view [0, {asOf}) of{" "}
              <strong data-testid="entries">
                {description.result.view.end_exclusive}
              </strong>{" "}
              entries
            </span>
          )}
        </>
      )}
      <form className="row" onSubmit={submit}>
        <label>
          as of{" "}
          <input
            name="boundary"
            inputMode="numeric"
            size={8}
            placeholder="boundary"
            value={draft}
            onChange={(event) => setDraft(event.target.value.trim())}
          />
        </label>
        <button type="submit" disabled={!valid || beyond}>
          Set
        </button>
        {beyond && <span className="muted">max {visible}</span>}
        {asOf !== null && (
          <button
            type="button"
            onClick={() => {
              onAsOf(null);
              setDraft("");
            }}
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

// The list on show is the last one loaded: a reload only adds a hint, so the
// entries, and the active one, stay where they are.
function Sidebar({
  types,
  screen,
  dispatchExplore,
}: {
  types: Load<TypeList>;
  screen: Screen;
  dispatchExplore: (action: ExploreAction) => void;
}) {
  const open = useOpen();
  const list = latest(types);
  const { places, unpin, removeView } = usePlaces();
  const view = (kind: "explore" | "findings", label: string) => (
    <li className={screen.kind === kind ? "active" : undefined}>
      <button type="button" className="link" onClick={() => open({ kind })}>
        {label}
      </button>
    </li>
  );
  return (
    <nav className="side" aria-label="Types">
      <h2>
        Types{" "}
        {types.status === "loading" && list !== null && (
          <span className="hint">reloading…</span>
        )}
      </h2>
      {types.status === "loading" && list === null && (
        <p className="muted">loading…</p>
      )}
      {types.status === "failed" && (
        <FailureView failure={types.failure} compact />
      )}
      {list !== null && (
        <ul className="types" data-testid="types">
          {list.types.map((type) => (
            <li
              key={type.name}
              className={
                screen.kind === "table" && screen.ty_name === type.name
                  ? "active"
                  : undefined
              }
            >
              <button
                type="button"
                className="link"
                onClick={() => open({ kind: "table", ty_name: type.name })}
              >
                {typeName(type.name)}
              </button>{" "}
              <span className="counts" title="current / historical nodes">
                {type.nodes.current}/{type.nodes.historical}
              </span>{" "}
              <ResolutionBadge resolution={type.resolution} />
            </li>
          ))}
        </ul>
      )}
      <h2>Views</h2>
      <ul className="types">
        {view("explore", "Explore")}
        {view("findings", "Findings")}
      </ul>
      <h2>Pinned</h2>
      <PlaceList
        places={places.pinned}
        screen={screen}
        testId="pinned"
        empty="☆ in the inspector pins an entry here"
        onRemove={(place) => unpin(place.id)}
      />
      <h2>Saved views</h2>
      {places.views.length === 0 ? (
        <p className="muted side-hint">Save view in Explore keeps a query</p>
      ) : (
        <ul className="types places" data-testid="saved-views">
          {places.views.map((saved) => (
            <li key={saved.name}>
              <button
                type="button"
                className="link place-label"
                title={`open and run "${saved.name}"`}
                onClick={() => {
                  dispatchExplore({ kind: "load", form: saved.form });
                  open({ kind: "explore" });
                }}
              >
                {saved.name}
              </button>
              <button
                type="button"
                className="link remove"
                aria-label={`remove view ${saved.name}`}
                onClick={() => removeView(saved.name)}
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}
      <h2>Recent</h2>
      <PlaceList
        places={places.recent}
        screen={screen}
        testId="recent"
        empty="inspected entries appear here"
      />
    </nav>
  );
}

// Entries kept by the sidebar, under the label and type they had when last
// inspected. Under an as-of boundary, those past it are dimmed: they would
// not be found.
function PlaceList({
  places,
  screen,
  testId,
  empty,
  onRemove,
}: {
  places: readonly Place[];
  screen: Screen;
  testId: string;
  empty: string;
  onRemove?: (place: Place) => void;
}) {
  const open = useOpen();
  const { asOf } = useData();
  if (places.length === 0) {
    return <p className="muted side-hint">{empty}</p>;
  }
  return (
    <ul className="types places" data-testid={testId}>
      {places.map((place) => {
        const beyond =
          asOf !== null && BigInt(place.id.position) >= BigInt(asOf);
        const active =
          screen.kind === "inspector" && sameId(screen.id, place.id);
        return (
          <li
            key={entryKey(place.id)}
            className={
              [active && "active", beyond && "beyond"]
                .filter(Boolean)
                .join(" ") || undefined
            }
          >
            <button
              type="button"
              className="link place-label"
              title={`${place.label}\n@${place.id.position} ${
                place.kind === "edge" ? "edge " : ""
              }${typeName(place.ty_name)}${
                beyond ? "\npast the as-of boundary" : ""
              }`}
              onClick={() => open({ kind: "inspector", id: place.id })}
            >
              {truncate(place.label, LABEL_LIMIT)}
            </button>
            <span className="place-type">
              {place.kind === "edge"
                ? `@${place.id.position}`
                : typeName(place.ty_name)}
            </span>
            {onRemove && (
              <button
                type="button"
                className="link remove"
                aria-label={`unpin ${place.label}`}
                onClick={() => onRemove(place)}
              >
                ×
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}
