import type {
  AnalysisRun,
  AnalysisRunParams,
  DecimalString,
  Dismissal,
  DismissalReason,
  FindingKey,
  NodeRecord,
} from "../contract.ts";
import type { Evaluation } from "../draft/model.ts";
import {
  defaultFilter,
  type FindingFilter,
  keyText,
} from "../model/findings.ts";
import type { Call } from "../rpc.ts";
import {
  type Bulk,
  classifyEdit,
  type EditFailure,
  type EditResult,
} from "./dismissalEdits.ts";
import { namedIds, readLabels } from "./entryLabels.ts";

// A stored-database analysis with what it was asked under and the nodes it
// names, for labels. `sent` counts the edits that had finished when it was
// requested.
export interface Ran {
  run: AnalysisRun;
  params: AnalysisRunParams;
  refresh: number;
  nodes: Map<string, NodeRecord>;
  // Why some labels could not be read, null when none failed; those entries
  // show as positions.
  labelFailure: unknown;
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
  text: string;
  expanded: readonly string[];
  last: { key: string; asOf: DecimalString | null; refresh: number } | null;
}

export function runKey(params: AnalysisRunParams, refresh: number): string {
  return JSON.stringify({ params, refresh });
}

// Outputs replay the whole history and can be large, so only a few are
// kept, besides the one the Findings screen shows.
const KEPT = 4;

