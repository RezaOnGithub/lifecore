import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import type {
  AsOfParams,
  DecimalString,
  EntryId,
  Method,
  NodeEntry,
  Params,
  Result,
  TypeSchema,
  TypeSummary,
} from "./contract.ts";
import { entryKey, rowLabel } from "./model/labels.ts";
import {
  emptyPlaces,
  loadPlaces,
  type Place,
  type Places,
  placesKey,
  removeView,
  saveView,
  storePlaces,
  togglePin,
  unpin,
  visit,
} from "./model/places.ts";
import type { ExploreForm } from "./model/query.ts";
import type { RpcClient } from "./rpc.ts";

// What every read shares: the client, the selected boundary (null for the
// whole history), and a counter the Refresh button bumps to re-fetch.
// `bumpRefresh` bumps it from anywhere, such as after an applied draft.
// `cache` keeps the last results by call, so a screen returned to by Back
// renders at once while it reads again.
export interface DataSource {
  client: RpcClient;
  asOf: DecimalString | null;
  refresh: number;
  bumpRefresh: () => void;
  cache: ResultCache;
}

// The last result of each call, by method and params, the least recently
// used dropped beyond `limit`. Results are only ever replaced whole.
export class ResultCache {
  readonly #entries = new Map<string, unknown>();
  readonly #limit: number;

  constructor(limit = 40) {
    this.#limit = limit;
  }

  get(key: string): unknown {
    const value = this.#entries.get(key);
    if (value !== undefined) {
      this.#entries.delete(key);
      this.#entries.set(key, value);
    }
    return value;
  }

  set(key: string, value: unknown) {
    this.#entries.delete(key);
    this.#entries.set(key, value);
    for (const oldest of this.#entries.keys()) {
      if (this.#entries.size <= this.#limit) {
        break;
      }
      this.#entries.delete(oldest);
    }
  }
}

export const DataContext = createContext<DataSource | null>(null);

export function useData(): DataSource {
  const source = useContext(DataContext);
  if (source === null) {
    throw new Error("useData needs a DataContext provider");
  }
  return source;
}

// The `as_of` member a read spreads into its params; empty for the whole
// history, so the request carries no `as_of` at all.
export function useAsOf(): AsOfParams {
  const { asOf } = useData();
  return asOf === null ? {} : { as_of: asOf };
}

type Settled<T> =
  | { status: "ready"; result: T }
  | { status: "failed"; failure: unknown };

// A reload carries the result it is replacing, so screens can keep showing
// it instead of flashing empty.
export type Load<T> =
  | { status: "idle" }
  | { status: "loading"; previous?: T }
  | Settled<T>;

// The newest result a load holds: the ready one, or the one a reload is
// about to replace.
export function latest<T>(load: Load<T>): T | null {
  switch (load.status) {
    case "ready":
      return load.result;
    case "loading":
      return load.previous ?? null;
    default:
      return null;
  }
}

// Reads whose answer is a function of the view alone: the history is
// append-only, so the same params answered at the same boundary of the same
// database give the same result, and a reread that names the cached view
// keeps the cached object, sparing the screen a re-render.
const VIEW_PURE = new Set<Method>([
  "entries.inspect",
  "entries.list",
  "types.list",
  "views.query",
]);

function sameView(a: unknown, b: unknown): boolean {
  const view = (value: unknown) =>
    typeof value === "object" && value !== null && "view" in value
      ? (value.view as { database?: unknown; end_exclusive?: unknown })
      : null;
  const x = view(a);
  const y = view(b);
  return (
    x !== null &&
    y !== null &&
    x.database === y.database &&
    x.end_exclusive === y.end_exclusive
  );
}

// Calls `method` whenever the JSON of `params`, the refresh counter, or `run`
// changes; null params mean no call. Keying by JSON, not identity, lets
// screens build params inline on every render. The loading state is derived:
// the stored answer names the key it answers, and any other key is loading,
// so a changed key never renders a stale answer as ready. While loading, the
// result on show is the cached one for the same call, else the last answer
// this caller had.
export function useCall<M extends Method>(
  method: M,
  params: Params<M> | null,
  run = 0,
): Load<Result<M>> {
  const { client, refresh, cache } = useData();
  const call = params === null ? null : JSON.stringify({ method, params, run });
  const key = call === null ? null : `${refresh}:${call}`;
  const [settled, setSettled] = useState<{
    key: string;
    load: Settled<Result<M>>;
  } | null>(null);
  useEffect(() => {
    if (key === null || call === null) {
      return;
    }
    let active = true;
    const { params: sent } = JSON.parse(call) as { params: Params<M> };
    client.call(method, sent).then(
      (fresh) => {
        const cached = cache.get(call) as Result<M> | undefined;
        const result =
          cached !== undefined &&
          VIEW_PURE.has(method) &&
          sameView(cached, fresh)
            ? cached
            : fresh;
        cache.set(call, result);
        if (active) {
          setSettled({ key, load: { status: "ready", result } });
        }
      },
      (failure: unknown) => {
        if (active) {
          setSettled({ key, load: { status: "failed", failure } });
        }
      },
    );
    return () => {
      active = false;
    };
  }, [client, cache, method, call, key]);
  if (key === null || call === null) {
    return { status: "idle" };
  }
  if (settled !== null && settled.key === key) {
    return settled.load;
  }
  const cached = cache.get(call) as Result<M> | undefined;
  if (cached !== undefined) {
    return { status: "loading", previous: cached };
  }
  return settled?.load.status === "ready"
    ? { status: "loading", previous: settled.load.result }
    : { status: "loading" };
}

