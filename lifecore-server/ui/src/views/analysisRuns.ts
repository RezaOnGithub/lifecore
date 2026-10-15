import { useSyncExternalStore } from "react";
import type {
  AnalysisRun,
  AnalysisRunParams,
  DecimalString,
  Dismissal,
  DismissalReason,
  FindingKey,
  NodeRecord,
} from "../contract.ts";
import { useData } from "../data.tsx";
import type { Evaluation } from "../draft/model.ts";
import {
  defaultFilter,
  type FindingFilter,
  keyText,
} from "../model/findings.ts";
import { entryKey } from "../model/labels.ts";
import type { RpcClient } from "../rpc.ts";

// A stored-database analysis with the nodes of its view, for labels. `sent`
// counts the dismissal edits that had finished when it was requested.
export interface Ran {
  run: AnalysisRun;
  nodes: Map<string, NodeRecord>;
  // Why the labels could not be read; the findings are shown without them.
  labels: unknown;
  sent: number;
}

export type RunState =
  | { status: "running"; token: object; previous: Ran | null }
  | { status: "ready"; ran: Ran }
  | { status: "failed"; failure: unknown; previous: Ran | null };

export function shownRun(state: RunState | undefined): Ran | null {
  if (state === undefined) {
    return null;
  }
  return state.status === "ready" ? state.ran : state.previous;
}

// What the Findings screen keeps across visits: its choices and its last
// run, with the boundary and refresh count it was asked under.
export interface ScreenMemo {
  evaluation: Evaluation;
  filter: FindingFilter;
  last: { key: string; asOf: DecimalString | null; refresh: number } | null;
}

// The runs of one page's client, keyed by their parameters and the refresh
// counter, so a reload of the screens asks again and anything else reuses
// the answer. Analysis replays the whole selected history, so nothing here
// starts a run on its own: the Findings screen runs on its button and the
// inspector section when expanded.
export class AnalysisRuns {
  readonly #client: RpcClient;
  readonly #runs = new Map<string, RunState>();
  readonly #listeners = new Set<() => void>();
  #version = 0;
  #edits = 0;
  #edited: Dismissal[] | null = null;
  #editing: string | null = null;
  #editFailure: { id: string; failure: unknown } | null = null;
  #screen: ScreenMemo = {
    evaluation: { kind: "none" },
    filter: defaultFilter,
    last: null,
  };
  #entryOpen = false;

  constructor(client: RpcClient) {
    this.#client = client;
  }

  subscribe = (listener: () => void) => {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  };

  version = () => this.#version;

  #changed() {
    this.#version += 1;
    for (const listener of this.#listeners) {
      listener();
    }
  }

  get(key: string): RunState | undefined {
    return this.#runs.get(key);
  }

  // Outputs replay the whole history and can be large, so only the last
  // few are kept.
  #set(key: string, state: RunState) {
    this.#runs.delete(key);
    this.#runs.set(key, state);
    while (this.#runs.size > 4) {
      const oldest = this.#runs.keys().next().value;
      if (oldest === undefined) {
        break;
      }
      this.#runs.delete(oldest);
    }
    this.#changed();
  }

  // Starts a run for `key` unless one is known; `again` replaces it. An
  // answer to a replaced run is dropped.
  run(key: string, params: AnalysisRunParams, again = false) {
    const known = this.#runs.get(key);
    if (known !== undefined && (!again || known.status === "running")) {
      return;
    }
    const token = {};
    const sent = this.#edits;
    this.#set(key, { status: "running", token, previous: shownRun(known) });
    const current = () => {
      const state = this.#runs.get(key);
      return state?.status === "running" && state.token === token;
    };
    void (async () => {
      let run: AnalysisRun;
      try {
        run = await this.#client.call("analysis.run", params);
      } catch (failure) {
        if (current()) {
          this.#set(key, {
            status: "failed",
            failure,
            previous: shownRun(known),
          });
        }
        return;
      }
      // Every subject of the run lies inside its view, so that view's nodes
      // label them all.
      const nodes = new Map<string, NodeRecord>();
      let labels: unknown = null;
      try {
        const list = await this.#client.call("entries.list", {
          kind: "node",
          currentness: "all",
          as_of: run.analysis.view.end_exclusive,
        });
        for (const entry of list.entries) {
          if (entry.kind === "node") {
            nodes.set(entryKey(entry.id), entry);
          }
        }
      } catch (failure) {
        labels = failure;
      }
      if (current()) {
        this.#set(key, { status: "ready", ran: { run, nodes, labels, sent } });
      }
    })();
  }

  // The records that apply to a run: those it came with, unless an edit
  // finished after it was requested, whose answer lists every stored record.
  dismissals(ran: Ran): Dismissal[] {
    return this.#edited !== null && this.#edits > ran.sent
      ? this.#edited
      : ran.run.dismissals;
  }

  // The key of the edit in flight. Edits go one at a time, so the last
  // answer is the newest stored list.
  editing(): string | null {
    return this.#editing;
  }

  editFailure(key: FindingKey): unknown {
    const id = keyText(key);
    return this.#editFailure?.id === id ? this.#editFailure.failure : null;
  }

  // Whether the edit was stored.
  async #edit(
    key: FindingKey,
    call: () => Promise<{ dismissals: Dismissal[] }>,
  ): Promise<boolean> {
    if (this.#editing !== null) {
      return false;
    }
    const id = keyText(key);
    this.#editing = id;
    this.#editFailure = null;
    this.#changed();
    try {
      const { dismissals } = await call();
      this.#edited = dismissals;
      this.#edits += 1;
      return true;
    } catch (failure) {
      this.#editFailure = { id, failure };
      return false;
    } finally {
      this.#editing = null;
      this.#changed();
    }
  }

  // Parameters never carry null, so an empty note is left out.
  dismiss(key: FindingKey, reason: DismissalReason, note: string) {
    return this.#edit(key, () =>
      this.#client.call(
        "findings.dismiss",
        note === "" ? { key, reason } : { key, reason, note },
      ),
    );
  }

  restore(key: FindingKey) {
    return this.#edit(key, () =>
      this.#client.call("findings.restore", { key }),
    );
  }

  screen(): ScreenMemo {
    return this.#screen;
  }

  setScreen(patch: Partial<ScreenMemo>) {
    this.#screen = { ...this.#screen, ...patch };
    this.#changed();
  }

  // The inspector section stays open across inspections once opened.
  entryOpen(): boolean {
    return this.#entryOpen;
  }

  setEntryOpen(open: boolean) {
    this.#entryOpen = open;
    this.#changed();
  }
}

export function runKey(params: AnalysisRunParams, refresh: number): string {
  return JSON.stringify({ params, refresh });
}

const stores = new WeakMap<RpcClient, AnalysisRuns>();

// The page's store, re-rendering the caller on every change of it.
export function useAnalysisRuns(): AnalysisRuns {
  const { client } = useData();
  let store = stores.get(client);
  if (store === undefined) {
    store = new AnalysisRuns(client);
    stores.set(client, store);
  }
  useSyncExternalStore(store.subscribe, store.version);
  return store;
}