// The runs and dismissal edits of one page's client. Runs are keyed by
// their parameters and the refresh counter, so a reload of the screens asks
// again and anything else reuses the answer. Analysis replays the whole
// selected history, so nothing here starts a run on its own: the Findings
// screen runs on its button and the inspector section when expanded.
// Nothing is retried: a failed run or edit waits for the user.
export class AnalysisRuns {
  readonly #call: Call;
  readonly #runs = new Map<string, RunState>();
  readonly #listeners = new Set<() => void>();
  #version = 0;
  // Finished edits that may have changed the stored records: stored,
  // written, or of unknown outcome. Runs and edits are ordered by it.
  #edits = 0;
  // The newest records an edit answered with, and its place in that order.
  #records: { at: number; dismissals: Dismissal[] } | null = null;
  // An edit of unknown outcome since the last answered one.
  #uncertainAt: number | null = null;
  // Keys whose last edit was published with durability unconfirmed.
  readonly #written = new Map<string, number>();
  #editing: string | null = null;
  readonly #failures = new Map<string, EditFailure>();
  #bulk: Bulk | null = null;
  readonly #added = new Map<string, string[]>();
  #screen: ScreenMemo = {
    evaluation: { kind: "none" },
    filter: defaultFilter,
    text: "",
    expanded: [],
    last: null,
  };
  #entryOpen = false;

  constructor(call: Call) {
    this.#call = call;
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

  // The screen's run is never evicted, so what it shows stays reachable.
  #set(key: string, state: RunState) {
    this.#runs.delete(key);
    this.#runs.set(key, state);
    const pinned = this.#screen.last?.key;
    for (const old of this.#runs.keys()) {
      if (this.#runs.size <= KEPT) {
        break;
      }
      if (old !== key && old !== pinned) {
        this.#runs.delete(old);
      }
    }
    this.#changed();
  }

  // Starts a run unless one of these parameters and refresh count is known;
  // `again` replaces a finished one. An answer to a replaced or evicted run
  // is dropped. Returns the run's key.
  run(params: AnalysisRunParams, refresh: number, again = false): string {
    const key = runKey(params, refresh);
    const known = this.#runs.get(key);
    if (known !== undefined && (!again || known.status === "running")) {
      return key;
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
        run = await this.#call("analysis.run", params);
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
      const { nodes, failure } = await readLabels(
        this.#call,
        namedIds(run),
        run.analysis.view.end_exclusive,
      );
      if (current()) {
        this.#set(key, {
          status: "ready",
          ran: { run, params, refresh, nodes, labelFailure: failure, sent },
        });
      }
    })();
    return key;
  }

  // The records that apply to a run: those it came with, unless an edit
  // answered after it was requested, whose answer lists every stored record.
  dismissals(ran: Ran): Dismissal[] {
    return this.#records !== null && this.#records.at > ran.sent
      ? this.#records.dismissals
      : ran.run.dismissals;
  }

  // An edit of unknown outcome finished after this run was requested, and
  // no edit has answered since: the stored records may differ from those
  // shown.
  uncertain(ran: Ran): boolean {
    return this.#uncertainAt !== null && this.#uncertainAt > ran.sent;
  }

  // The key's last edit was published, with durability unconfirmed, after
  // this run was requested.
  written(ran: Ran, key: FindingKey): boolean {
    return (this.#written.get(keyText(key)) ?? 0) > ran.sent;
  }

  // The key of the edit in flight. Edits go one at a time, so the last
  // answer is the newest stored list.
  editing(): string | null {
    return this.#editing;
  }

  // Whether an edit or a "Dismiss all" is under way.
  busy(): boolean {
    return this.#editing !== null || this.#bulk?.running === true;
  }

  // How the key's last edit failed, until it is edited again.
  editFailure(key: FindingKey): EditFailure | null {
    return this.#failures.get(keyText(key)) ?? null;
  }

  // Callers make sure no other edit is in flight.
  async #edit(
    key: FindingKey,
    call: () => Promise<{ dismissals: Dismissal[] }>,
  ): Promise<"stored" | EditFailure["kind"]> {
    const id = keyText(key);
    this.#editing = id;
    this.#failures.delete(id);
    this.#changed();
    try {
      const { dismissals } = await call();
      this.#edits += 1;
      this.#records = { at: this.#edits, dismissals };
      this.#uncertainAt = null;
      // This publication carries every earlier record durably.
      this.#written.clear();
      return "stored";
    } catch (failure) {
      const classified = classifyEdit(failure);
      switch (classified.kind) {
        case "failed":
          this.#failures.set(id, classified);
          break;
        case "written":
          this.#edits += 1;
          this.#records = {
            at: this.#edits,
            dismissals: classified.dismissals,
          };
          this.#uncertainAt = null;
          this.#written.set(id, this.#edits);
          break;
        case "uncertain":
          this.#edits += 1;
          this.#uncertainAt = this.#edits;
          this.#failures.set(id, classified);
          break;
      }
      return classified.kind;
    } finally {
      this.#editing = null;
      this.#changed();
    }
  }

  // Parameters never carry null, so an empty note is left out; any other
  // note is sent as typed.
  #dismissCall(key: FindingKey, reason: DismissalReason, note: string) {
    return this.#call(
      "findings.dismiss",
      note === "" ? { key, reason } : { key, reason, note },
    );
  }

  async dismiss(
    key: FindingKey,
    reason: DismissalReason,
    note: string,
  ): Promise<EditResult> {
    if (this.busy()) {
      return "refused";
    }
    return this.#edit(key, () => this.#dismissCall(key, reason, note));
  }

  async restore(key: FindingKey): Promise<EditResult> {
    if (this.busy()) {
      return "refused";
    }
    return this.#edit(key, () => this.#call("findings.restore", { key }));
  }

  bulk(): Bulk | null {
    return this.#bulk;
  }

  // Dismisses each key in turn with one reason and note. A failure known to
  // precede publication is the key's own and the next key follows; one of
  // unknown outcome ends the run, as does Stop after the edit in flight.
  async dismissAll(
    cluster: { id: string; label: string },
    keys: readonly FindingKey[],
    reason: DismissalReason,
    note: string,
  ): Promise<void> {
    if (this.busy() || keys.length === 0) {
      return;
    }
    const bulk: Bulk = {
      cluster: cluster.id,
      label: cluster.label,
      total: keys.length,
      stored: 0,
      failed: 0,
      running: true,
      stopping: false,
      stopped: null,
    };
    // From here until it ends, `busy()` refuses every other edit, and each
    // of its own is awaited, so none of them overlaps another.
    this.#bulk = bulk;
    this.#changed();
    for (const key of keys) {
      if (bulk.stopping) {
        bulk.stopped = "stopped";
        break;
      }
      const result = await this.#edit(key, () =>
        this.#dismissCall(key, reason, note),
      );
      if (result === "stored" || result === "written") {
        bulk.stored += 1;
      } else {
        bulk.failed += 1;
      }
      if (result === "uncertain") {
        bulk.stopped = "an edit's outcome is unknown";
        break;
      }
    }
    bulk.running = false;
    this.#changed();
  }

  stopBulk() {
    if (this.#bulk?.running === true) {
      this.#bulk.stopping = true;
      this.#changed();
    }
  }

  // Forgets a finished "Dismiss all"; its keys' failures stay on their rows.
  clearBulk() {
    if (this.#bulk !== null && !this.#bulk.running) {
      this.#bulk = null;
      this.#changed();
    }
  }

  // The draft operations a suggestion's actions were added as, by the
  // observation, so a second addition can ask first.
  added(observation: string): readonly string[] | null {
    return this.#added.get(observation) ?? null;
  }

  setAdded(observation: string, names: string[]) {
    this.#added.set(observation, names);
    this.#changed();
  }

  forgetAdded(observation: string) {
    if (this.#added.delete(observation)) {
      this.#changed();
    }
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