// Labels of entries met on any screen, by entry key, so the Back trail and
// Explore's seeds can name entries the screen on show has not read. Notes
// are made in effects, after rendering; `subscribe` and `version` let a
// component follow them as an external store.
export class LabelBook {
  readonly #labels = new Map<string, string>();
  readonly #listeners = new Set<() => void>();
  #version = 0;

  get(id: EntryId): string | undefined {
    return this.#labels.get(entryKey(id));
  }

  note(id: EntryId, label: string) {
    this.#set([[entryKey(id), label]]);
  }

  noteNodes(nodes: Iterable<NodeEntry>) {
    this.#set(
      (function* () {
        for (const node of nodes) {
          yield [entryKey(node.id), rowLabel(node)] as const;
        }
      })(),
    );
  }

  #set(labels: Iterable<readonly [string, string]>) {
    let changed = false;
    for (const [key, label] of labels) {
      if (this.#labels.get(key) !== label) {
        this.#labels.set(key, label);
        changed = true;
      }
    }
    if (changed) {
      this.#version += 1;
      for (const listener of this.#listeners) {
        listener();
      }
    }
  }

  readonly subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  };

  readonly version = () => this.#version;
}

export const LabelsContext = createContext<LabelBook | null>(null);

export function useLabels(): LabelBook {
  const labels = useContext(LabelsContext);
  if (labels === null) {
    throw new Error("useLabels needs a LabelsContext provider");
  }
  return labels;
}

// The sidebar's type list, shared so screens can look up a schema without a
// second `types.list` call; null until it has loaded, and the last loaded
// list while a reload is in flight.
export const TypesContext = createContext<TypeSummary[] | null>(null);

export function useTypes(): TypeSummary[] | null {
  return useContext(TypesContext);
}

export function schemaOf(
  types: TypeSummary[] | null,
  ty_name: string,
): TypeSchema | null {
  const summary = types?.find((type) => type.name === ty_name);
  return summary?.resolution.kind === "declared"
    ? summary.resolution.schema
    : null;
}

export interface PlacesApi {
  places: Places;
  visit: (place: Place) => void;
  togglePin: (place: Place) => void;
  unpin: (id: EntryId) => void;
  saveView: (name: string, form: ExploreForm) => void;
  removeView: (name: string) => void;
}

const PlacesContext = createContext<PlacesApi | null>(null);

export function usePlaces(): PlacesApi {
  const places = useContext(PlacesContext);
  if (places === null) {
    throw new Error("usePlaces needs a PlacesProvider");
  }
  return places;
}

// Recent, pinned, and saved places of one database, kept in localStorage.
// Mount it keyed by the database id, like the draft: the stored places are
// read once, on mount, and written after every change; nothing is stored
// before the database is known.
export function PlacesProvider({
  database,
  children,
}: {
  database: string | null;
  children: ReactNode;
}) {
  const [places, setPlaces] = useState(() =>
    database === null ? emptyPlaces : loadPlaces(window.localStorage, database),
  );
  useEffect(() => {
    if (database !== null) {
      storePlaces(window.localStorage, database, places);
    }
  }, [database, places]);
  // Another tab's change arrives as a storage event; storing the same text
  // again raises none, so the tabs do not echo each other.
  useEffect(() => {
    if (database === null) {
      return;
    }
    const onStorage = (event: StorageEvent) => {
      if (
        event.storageArea === window.localStorage &&
        event.key === placesKey(database)
      ) {
        setPlaces(loadPlaces(window.localStorage, database));
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [database]);
  // The actions keep their identity across changes, so an effect that
  // records a visit does not run again because the places changed.
  const actions = useMemo(
    () => ({
      visit: (place: Place) => setPlaces((current) => visit(current, place)),
      togglePin: (place: Place) =>
        setPlaces((current) => togglePin(current, place)),
      unpin: (id: EntryId) => setPlaces((current) => unpin(current, id)),
      saveView: (name: string, form: ExploreForm) =>
        setPlaces((current) => saveView(current, name, form)),
      removeView: (name: string) =>
        setPlaces((current) => removeView(current, name)),
    }),
    [],
  );
  const value = useMemo(() => ({ places, ...actions }), [places, actions]);
  return <PlacesContext value={value}>{children}</PlacesContext>;
}
