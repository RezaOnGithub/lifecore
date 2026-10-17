import type {
  DecodeError,
  EntryChange,
  EntryId,
  Format,
  Method,
  NodeRecord,
  Params,
  Proposal,
  ProposalEntry,
  Result,
  RevisionIntent,
} from "../contract.ts";
import type { PickRole } from "../editors/OperationEditor.tsx";
import { RpcError } from "../rpc.ts";
import { type Classified, classify, message } from "./classify.ts";
import { namesOf } from "./edits.ts";
import {
  applicable,
  applyParams,
  type DraftState,
  type Evaluation,
  evaluationParams,
  frozen,
  type InFlight,
  proposalOf,
  requestOf,
  UNSET,
  type Verdict,
} from "./model.ts";
import {
  defaultName,
  findApplied,
  incomplete,
  suggestedOperations,
} from "./operations.ts";
import { reconnectable } from "./reconnect.ts";
import type { DraftAction } from "./reducer.ts";

export type Call = <M extends Method>(
  method: M,
  params: Params<M>,
) => Promise<Result<M>>;

export type Decoded =
  | { kind: "ok"; proposal: Proposal }
  | { kind: "malformed"; message: string; decode: DecodeError }
  | { kind: "failed"; message: string };

export type Encoded =
  | { kind: "ok"; text: string }
  | { kind: "failed"; message: string };

// Read numbers are unique in the page, so an answer never lands on a read
// another instance started.
let lastRead = 0;

// The event-driven side of the draft: every method is called from a handler,
// reads the state of that moment through `state()`, which must already
// reflect its own dispatches, and records what the server answered through
// `dispatch`; nothing here is keyed or retried. `refresh` reloads every
// screen after a write that was, or may have been, published. The dispatch
// persists the stored parts of the draft synchronously, so the in-flight
// marker is saved before an apply request leaves.
export class DraftActions {
  readonly #call: Call;
  readonly #dispatch: (action: DraftAction) => void;
  readonly #state: () => DraftState;
  readonly #refresh: () => void;
  #minting: Promise<void> | null = null;

  constructor(
    client: { call: Call },
    dispatch: (action: DraftAction) => void,
    state: () => DraftState,
    refresh: () => void = () => {},
  ) {
    this.#call = (method, params) => client.call(method, params);
    this.#dispatch = dispatch;
    this.#state = state;
    this.#refresh = refresh;
  }

