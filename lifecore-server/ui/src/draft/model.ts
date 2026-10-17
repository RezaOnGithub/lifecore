import type {
  DecimalString,
  DecodeError,
  EntryId,
  EvaluationParams,
  OperationEntry,
  PlanId,
  Proposal,
  ProposalLocation,
  ProposalReference,
  ProposalReport,
  Published,
  Uuid,
} from "../contract.ts";
import type { PickRole } from "../editors/OperationEditor.tsx";

// An endpoint the user has yet to pick. The contract has no "absent" edge
// endpoint or predecessor, so an empty identity stands in; it is never sent
// (`incomplete` stops Check and Export) and renders as "?".
const UNSET_ID: EntryId = { origin: "", position: "" };
export const UNSET: ProposalReference = { existing: UNSET_ID };

export function isUnset(reference: ProposalReference): boolean {
  return "existing" in reference && reference.existing.origin === "";
}

export function sameReference(
  a: ProposalReference,
  b: ProposalReference,
): boolean {
  if ("added" in a) {
    return "added" in b && a.added === b.added;
  }
  return (
    "existing" in b &&
    a.existing.origin === b.existing.origin &&
    a.existing.position === b.existing.position
  );
}

// The evaluation-time choice of Check (INTERFACES §5): absent by default.
export type Evaluation =
  | { kind: "none" }
  | { kind: "now" }
  | { kind: "at"; time: string };

export function evaluationParams(evaluation: Evaluation): EvaluationParams {
  switch (evaluation.kind) {
    case "none":
      return {};
    case "now":
      return { now: true };
    case "at":
      return { evaluation_time: evaluation.time };
  }
}

export function sameEvaluation(a: Evaluation, b: Evaluation): boolean {
  if (a.kind === "at" || b.kind === "at") {
    return a.kind === "at" && b.kind === "at" && a.time === b.time;
  }
  return a.kind === b.kind;
}

// The last complete report: a check's, or the one a rejected apply returned.
// `request` is the JSON of the proposal it judged; Apply is gated on it
// matching the current draft exactly. `names` are that proposal's operation
// names by index, following later renames, so findings stay on their
// operation. `spent` marks a check an apply of uncertain fate used.
export interface Outcome {
  source: "check" | "apply";
  request: string;
  names: string[];
  evaluation: Evaluation;
  accepted: boolean;
  report: ProposalReport;
  spent: boolean;
}

// What the last call reported besides a report: shown as a banner until the
// next call or, for content problems, the next edit. Each one is known to
// have happened before publication; an apply that may have been published is
// an `Uncertain` record instead.
export type Problem =
  | {
      kind: "invalid_proposal";
      message: string;
      location: ProposalLocation | null;
    }
  | {
      kind: "malformed_proposal";
      message: string;
      decode: DecodeError;
      operation: string | null;
    }
  | { kind: "incomplete"; message: string; operation: string }
  | {
      kind: "foreign_database";
      message: string;
      expected: Uuid;
      actual: Uuid;
    }
  | { kind: "busy"; message: string; action: "check" | "apply" }
  // A check that got no answer; nothing was written, retry is the user's call.
  | { kind: "retryable"; message: string }
  | { kind: "failed"; message: string; action: "check" | "apply" };

// The apply being sent, persisted before the request leaves so a reload in
// the middle restores the draft as uncertain. The base comes from the check
// it used, to look for the entries afterwards.
export interface InFlight {
  plan_id: PlanId | null;
  request: string;
  base_revision: Uuid | null;
  base_end: DecimalString;
}

// What a look at the graph after an uncertain apply found.
export type Verdict =
  | { kind: "unchanged" }
  | { kind: "absent" }
  | { kind: "other" }
  | { kind: "present"; operations: OperationEntry[] }
  | { kind: "failed"; message: string };

// An apply that may have been published: `error` is the error kind,
// `transport` for a lost answer, or `interrupted` for a page that went away
// mid-apply. Only the user's decision clears it.
export interface Uncertain extends InFlight {
  error: string;
  message: string;
  outcome: Published | null;
  verdict: Verdict | null;
}

