import { useSyncExternalStore } from "react";
import { useData } from "../data.tsx";
import type { RpcClient } from "../rpc.ts";
import { AnalysisRuns } from "./analysisRuns.ts";

const stores = new WeakMap<RpcClient, AnalysisRuns>();

// The page's store, re-rendering the caller on every change of it.
export function useAnalysisRuns(): AnalysisRuns {
  const { client } = useData();
  let store = stores.get(client);
  if (store === undefined) {
    store = new AnalysisRuns((method, params) => client.call(method, params));
    stores.set(client, store);
  }
  useSyncExternalStore(store.subscribe, store.version);
  return store;
}