  #notice(text: string) {
    this.#dispatch({ kind: "notice", text });
  }

  // ---- Local edits that may need a plan ID ----

  // The name the reducer settles on is `entry.name` unless taken; callers
  // pass a free one. Null while an apply is in flight: nothing is added.
  add(entry: ProposalEntry, after?: string): string | null {
    if (frozen(this.#state())) {
      return null;
    }
    this.#dispatch(
      after === undefined
        ? { kind: "add", entry }
        : { kind: "add", entry, after },
    );
    void this.ensurePlanId();
    return entry.name;
  }

  addNode(ty_name: string): string | null {
    const { entries } = this.#state().proposal;
    return this.add({
      name: defaultName(entries, ty_name),
      kind: "node",
      ty_name,
      payload: {},
    });
  }

  addEdge(ty_name = ""): string | null {
    const { entries } = this.#state().proposal;
    return this.add({
      name: defaultName(entries, ty_name === "" ? "edge" : ty_name),
      kind: "edge",
      ty_name,
      from: UNSET,
      to: UNSET,
    });
  }

  // "Connect": an edge from this node whose other end is picked next.
  connect(from: EntryId): string | null {
    const { entries } = this.#state().proposal;
    const name = this.add({
      name: defaultName(entries, "edge"),
      kind: "edge",
      ty_name: "",
      from: { existing: from },
      to: UNSET,
    });
    if (name !== null) {
      this.#dispatch({
        kind: "pick",
        picking: { operation: name, role: "to" },
      });
    }
    return name;
  }

  // A revision with the payload copied. For revise, the kept edges follow
  // once the node's current relationships are read.
  revise(node: NodeRecord, intent: RevisionIntent): string | null {
    const { entries } = this.#state().proposal;
    const name = this.add({
      name: defaultName(entries, node.ty_name),
      kind: "node",
      ty_name: node.ty_name,
      predecessor: { existing: node.id },
      intent,
      payload: node.payload,
    });
    if (name !== null && intent === "revise") {
      this.#keepCurrentEdges(name, node.id);
    }
    return name;
  }

  // Reads without `as_of`, because the draft applies to the current state.
  // The answer goes to the operation under its name at that time, and lands
  // only if the operation still revises the node; the reducer checks that
  // when it arrives.
  #keepCurrentEdges(operation: string, predecessor: EntryId) {
    lastRead += 1;
    const read = lastRead;
    this.#dispatch({ kind: "reading", read, operation });
    this.#call("entries.inspect", { id: predecessor }).then(
      (inspection) => {
        if (inspection.kind === "node") {
          this.#dispatch({
            kind: "keep",
            read,
            operation,
            predecessor,
            intent: "revise",
            edges: reconnectable(inspection.edges),
          });
        } else {
          this.#dispatch({ kind: "reading", read, operation: null });
        }
      },
      (failure: unknown) => {
        this.#dispatch({ kind: "reading", read, operation: null });
        this.#notice(
          `could not read the current edges of @${predecessor.position}: ${message(failure)}`,
        );
      },
    );
  }

  startPick(operation: string, role: PickRole) {
    this.#dispatch({ kind: "pick", picking: { operation, role } });
  }

  cancelPick() {
    this.#dispatch({ kind: "pick", picking: null });
  }

  // A predecessor picked for a revise gets the same kept edges as Revise in
  // the inspector; a branch gets none.
  resolvePick(id: EntryId): string | null {
    const { picking } = this.#state();
    if (picking === null || frozen(this.#state())) {
      return null;
    }
    this.#dispatch({ kind: "resolve_pick", id });
    const entry = this.#state().proposal.entries.find(
      (e) => e.name === picking.operation,
    );
    if (
      picking.role === "predecessor" &&
      entry?.kind === "node" &&
      entry.intent === "revise"
    ) {
      this.#keepCurrentEdges(picking.operation, id);
    }
    return picking.operation;
  }

  // The name of the first operation as it landed, or null when nothing did.
  importProposal(
    proposal: Proposal,
    mode: "replace" | "append",
  ): string | null {
    if (frozen(this.#state()) || proposal.acknowledges !== undefined) {
      return null;
    }
    const before = this.#state().proposal.entries.length;
    this.#dispatch({ kind: "import", proposal, mode });
    void this.ensurePlanId();
    const { entries } = this.#state().proposal;
    return entries[mode === "replace" ? 0 : before]?.name ?? null;
  }

  // A suggestion's actions appended as ordinary operations, never with
  // `acknowledges` (PLAN §4.4). The names they landed under; none while an
  // apply is in flight.
  addSuggested(actions: readonly EntryChange[]): string[] {
    const { entries } = this.#state().proposal;
    if (actions.length === 0 || frozen(this.#state())) {
      return [];
    }
    this.importProposal(
      { entries: suggestedOperations(entries, actions) },
      "append",
    );
    return this.#state()
      .proposal.entries.slice(entries.length)
      .map((entry) => entry.name);
  }

  // ---- Calls ----

  // Whether the draft has a plan ID, minting one when it lacks it. Mints
  // once per missing plan ID: a caller arriving during a mint waits for
  // that one. The reducer drops the answer if the draft was cleared
  // meanwhile. A failure is a notice, so it never hides a banner about the
  // last check or apply.
  async ensurePlanId(): Promise<boolean> {
    const { proposal } = this.#state();
    if (proposal.plan_id === undefined && proposal.entries.length > 0) {
      this.#minting ??= this.#mint().finally(() => {
        this.#minting = null;
      });
      await this.#minting;
    }
    return this.#state().proposal.plan_id !== undefined;
  }

  async #mint(): Promise<void> {
    if (this.#state().pending === null) {
      this.#dispatch({ kind: "pending", pending: "mint" });
    }
    try {
      const { plan_id } = await this.#call("plan_id.mint", {});
      this.#dispatch({ kind: "set_plan_id", plan_id });
    } catch (failure) {
      this.#notice(`could not mint a plan ID: ${message(failure)}`);
    } finally {
      if (this.#state().pending === "mint") {
        this.#dispatch({ kind: "pending", pending: null });
      }
    }
  }

  // Not while an apply may have been published or is in flight: that needs
  // the user's decision or the apply's answer first.
  async replan(): Promise<void> {
    const state = this.#state();
    if (state.pending !== null || state.uncertain !== null || frozen(state)) {
      return;
    }
    this.#dispatch({ kind: "pending", pending: "mint" });
    try {
      const { plan_id } = await this.#call("plan_id.mint", {});
      this.#dispatch({ kind: "replan", plan_id });
    } catch (failure) {
      this.#dispatch({ kind: "pending", pending: null });
      this.#notice(`could not mint a plan ID: ${message(failure)}`);
    }
  }

  async check(): Promise<void> {
    const state = this.#state();
    if (state.pending !== null || state.proposal.entries.length === 0) {
      return;
    }
    const missing = incomplete(state.proposal.entries);
    if (missing !== null) {
      this.#dispatch({
        kind: "problem",
        problem: { kind: "incomplete", ...missing },
      });
      return;
    }
    if (!(await this.ensurePlanId())) {
      return;
    }
    const current = this.#state();
    if (current.pending !== null) {
      return;
    }
    const { evaluation } = current;
    const proposal = proposalOf(current);
    const request = requestOf(current);
    this.#dispatch({ kind: "pending", pending: "check" });
    try {
      const result = await this.#call("proposal.check", {
        proposal,
        ...evaluationParams(evaluation),
      });
      this.#dispatch({ kind: "checked", result, request, evaluation });
    } catch (failure) {
      this.#settle(classify(failure, "check", namesOf(request)), request, {
        evaluation,
        flight: null,
      });
    }
  }

  // Applies exactly what the accepted check judged, with the inputs it
  // reported. The draft is frozen until the answer, and the marker saved
  // first turns a reload in the middle into an uncertain draft. Nothing is
  // sent when the marker could not be saved over another tab's.
  async apply(): Promise<void> {
    const state = this.#state();
    const { outcome } = state;
    if (outcome === null || !applicable(state)) {
      return;
    }
    const proposal = proposalOf(state);
    const { request, evaluation } = outcome;
    const flight: InFlight = {
      plan_id: proposal.plan_id ?? null,
      request,
      base_revision: outcome.report.base_revision ?? null,
      base_end: outcome.report.base.end_exclusive,
    };
    this.#dispatch({ kind: "apply_started", flight });
    // Another tab's marker was stored first and has been adopted instead.
    if (this.#state().inFlight !== flight) {
      return;
    }
    let result: Result<"proposal.apply">;
    try {
      result = await this.#call("proposal.apply", {
        proposal,
        ...applyParams(outcome),
      });
    } catch (failure) {
      this.#settle(classify(failure, "apply", namesOf(request)), request, {
        evaluation,
        flight,
      });
      return;
    }
    this.#dispatch({ kind: "applied", result, request });
    this.#refresh();
    // Operations the request did not carry stay and need a new plan ID.
    void this.ensurePlanId();
  }

  #settle(
    classified: Classified,
    request: string,
    { evaluation, flight }: { evaluation: Evaluation; flight: InFlight | null },
  ) {
    switch (classified.kind) {
      case "stale":
        this.#dispatch({
          kind: "stale",
          expected: classified.expected,
          actual: classified.actual,
        });
        return;
      case "rejected":
        this.#dispatch({
          kind: "rejected",
          report: classified.report,
          request,
          evaluation,
        });
        return;
      case "problem":
        this.#dispatch({ kind: "problem", problem: classified.problem });
        return;
      case "uncertain":
        if (flight === null) {
          return;
        }
        this.#dispatch({
          kind: "uncertain",
          uncertain: {
            ...flight,
            error: classified.error,
            message: classified.message,
            outcome: classified.outcome,
            verdict: null,
          },
        });
        // A possibly published write: reload what the screens show, which
        // is a read and never a retry of the apply.
        this.#refresh();
        return;
    }
  }

  // Looks for the entries an uncertain apply would have added, at the
  // positions it would have taken after the check's base. Only reads.
  async lookForEntries(): Promise<void> {
    const { uncertain } = this.#state();
    if (uncertain === null) {
      return;
    }
    let verdict: Verdict;
    try {
      const description = await this.#call("database.describe", {});
      if (description.graph_revision === uncertain.base_revision) {
        verdict = { kind: "unchanged" };
      } else if (
        BigInt(description.view.end_exclusive) <= BigInt(uncertain.base_end)
      ) {
        verdict = { kind: "absent" };
      } else {
        const sent = (JSON.parse(uncertain.request) as Proposal).entries;
        const { entries } = await this.#call("entries.list", {
          currentness: "all",
        });
        const operations = findApplied(sent, uncertain.base_end, entries);
        verdict =
          operations === null
            ? { kind: "other" }
            : { kind: "present", operations };
      }
    } catch (failure) {
      verdict = { kind: "failed", message: message(failure) };
    }
    if (this.#state().uncertain?.request === uncertain.request) {
      this.#dispatch({ kind: "verdict", verdict });
    }
  }

  // Refused locally while an endpoint is unset: the codec would write the
  // placeholder as if it were an entry.
  async encode(format: Format): Promise<Encoded> {
    const state = this.#state();
    if (state.pending !== null) {
      return { kind: "failed", message: "another call is in flight" };
    }
    const missing = incomplete(state.proposal.entries);
    if (missing !== null) {
      return { kind: "failed", message: `cannot export: ${missing.message}` };
    }
    this.#dispatch({ kind: "pending", pending: "encode" });
    try {
      const { text } = await this.#call("proposal.encode", {
        proposal: proposalOf(state),
        format,
      });
      return { kind: "ok", text };
    } catch (failure) {
      return {
        kind: "failed",
        message: `could not encode the draft: ${message(failure)}`,
      };
    } finally {
      this.#dispatch({ kind: "pending", pending: null });
    }
  }

  // The raw text goes to the Rust codec unparsed: bare integers must not pass
  // through JavaScript numbers.
  async decode(text: string, format: Format): Promise<Decoded> {
    if (this.#state().pending !== null) {
      return { kind: "failed", message: "another call is in flight" };
    }
    this.#dispatch({ kind: "pending", pending: "decode" });
    try {
      const proposal = await this.#call("proposal.decode", { text, format });
      return { kind: "ok", proposal };
    } catch (failure) {
      if (failure instanceof RpcError) {
        const { error } = failure;
        const data = "data" in error ? error.data : undefined;
        if (data?.kind === "malformed_proposal") {
          return {
            kind: "malformed",
            message: error.message,
            decode: data.decode,
          };
        }
      }
      return { kind: "failed", message: message(failure) };
    } finally {
      this.#dispatch({ kind: "pending", pending: null });
    }
  }
}
