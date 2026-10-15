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
  TypesContext,
  useAsOf,
  useCall,
} from "./data.tsx";
import { FailureView } from "./failure.tsx";
import { NavigationContext, useOpen } from "./links.tsx";
import { entryKey } from "./model/labels.ts";
import { addSeed, emptyForm } from "./model/query.ts";
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
import { ResolutionBadge, TableScreen } from "./screens/Table.tsx";

export function App({ client }: { client: RpcClient }) {
  const [asOf, setAsOf] = useState<DecimalString | null>(null);
  const [refresh, setRefresh] = useState(0);
  const source = useMemo(
    () => ({ client, asOf, refresh }),
    [client, asOf, refresh],
  );
  return (
    <DataContext value={source}>
      <Shell
        asOf={asOf}
        onAsOf={setAsOf}
        onRefresh={() => setRefresh((count) => count + 1)}
      />
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
  const [explore, setExplore] = useState(emptyForm);
  const open = useCallback(
    (screen: Screen) => dispatch({ kind: "open", screen }),
    [],
  );
  const description = useCall("database.describe", {});
  const types = useCall("types.list", useAsOf());
  const exploreFrom = (id: EntryId) => {
    setExplore((form) => addSeed(form, id));
    open({ kind: "explore" });
  };
  const screen = nav.current;
  return (
    <NavigationContext value={open}>
      <TypesContext
        value={types.status === "ready" ? types.result.types : null}
      >
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
                    <p className="muted">Loading the database description…</p>
                  ))}
                {screen.kind === "table" && (
                  <TableScreen key={screen.ty_name} ty_name={screen.ty_name} />
                )}
                {screen.kind === "inspector" && (
                  <InspectorScreen
                    key={entryKey(screen.id)}
                    id={screen.id}
                    onExplore={exploreFrom}
                  />
                )}
                {/* Explore stays mounted so its results survive a detour
                    through the inspector and Back. */}
                <div hidden={screen.kind !== "explore"}>
                  <ExploreScreen form={explore} onChange={setExplore} />
                </div>
              </main>
            </>
          )}
        </div>
      </TypesContext>
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
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (valid) {
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
          <span>
            entries{" "}
            <strong data-testid="entries">
              {description.result.view.end_exclusive}
            </strong>
          </span>
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
        <button type="submit" disabled={!valid}>
          Set
        </button>
        {asOf !== null && (
          <>
            <span className="badge asof" data-testid="as-of">
              as of {asOf}
            </span>
            <button
              type="button"
              onClick={() => {
                onAsOf(null);
                setDraft("");
              }}
            >
              current
            </button>
          </>
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

function Sidebar({ types, screen }: { types: Load<TypeList>; screen: Screen }) {
  const open = useOpen();
  return (
    <nav className="side" aria-label="Types">
      <h2>Types</h2>
      {types.status === "loading" && <p className="muted">loading…</p>}
      {types.status === "failed" && <FailureView failure={types.failure} />}
      {types.status === "ready" && (
        <ul className="types" data-testid="types">
          {types.result.types.map((type) => (
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
                {type.name === "" ? <em>(unnamed)</em> : type.name}
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
