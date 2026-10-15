import { createContext, useContext, useEffect, useState } from "react";
import type {
  AsOfParams,
  DecimalString,
  Method,
  Params,
  Result,
  TypeSchema,
  TypeSummary,
} from "./contract.ts";
import type { RpcClient } from "./rpc.ts";

// What every read shares: the client, the selected boundary (null for the
// whole history), and a counter the Refresh button bumps to re-fetch.
// `bumpRefresh` bumps it from anywhere, such as after an applied draft.
export interface DataSource {
  client: RpcClient;
  asOf: DecimalString | null;
  refresh: number;
  bumpRefresh: () => void;
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

// Calls `method` whenever the JSON of `params`, the refresh counter, or `run`
// changes; null params mean no call. Keying by JSON, not identity, lets
// screens build params inline on every render. The loading state is derived:
// the stored answer names the key it answers, and any other key is loading,
// so a changed key never renders a stale answer as ready.
export function useCall<M extends Method>(
  method: M,
  params: Params<M> | null,
  run = 0,
): Load<Result<M>> {
  const { client, refresh } = useData();
  const key = params === null ? null : JSON.stringify({ params, refresh, run });
  const [settled, setSettled] = useState<{
    key: string;
    load: Settled<Result<M>>;
  } | null>(null);
  useEffect(() => {
    if (key === null) {
      return;
    }
    let active = true;
    const { params: sent } = JSON.parse(key) as { params: Params<M> };
    client.call(method, sent).then(
      (result) => {
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
  }, [client, method, key]);
  if (key === null) {
    return { status: "idle" };
  }
  if (settled !== null && settled.key === key) {
    return settled.load;
  }
  return settled?.load.status === "ready"
    ? { status: "loading", previous: settled.load.result }
    : { status: "loading" };
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
