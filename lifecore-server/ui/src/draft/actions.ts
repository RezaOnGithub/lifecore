import type {
  DecodeError,
  EntryId,
  Format,
  IncidentEdge,
  Method,
  NodeRecord,
  Params,
  Proposal,
  ProposalEntry,
  ProposalReport,
  Result,
  RevisionIntent,
  Uuid,
} from "../contract.ts";
import type { PickRole } from "../editors/OperationEditor.tsx";
import { HttpRejectionError, RpcError, TransportError } from "../rpc.ts";
import {
  type DraftAction,
  type DraftState,
  defaultName,
  evaluationParams,
  incomplete,
  type Problem,
  proposalOf,
  requestOf,
  UNSET,
} from "./model.ts";
import { reconnections } from "./reconnect.ts";

export type Call = <M extends Method>(
  method: M,
  params: Params<M>,
) => Promise<Result<M>>;

export type Classified =
  | { kind: "stale"; expected: Uuid; actual: Uuid }
  | { kind: "rejected"; report: ProposalReport }
  | { kind: "problem"; problem: Problem };

function message(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure);
}

// Sorts a failed check or apply by what the user can do about it. A lost
// answer means nothing was written for a check, but may have been for an
// apply, which is never retried on its own.
export function classify(
  failure: unknown,
  action: "check" | "apply",
): Classified {
  if (failure instanceof RpcError) {
    const { error } = failure;
    const data = "data" in error ? error.data : undefined;
    switch (data?.kind) {
      case "stale_base":
        return { kind: "stale", expected: data.expected, actual: data.actual };
      case "rejected":
        return { kind: "rejected", report: data.report };
      case "invalid_proposal":
        return {
          kind: "problem",
          problem: {
            kind: "invalid_proposal",
            message: error.message,
            location: data.location ?? null,
          },
        };
      case "malformed_proposal":
        return {
          kind: "problem",
          problem: {
            kind: "malformed_proposal",
            message: error.message,
            decode: data.decode,
          },
        };
      case "foreign_database":
        return {
          kind: "problem",
          problem: { kind: "foreign_database", message: error.message },
        };
      case "busy":
        return {
          kind: "problem",
          problem: { kind: "busy", message: error.message, action },
        };
      case "publication_uncertain":
        return {
          kind: "problem",
          problem: {
            kind: "uncertain",
            message: error.message,
            outcome: data.outcome,
          },
        };
      case "backend_failure":
        return {
          kind: "problem",
          problem:
            action === "apply"
              ? { kind: "uncertain", message: error.message, outcome: null }
              : { kind: "retryable", message: error.message },
        };
      default:
        return {
          kind: "problem",
          problem: { kind: "failed", message: error.message },
        };
    }
  }
  if (failure instanceof TransportError) {
    return {
      kind: "problem",
      problem:
        action === "apply"
          ? { kind: "uncertain", message: failure.message, outcome: null }
          : { kind: "retryable", message: failure.message },
    };
  }
  if (failure instanceof HttpRejectionError) {
    return {
      kind: "problem",
      problem: { kind: "failed", message: failure.message },
    };
  }
  return {
    kind: "problem",
    problem: { kind: "failed", message: message(failure) },
  };
}

export type Decoded =
  | { kind: "ok"; proposal: Proposal }
  | { kind: "malformed"; message: string; decode: DecodeError }
  | { kind: "failed"; message: string };

// The event-driven side of the draft: every method is called from a handler,
// reads the state of that moment through `state()`, which must already
// reflect its own dispatches, and records what the server answered through
// `dispatch`; nothing here is keyed or retried. `refresh` reloads every
// screen after a write that was, or may have been, published.
export class DraftActions {
  readonly #call: Call;
  readonly #dispatch: (action: DraftAction) => void;
  readonly #state: () => DraftState;
  readonly #refresh: () => void;
  #minting = false;

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

  // ---- Local edits that may need a plan ID ----