// Where a kept edge came from: the revision it keeps a relationship for, the
// node that revision revised when it was kept, and the original edge.
export interface KeptEdge {
  revision: string;
  predecessor: EntryId;
  edge: EntryId;
}

// A note about an edit that stays until dismissed; an offer is a follow-up
// the user may take, such as removing edges left without an end.
export interface Notice {
  id: number;
  text: string;
  offer: { label: string; remove: string[] } | null;
}

export interface Applied {
  graph_revision: Uuid | null;
  operations: OperationEntry[];
  report: ProposalReport | null;
}

export interface Picking {
  operation: string;
  role: PickRole;
}

export type Pending = "mint" | "check" | "apply" | "encode" | "decode" | null;

export interface DraftState {
  // True until the provider knows the database: nothing can be stored, and
  // the first description remounts the provider, so no edit lands.
  detached: boolean;
  proposal: Proposal;
  // Provenance of the kept edges, by edge operation name.
  kept: Record<string, KeptEdge>;
  // The operation each read of a predecessor's current edges is for, by read
  // number, following renames; a removed operation's read is dropped, so its
  // answer lands nowhere.
  reads: Record<number, string>;
  selected: string | null;
  picking: Picking | null;
  evaluation: Evaluation;
  outcome: Outcome | null;
  // The plan ID names an earlier graph revision; only Replan clears it.
  stale: { expected: Uuid; actual: Uuid } | null;
  problem: Problem | null;
  // This tab's apply while `pending` is "apply"; otherwise another tab's,
  // adopted from storage.
  inFlight: InFlight | null;
  uncertain: Uncertain | null;
  // The last successful Apply, kept until dismissed.
  applied: Applied | null;
  pending: Pending;
  notices: Notice[];
  nextNotice: number;
}

export const initialDraft: DraftState = {
  detached: false,
  proposal: { entries: [] },
  kept: {},
  reads: {},
  selected: null,
  picking: null,
  evaluation: { kind: "none" },
  outcome: null,
  stale: null,
  problem: null,
  inFlight: null,
  uncertain: null,
  applied: null,
  pending: null,
  notices: [],
  nextNotice: 1,
};

export const detachedDraft: DraftState = { ...initialDraft, detached: true };

// The draft without client state, exactly as it is sent.
export function proposalOf(state: DraftState): Proposal {
  const { plan_id, entries } = state.proposal;
  return plan_id === undefined ? { entries } : { plan_id, entries };
}

// Proposals in the state are never changed in place, so the text is
// computed once per proposal.
const requests = new WeakMap<Proposal, string>();

export function requestOf(state: DraftState): string {
  const known = requests.get(state.proposal);
  if (known !== undefined) {
    return known;
  }
  const request = JSON.stringify(proposalOf(state));
  requests.set(state.proposal, request);
  return request;
}

// While an apply is in flight, in this tab or another, the draft is what was
// sent: no edit lands. Nor does one before the database is known.
export function frozen(state: DraftState): boolean {
  return state.detached || state.inFlight !== null || state.pending === "apply";
}

export function applyingElsewhere(state: DraftState): boolean {
  return state.inFlight !== null && state.pending !== "apply";
}

// Apply is enabled only for an unspent accepted check of this very content
// and evaluation choice, with no apply of uncertain fate outstanding and
// none in flight in another tab.
export function applicable(state: DraftState): boolean {
  const { outcome } = state;
  return (
    outcome !== null &&
    outcome.source === "check" &&
    outcome.accepted &&
    !outcome.spent &&
    state.stale === null &&
    state.uncertain === null &&
    state.pending === null &&
    !frozen(state) &&
    state.proposal.entries.length > 0 &&
    outcome.request === requestOf(state) &&
    sameEvaluation(outcome.evaluation, state.evaluation)
  );
}

// Apply reuses the inputs the check reported, so a check with `now` applies
// at the instant that check captured, not at a new one.
export function applyParams(outcome: Outcome): EvaluationParams {
  const time = outcome.report.analysis.inputs.evaluation_time;
  return time === null ? {} : { evaluation_time: time };
}
