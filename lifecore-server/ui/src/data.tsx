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
export interface DataSource {
  client: RpcClient;
  asOf: DecimalString | null;
  refresh: number;
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

export type Load<T> =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; result: T }
  | { status: "failed"; failure: unknown };

// Calls `method` whenever the JSON of `params`, the refresh counter, or `run`
// changes; null params mean no call. Keying by JSON, not identity, lets
// screens build params inline on every render.
export function useCall<M extends Method>(
  method: M,
  params: Params<M> | null,
  run = 0,
): Load<Result<M>> {
  const { client, refresh } = useData();
  const key = params === null ? null : JSON.stringify({ params, refresh, run });
  const [load, setLoad] = useState<Load<Result<M>>>({
    status: key === null ? "idle" : "loading",
  });
  useEffect(() => {
    if (key === null) {
      setLoad({ status: "idle" });
      return;
    }
    let active = true;
    setLoad({ status: "loading" });
    const { params: sent } = JSON.parse(key) as { params: Params<M> };
    client.call(method, sent).then(
      (result) => {
        if (active) {
          setLoad({ status: "ready", result });
        }
      },
      (failure: unknown) => {
        if (active) {
          setLoad({ status: "failed", failure });
        }
      },
    );
    return () => {
      active = false;
    };
  }, [client, method, key]);
  return load;
}

// The sidebar's type list, shared so screens can look up a schema without a
// second `types.list` call; null until it has loaded.
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