  // The name the reducer settles on is `entry.name` unless taken; callers
  // pass a free one, so the returned name is the entry's.
  add(entry: ProposalEntry, after?: string): string {
    const name = entry.name;
    this.#dispatch(
      after === undefined
        ? { kind: "add", entry }
        : { kind: "add", entry, after },
    );
    void this.ensurePlanId();
    return name;
  }

  addNode(ty_name: string): string {
    const { entries } = this.#state().proposal;
    return this.add({
      name: defaultName(entries, ty_name),
      kind: "node",
      ty_name,
      payload: {},
    });
  }

  addEdge(ty_name = ""): string {
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
  connect(from: EntryId): string {
    const { entries } = this.#state().proposal;
    const name = this.add({
      name: defaultName(entries, "edge"),
      kind: "edge",
      ty_name: "",
      from: { existing: from },
      to: UNSET,
    });
    this.#dispatch({ kind: "pick", picking: { operation: name, role: "to" } });
    return name;
  }

  // A revision with the payload copied. The kept edges come from the node's
  // current relationships, read without `as_of` because the draft applies to
  // the current state; they follow once that read answers.
  revise(node: NodeRecord, intent: RevisionIntent): string {
    const { entries } = this.#state().proposal;
    const name = this.add({
      name: defaultName(entries, node.ty_name),
      kind: "node",
      ty_name: node.ty_name,
      predecessor: { existing: node.id },
      intent,
      payload: node.payload,
    });
    if (intent === "revise") {
      this.#call("entries.inspect", { id: node.id }).then(
        (inspection) => {
          if (inspection.kind !== "node") {
            return;
          }
          this.reconnect(name, inspection.edges, intent);
        },
        (failure: unknown) => {
          this.#dispatch({
            kind: "notice",
            notice: `could not read the current edges of @${node.id.position}: ${message(failure)}`,
          });
        },
      );
    }
    return name;
  }

  // Adds the default edge operations right after `operation`, each after the
  // previous one, so they keep the relationships' order.
  reconnect(
    operation: string,
    edges: readonly IncidentEdge[],
    intent: RevisionIntent,
  ) {
    if (!this.#state().proposal.entries.some((e) => e.name === operation)) {
      return;
    }
    let after = operation;
    for (const entry of reconnections(operation, edges, intent)) {
      this.#dispatch({ kind: "add", entry, after, select: false });
      // The reducer may have suffixed a taken name; the newest selection is
      // not it, so find the entry by position instead.
      const { entries } = this.#state().proposal;
      const at = entries.findIndex((e) => e.name === after);
      after = entries[at + 1]?.name ?? after;
    }
  }

  startPick(operation: string, role: PickRole) {
    this.#dispatch({ kind: "pick", picking: { operation, role } });
  }

  cancelPick() {
    this.#dispatch({ kind: "pick", picking: null });
  }

  resolvePick(id: EntryId): string | null {
    const { picking } = this.#state();
    if (picking === null) {
      return null;
    }
    this.#dispatch({ kind: "resolve_pick", id });
    return picking.operation;
  }

  importProposal(proposal: Proposal, mode: "replace" | "append") {
    this.#dispatch({ kind: "import", proposal, mode });
    void this.ensurePlanId();
  }

  // ---- Calls ----

  // Mints once per missing plan ID; the reducer drops the answer if the
  // draft was cleared meanwhile.
  async ensurePlanId(): Promise<boolean> {
    const { proposal, pending } = this.#state();
    if (proposal.plan_id !== undefined || proposal.entries.length === 0) {
      return proposal.plan_id !== undefined;
    }
    if (this.#minting) {
      return false;
    }
    this.#minting = true;
    if (pending === null) {
      this.#dispatch({ kind: "pending", pending: "mint" });
    }
    try {
      const { plan_id } = await this.#call("plan_id.mint", {});
      this.#dispatch({ kind: "set_plan_id", plan_id });
      this.#dispatch({ kind: "pending", pending: null });
      return true;
    } catch (failure) {
      this.#dispatch({
        kind: "problem",
        problem: {
          kind: "failed",
          message: `could not mint a plan ID: ${message(failure)}`,
        },
      });
      return false;
    } finally {
      this.#minting = false;
    }
  }

  async replan(): Promise<void> {
    if (this.#state().pending !== null) {
      return;
    }
    this.#dispatch({ kind: "pending", pending: "mint" });
    try {
      const { plan_id } = await this.#call("plan_id.mint", {});
      this.#dispatch({ kind: "replan", plan_id });
    } catch (failure) {
      this.#dispatch({
        kind: "problem",
        problem: {
          kind: "failed",
          message: `could not mint a plan ID: ${message(failure)}`,
        },
      });
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
      this.#settle(classify(failure, "check"), request, evaluation);
    }
  }

  // Applies with the inputs the accepted check used; the tray only enables
  // it for the same content.
  async apply(): Promise<void> {
    const state = this.#state();
    const { outcome } = state;
    if (
      state.pending !== null ||
      outcome === null ||
      !outcome.accepted ||
      outcome.request !== requestOf(state)
    ) {
      return;
    }
    const { evaluation } = outcome;
    this.#dispatch({ kind: "pending", pending: "apply" });
    try {
      const result = await this.#call("proposal.apply", {
        proposal: proposalOf(state),
        ...evaluationParams(evaluation),
      });
      this.#dispatch({ kind: "applied", result });
      this.#refresh();
    } catch (failure) {
      const classified = classify(failure, "apply");
      this.#settle(classified, outcome.request, evaluation);
      // A possibly published write: reload what the screens show, which is
      // a read and never a retry of the apply.
      if (
        classified.kind === "problem" &&
        classified.problem.kind === "uncertain"
      ) {
        this.#refresh();
      }
    }
  }

  #settle(
    classified: Classified,
    request: string,
    evaluation: DraftState["evaluation"],
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
    }
  }

  async encode(format: Format): Promise<string | null> {
    const state = this.#state();
    if (state.pending !== null) {
      return null;
    }
    this.#dispatch({ kind: "pending", pending: "encode" });
    try {
      const { text } = await this.#call("proposal.encode", {
        proposal: proposalOf(state),
        format,
      });
      this.#dispatch({ kind: "pending", pending: null });
      return text;
    } catch (failure) {
      this.#dispatch({
        kind: "problem",
        problem: {
          kind: "failed",
          message: `could not encode the draft: ${message(failure)}`,
        },
      });
      return null;
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
      this.#dispatch({ kind: "pending", pending: null });
      return { kind: "ok", proposal };
    } catch (failure) {
      this.#dispatch({ kind: "pending", pending: null });
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
    }
  }
}
