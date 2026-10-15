import type {
  ApplyResult,
  CheckResult,
  DecodeError,
  EntryId,
  EvaluationParams,
  OperationEntry,
  PlanId,
  Proposal,
  ProposalEntry,
  ProposalLocation,
  ProposalReference,
  ProposalReport,
  Published,
  Uuid,
} from "../contract.ts";
import type { PickRole } from "../editors/OperationEditor.tsx";

// Operation names (FILES §2): a letter or underscore, then letters, digits,
// underscores, and hyphens.
export const NAME = /^[A-Za-z_][A-Za-z0-9_-]*$/;

export function validName(name: string): boolean {
  return NAME.test(name);
}

// An endpoint the user has yet to pick. The contract has no "absent" edge
// endpoint, so an empty identity stands in; it is never sent (`incomplete`
// stops Check) and renders as "?" instead of a position.
export const UNSET_ID: EntryId = { origin: "", position: "" };
export const UNSET: ProposalReference = { existing: UNSET_ID };

export function isUnset(reference: ProposalReference): boolean {
  return "existing" in reference && reference.existing.origin === "";
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

// The last complete report: a check's, or the one a rejected apply returned.
// `request` is the JSON of the proposal it judged; Apply is gated on it
// matching the current draft exactly.
export interface Outcome {
  source: "check" | "apply";
  request: string;
  evaluation: Evaluation;
  accepted: boolean;
  report: ProposalReport;
}

// What the last call reported besides a report: shown as a banner until the
// next call or, for content problems, the next edit.
export type Problem =
  | {
      kind: "invalid_proposal";
      message: string;
      location: ProposalLocation | null;
    }
  | { kind: "malformed_proposal"; message: string; decode: DecodeError }
  | { kind: "incomplete"; message: string; operation: string }
  | { kind: "foreign_database"; message: string }
  | { kind: "busy"; message: string; action: "check" | "apply" }
  // A write may have been published: `publication_uncertain` with its
  // outcome, or a backend or transport failure during apply without one.
  | { kind: "uncertain"; message: string; outcome: Published | null }
  // A check that got no answer; nothing was written, retry is the user's call.
  | { kind: "retryable"; message: string }
  | { kind: "failed"; message: string };

export interface Applied {
  graph_revision: Uuid;
  operations: OperationEntry[];
}

export interface Picking {
  operation: string;
  role: PickRole;
}

export type Pending = "mint" | "check" | "apply" | "encode" | "decode" | null;

export interface DraftState {
  proposal: Proposal;
  selected: string | null;
  picking: Picking | null;
  evaluation: Evaluation;
  outcome: Outcome | null;
  // The plan ID names an earlier graph revision; only Replan clears it.
  stale: { expected: Uuid; actual: Uuid } | null;
  problem: Problem | null;
  // The last successful Apply, kept until dismissed while the draft is empty.
  applied: Applied | null;
  pending: Pending;
  // A transient note about the last edit, such as cleared references.
  notice: string | null;
}

export const initialDraft: DraftState = {
  proposal: { entries: [] },
  selected: null,
  picking: null,
  evaluation: { kind: "none" },
  outcome: null,
  stale: null,
  problem: null,
  applied: null,
  pending: null,
  notice: null,
};

// The draft without client state, exactly as it is sent.
export function proposalOf(state: DraftState): Proposal {
  const { plan_id, entries } = state.proposal;
  return plan_id === undefined ? { entries } : { plan_id, entries };
}

export function requestOf(state: DraftState): string {
  return JSON.stringify(proposalOf(state));
}

// Apply is enabled only for an accepted check of this very content.
export function applicable(state: DraftState): boolean {
  const { outcome } = state;
  return (
    outcome?.accepted === true &&
    state.stale === null &&
    state.pending === null &&
    state.proposal.entries.length > 0 &&
    outcome.request === requestOf(state)
  );
}

// ---- Names ----

function nameBase(ty_name: string): string {
  const base = ty_name.toLowerCase().replace(/[^a-z0-9_-]/g, "_");
  if (base === "") {
    return "op";
  }
  return /^[a-z_]/.test(base) ? base : `_${base}`;
}

function taken(entries: readonly ProposalEntry[]): Set<string> {
  return new Set(entries.map((entry) => entry.name));
}

// `<ty_name lowercased>_<n>` with the smallest n that is free.
export function defaultName(
  entries: readonly ProposalEntry[],
  ty_name: string,
): string {
  const used = taken(entries);
  const base = nameBase(ty_name);
  for (let n = 1; ; n++) {
    const candidate = `${base}_${n}`;
    if (!used.has(candidate)) {
      return candidate;
    }
  }
}

// `wanted` itself when free, else `wanted_2`, `wanted_3`, and so on.
export function uniqueName(
  entries: readonly ProposalEntry[],
  wanted: string,
): string {
  const used = taken(entries);
  if (!used.has(wanted)) {
    return wanted;
  }
  for (let n = 2; ; n++) {
    const candidate = `${wanted}_${n}`;
    if (!used.has(candidate)) {
      return candidate;
    }
  }
}

// ---- References ----

function renamed(
  reference: ProposalReference,
  from: string,
  to: string,
): ProposalReference {
  return "added" in reference && reference.added === from
    ? { added: to }
    : reference;
}

export function renameReferences(
  entries: readonly ProposalEntry[],
  from: string,
  to: string,
): ProposalEntry[] {
  return entries.map((entry) => {
    if (entry.kind === "edge") {
      return {
        ...entry,
        from: renamed(entry.from, from, to),
        to: renamed(entry.to, from, to),
      };
    }
    if (entry.predecessor !== undefined) {
      return { ...entry, predecessor: renamed(entry.predecessor, from, to) };
    }
    return entry;
  });
}

function refersTo(reference: ProposalReference, name: string): boolean {
  return "added" in reference && reference.added === name;
}

// Drops the named operation and clears every `added` reference to it: a
// node loses its predecessor and intent, an edge endpoint becomes unset.
export function removeOperation(
  entries: readonly ProposalEntry[],
  name: string,
): { entries: ProposalEntry[]; cleared: number } {
  let cleared = 0;
  const rest: ProposalEntry[] = [];
  for (const entry of entries) {
    if (entry.name === name) {
      continue;
    }
    if (entry.kind === "edge") {
      const from = refersTo(entry.from, name);
      const to = refersTo(entry.to, name);
      cleared += Number(from) + Number(to);
      rest.push({
        ...entry,
        from: from ? UNSET : entry.from,
        to: to ? UNSET : entry.to,
      });
    } else if (
      entry.predecessor !== undefined &&
      refersTo(entry.predecessor, name)
    ) {
      cleared += 1;
      const { predecessor: _p, intent: _i, ...root } = entry;
      rest.push(root);
    } else {
      rest.push(entry);
    }
  }
  return { entries: rest, cleared };
}

// Why an `added` reference would fail: it names no operation, or a later one.
export function referenceIssue(
  entries: readonly ProposalEntry[],
  index: number,
  reference: ProposalReference,
): "missing" | "later" | null {
  if (!("added" in reference)) {
    return null;
  }
  const target = entries.findIndex((entry) => entry.name === reference.added);
  if (target < 0) {
    return "missing";
  }
  return target < index ? null : "later";
}

// The first reason the draft cannot be sent: an unset endpoint. Everything
// else is the server's judgment.
export function incomplete(
  entries: readonly ProposalEntry[],
): { operation: string; message: string } | null {
  for (const entry of entries) {
    if (entry.kind === "edge") {
      for (const end of ["from", "to"] as const) {
        if (isUnset(entry[end])) {
          return {
            operation: entry.name,
            message: `edge ${entry.name} has no ${end} node yet`,
          };
        }
      }
    } else if (entry.predecessor !== undefined && isUnset(entry.predecessor)) {
      return {
        operation: entry.name,
        message: `node ${entry.name} has no predecessor picked yet`,
      };
    }
  }
  return null;
}

// Appends imported operations after the draft's own, renaming names already
// in use with a numeric suffix. An imported `added` reference resolves among
// the imported operations before it, as it would in its own file, and
// follows that operation's final name; the draft's operations never pointed
// at an imported name, so they are untouched.
export function appendOperations(
  entries: readonly ProposalEntry[],
  imported: readonly ProposalEntry[],
): { entries: ProposalEntry[]; renamed: [string, string][] } {
  const renames: [string, string][] = [];
  const combined = [...entries];
  // Original name to final name, for the imported operations seen so far.
  const finals = new Map<string, string>();
  const follow = (reference: ProposalReference): ProposalReference => {
    if (!("added" in reference)) {
      return reference;
    }
    const final = finals.get(reference.added);
    return final === undefined ? reference : { added: final };
  };
  for (const entry of imported) {
    const followed: ProposalEntry =
      entry.kind === "edge"
        ? { ...entry, from: follow(entry.from), to: follow(entry.to) }
        : entry.predecessor === undefined
          ? entry
          : { ...entry, predecessor: follow(entry.predecessor) };
    const name = uniqueName(combined, entry.name);
    if (name !== entry.name) {
      renames.push([entry.name, name]);
    }
    finals.set(entry.name, name);
    combined.push({ ...followed, name });
  }
  return { entries: combined, renamed: renames };
}

// ---- Reducer ----

export type DraftAction =
  // Selects the new operation unless `select` is false, as for the kept
  // edges a revision adds beside itself.
  | { kind: "add"; entry: ProposalEntry; after?: string; select?: boolean }
  | { kind: "replace"; name: string; entry: ProposalEntry }
  | { kind: "rename"; name: string; to: string }
  | { kind: "move"; name: string; by: -1 | 1 }
  | { kind: "duplicate"; name: string }
  | { kind: "remove"; name: string }
  | { kind: "clear" }
  | { kind: "import"; proposal: Proposal; mode: "replace" | "append" }
  | { kind: "set_plan_id"; plan_id: PlanId }
  | { kind: "replan"; plan_id: PlanId }
  | { kind: "evaluation"; evaluation: Evaluation }
  | { kind: "pending"; pending: Pending }
  | {
      kind: "checked";
      result: CheckResult;
      request: string;
      evaluation: Evaluation;
    }
  | { kind: "applied"; result: ApplyResult }
  | {
      kind: "rejected";
      report: ProposalReport;
      request: string;
      evaluation: Evaluation;
    }
  | { kind: "stale"; expected: Uuid; actual: Uuid }
  | { kind: "problem"; problem: Problem | null }
  | { kind: "dismiss_applied" }
  | { kind: "notice"; notice: string | null }
  | { kind: "select"; name: string | null }
  | { kind: "pick"; picking: Picking | null }
  | { kind: "resolve_pick"; id: EntryId };

// Problems about the content go away with the next edit; the others stay
// until the next call or an explicit dismissal.
function survivesEdit(problem: Problem | null): Problem | null {
  switch (problem?.kind) {
    case "invalid_proposal":
    case "malformed_proposal":
    case "incomplete":
      return null;
    default:
      return problem;
  }
}

function edited(
  state: DraftState,
  entries: ProposalEntry[],
  more: Partial<DraftState> = {},
): DraftState {
  return {
    ...state,
    proposal: { ...state.proposal, entries },
    problem: survivesEdit(state.problem),
    notice: null,
    ...more,
  };
}

function withEntry(
  entries: readonly ProposalEntry[],
  index: number,
  entry: ProposalEntry,
): ProposalEntry[] {
  const copy = [...entries];
  copy[index] = entry;
  return copy;
}

function emptied(state: DraftState): DraftState {
  return {
    ...state,
    proposal: { entries: [] },
    selected: null,
    picking: null,
    outcome: null,
    stale: null,
    problem: null,
    notice: null,
  };
}

export function draftReducer(
  state: DraftState,
  action: DraftAction,
): DraftState {
  const { entries } = state.proposal;
  switch (action.kind) {
    case "add": {
      const name = uniqueName(entries, action.entry.name);
      const entry = { ...action.entry, name };
      const at =
        action.after === undefined
          ? -1
          : entries.findIndex((e) => e.name === action.after);
      const next =
        at < 0
          ? [...entries, entry]
          : [...entries.slice(0, at + 1), entry, ...entries.slice(at + 1)];
      return edited(state, next, {
        selected: action.select === false ? state.selected : name,
      });
    }
    case "replace": {
      const index = entries.findIndex((e) => e.name === action.name);
      if (index < 0) {
        return state;
      }
      let entry = action.entry;
      let next = entries;
      let notice: string | null = null;
      // Names are the operations' identities in the client, so a name already
      // in use is refused. A name outside the grammar is kept, as the user
      // typed it mid-edit; the editor flags it and the server rejects it.
      if (entry.name !== action.name) {
        if (entries.some((e) => e.name === entry.name)) {
          notice = `the name ${entry.name} is already used; kept ${action.name}`;
          entry = { ...entry, name: action.name };
        } else {
          next = renameReferences(entries, action.name, entry.name);
        }
      }
      const selected =
        state.selected === action.name ? entry.name : state.selected;
      const picking =
        state.picking?.operation === action.name
          ? { ...state.picking, operation: entry.name }
          : state.picking;
      return edited(state, withEntry(next, index, entry), {
        selected,
        picking,
        notice,
      });
    }
    case "rename": {
      const index = entries.findIndex((e) => e.name === action.name);
      const entry = entries[index];
      if (entry === undefined) {
        return state;
      }
      return draftReducer(state, {
        kind: "replace",
        name: action.name,
        entry: { ...entry, name: action.to },
      });
    }
    case "move": {
      const index = entries.findIndex((e) => e.name === action.name);
      const target = index + action.by;
      if (index < 0 || target < 0 || target >= entries.length) {
        return state;
      }
      const next = [...entries];
      const [moved] = next.splice(index, 1);
      if (moved === undefined) {
        return state;
      }
      next.splice(target, 0, moved);
      return edited(state, next);
    }
    case "duplicate": {
      const entry = entries.find((e) => e.name === action.name);
      if (entry === undefined) {
        return state;
      }
      return draftReducer(state, {
        kind: "add",
        entry: { ...entry, name: uniqueName(entries, entry.name) },
        after: action.name,
      });
    }
    case "remove": {
      if (!entries.some((e) => e.name === action.name)) {
        return state;
      }
      const { entries: next, cleared } = removeOperation(entries, action.name);
      return edited(state, next, {
        selected: state.selected === action.name ? null : state.selected,
        picking:
          state.picking?.operation === action.name ? null : state.picking,
        notice:
          cleared === 0
            ? null
            : `removed ${action.name} and cleared ${cleared} reference${cleared === 1 ? "" : "s"} to it`,
      });
    }
    case "clear":
      return emptied(state);
    case "import": {
      if (action.mode === "replace") {
        const plan_id = action.proposal.plan_id ?? state.proposal.plan_id;
        return {
          ...emptied(state),
          proposal:
            plan_id === undefined
              ? { entries: action.proposal.entries }
              : { plan_id, entries: action.proposal.entries },
        };
      }
      const { entries: next, renamed } = appendOperations(
        entries,
        action.proposal.entries,
      );
      return edited(state, next, {
        notice:
          renamed.length === 0
            ? null
            : `renamed ${renamed.map(([from, to]) => `${from} → ${to}`).join(", ")}`,
      });
    }
    case "set_plan_id":
      // A plan ID minted for a draft that was cleared meanwhile is dropped.
      if (entries.length === 0 || state.proposal.plan_id !== undefined) {
        return state;
      }
      return {
        ...state,
        proposal: { ...state.proposal, plan_id: action.plan_id },
      };
    case "replan":
      return {
        ...state,
        proposal: { ...state.proposal, plan_id: action.plan_id },
        outcome: null,
        stale: null,
        problem: null,
        pending: null,
      };
    case "evaluation":
      return { ...state, evaluation: action.evaluation };
    case "pending":
      return { ...state, pending: action.pending };
    case "checked":
      return {
        ...state,
        outcome: {
          source: "check",
          request: action.request,
          evaluation: action.evaluation,
          accepted: action.result.accepted,
          report: action.result.report,
        },
        stale: null,
        problem: null,
        pending: null,
      };
    case "applied":
      return {
        ...emptied(state),
        applied: {
          graph_revision: action.result.graph_revision,
          operations: action.result.report.operations,
        },
        pending: null,
      };
    case "rejected":
      return {
        ...state,
        outcome: {
          source: "apply",
          request: action.request,
          evaluation: action.evaluation,
          accepted: false,
          report: action.report,
        },
        stale: null,
        problem: null,
        pending: null,
      };
    case "stale":
      return {
        ...state,
        stale: { expected: action.expected, actual: action.actual },
        problem: null,
        pending: null,
      };
    case "problem":
      return { ...state, problem: action.problem, pending: null };
    case "dismiss_applied":
      return { ...state, applied: null };
    case "notice":
      return { ...state, notice: action.notice };
    case "select":
      return { ...state, selected: action.name };
    case "pick":
      return { ...state, picking: action.picking };
    case "resolve_pick": {
      const { picking } = state;
      if (picking === null) {
        return state;
      }
      const index = entries.findIndex((e) => e.name === picking.operation);
      const entry = entries[index];
      if (entry === undefined) {
        return { ...state, picking: null };
      }
      const reference: ProposalReference = { existing: action.id };
      let next: ProposalEntry;
      if (picking.role === "predecessor") {
        if (entry.kind !== "node") {
          return { ...state, picking: null };
        }
        // A predecessor needs an intent; revise is the common case and the
        // editor offers branch.
        next = {
          ...entry,
          predecessor: reference,
          intent: entry.intent ?? "revise",
        };
      } else {
        if (entry.kind !== "edge") {
          return { ...state, picking: null };
        }
        next = { ...entry, [picking.role]: reference };
      }
      return edited(state, withEntry(entries, index, next), {
        picking: null,
        selected: entry.name,
      });
    }
  }
}
