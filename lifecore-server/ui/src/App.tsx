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
  TypesContext,
  useAsOf,
  useCall,
} from "./data.tsx";
import { DraftProvider } from "./draft/context.tsx";
import { FailureView } from "./failure.tsx";
import { NavigationContext, ReplaceContext, useOpen } from "./links.tsx";
import { entryKey, typeName } from "./model/labels.ts";
import { exploreReducer, initialExplore } from "./model/query.ts";
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
  // remounts, restoring that database's draft, if it ever changes.
  const database = latest(description)?.database ?? null;
  // Explore stays mounted, hidden under other screens, so its results
  // survive a detour through the inspector and Back.
  return (
    <NavigationContext value={open}>
      <ReplaceContext value={replace}>
        <TypesContext value={latest(types)?.types ?? null}>
          <DraftProvider key={database ?? ""} database={database}>
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
                <main className="full">
                  <FailureView failure={description.failure} />
                </main>
              ) : (
                <>
                  <Sidebar types={types} screen={screen} />
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
                    {/* Not keyed by name: a rename must not remount the
                      editor under the user's cursor. */}
                    {screen.kind === "operation" && (
                      <OperationScreen name={screen.name} />
                    )}
                    {screen.kind === "import" && <ImportView />}
                    <div hidden={screen.kind !== "explore"}>
                      <ExploreScreen
                        state={explore}
                        dispatch={dispatchExplore}
                      />
                    </div>
                  </main>
                  <Tray screen={screen} />
                </>
              )}
            </div>
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
function Sidebar({ types, screen }: { types: Load<TypeList>; screen: Screen }) {
  const open = useOpen();
  const list = latest(types);
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
        <li className={screen.kind === "explore" ? "active" : undefined}>
          <button
            type="button"
            className="link"
            onClick={() => open({ kind: "explore" })}
          >
            Explore
          </button>
        </li>
      </ul>
    </nav>
  );
}
