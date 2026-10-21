import type {
  ApplyResult,
  CheckResult,
  EntryId,
  PlanId,
  Proposal,
  ProposalEntry,
  ProposalReport,
  Uuid,
} from "../contract.ts";
import {
  adopted,
  edited,
  emptied,
  type Keep,
  keep,
  namesOf,
  published,
  removal,
  renamed,
  withEntry,
  withNotice,
} from "./edits.ts";
import {
  type DraftState,
  type Evaluation,
  frozen,
  type InFlight,
  type KeptEdge,
  type Notice,
  type Pending,
  type Picking,
  type Problem,
  type Uncertain,
  type Verdict,
} from "./model.ts";
import {
  appendOperations,
  appliedNames,
  forwardReferences,
  renameReferences,
  uniqueName,
} from "./operations.ts";
import type { Stored } from "./persist.ts";

export type DraftAction =
  // Selects the new operation unless `select` is false, as for the kept
  // edges a revision adds beside itself.
  | {
      kind: "add";
      entry: ProposalEntry;
      after?: string;
      select?: boolean;
      kept?: KeptEdge;
    }
  // A changed name renames the operation.
  | { kind: "replace"; name: string; entry: ProposalEntry }
  | { kind: "move"; name: string; by: -1 | 1 }
  | { kind: "duplicate"; name: string }
  | { kind: "remove"; names: string[] }
  | { kind: "clear" }
  | { kind: "import"; proposal: Proposal; mode: "replace" | "append" }
  // Kept edges for `operation` while it still revises `predecessor` (and,
  // when given, with `intent`); edges it already keeps are skipped.
  | ({ kind: "keep" } & Keep)
  // A read of current edges started for `operation`, whose answer is a
  // `keep` with the same `read`; null forgets a read that failed.
  | { kind: "reading"; read: number; operation: string | null }
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
  | { kind: "apply_started"; flight: InFlight }
  | { kind: "applied"; result: ApplyResult; request: string }
  | {
      kind: "rejected";
      report: ProposalReport;
      request: string;
      evaluation: Evaluation;
    }
  | { kind: "stale"; expected: Uuid; actual: Uuid }
  | { kind: "problem"; problem: Problem | null }
  | { kind: "uncertain"; uncertain: Uncertain }
  | { kind: "verdict"; verdict: Verdict }
  // The user's answer to an uncertain apply: its entries are in the graph,
  // or it was verified not to be.
  | { kind: "decide"; present: boolean }
  | { kind: "dismiss_applied" }
  | { kind: "notice"; text: string; offer?: Notice["offer"] }
  | { kind: "dismiss_notice"; id: number }
  | { kind: "select"; name: string | null }
  | { kind: "pick"; picking: Picking | null }
  | { kind: "resolve_pick"; id: EntryId }
  // Another tab's stored record of this draft; `refused` when this tab's
  // last change was not saved over it.
  | { kind: "adopt"; stored: Stored | null; refused?: boolean };

function sentOf(request: string): ProposalEntry[] {
  return (JSON.parse(request) as Proposal).entries;
}

// The actions that change the operations; none lands while the draft is
// frozen, so it stays what was sent. Kept edges and Replan say why they
// did not land.
const EDITS: ReadonlySet<DraftAction["kind"]> = new Set([
  "add",
  "replace",
  "move",
  "duplicate",
  "remove",
  "clear",
  "import",
  "resolve_pick",
  "decide",
]);

export function draftReducer(
  state: DraftState,
  action: DraftAction,
): DraftState {
  if (
    frozen(state) &&
    (EDITS.has(action.kind) ||
      (action.kind === "pick" && action.picking !== null))
  ) {
    return state;
  }
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
        kept:
          action.kept === undefined
            ? state.kept
            : { ...state.kept, [name]: action.kept },
      });
    }
    case "replace": {
      const index = entries.findIndex((e) => e.name === action.name);
      if (index < 0) {
        return state;
      }
      const { entry } = action;
      if (entry.name === action.name) {
        return edited(state, withEntry(entries, index, entry));
      }
      // Names are the operations' identities in the client, so a name
      // already in use is refused.
      if (entries.some((e) => e.name === entry.name)) {
        return withNotice(
          edited(
            state,
            withEntry(entries, index, { ...entry, name: action.name }),
          ),
          `the name ${entry.name} is already used; kept ${action.name}`,
        );
      }
      const next = renameReferences(entries, action.name, entry.name);
      return edited(
        renamed(state, action.name, entry.name),
        withEntry(next, index, entry),
      );
    }
    case "move": {
      const index = entries.findIndex((e) => e.name === action.name);
      const target = index + action.by;
      const moved = entries[index];
      if (moved === undefined || target < 0 || target >= entries.length) {
        return state;
      }
      const next = [...entries];
      next.splice(index, 1);
      next.splice(target, 0, moved);
      const before = new Set(forwardReferences(entries));
      const broken = forwardReferences(next).filter(
        (pair) => !before.has(pair),
      );
      const result = edited(state, next);
      return broken.length === 0
        ? result
        : withNotice(
            result,
            `moving ${action.name} left a reference to a later operation: ${broken.join(", ")}`,
          );
    }
    case "duplicate": {
      const entry = entries.find((e) => e.name === action.name);
      return entry === undefined
        ? state
        : draftReducer(state, {
            kind: "add",
            entry: { ...entry, name: uniqueName(entries, entry.name) },
            after: action.name,
          });
    }
    case "remove":
      return removal(state, action.names);
    case "clear":
      // Clearing is an explicit decision about the whole draft, an apply of
      // uncertain fate included.
      return { ...emptied(state), uncertain: null, notices: [] };
    case "import": {
      if (action.proposal.acknowledges !== undefined) {
        return withNotice(
          state,
          "a proposal that acknowledges a finding cannot be imported: acknowledgment is not supported in this prototype",
        );
      }
      const base = action.mode === "replace" ? [] : entries;
      const { entries: next, renamed: names } = appendOperations(
        base,
        action.proposal.entries,
      );
      // A replaced draft takes the file's plan ID or a fresh one, never the
      // old draft's, which named another proposal.
      const { plan_id } = action.proposal;
      const result =
        action.mode === "replace"
          ? {
              ...emptied(state),
              proposal:
                plan_id === undefined
                  ? { entries: next }
                  : { plan_id, entries: next },
            }
          : edited(state, next);
      return names.length === 0
        ? result
        : withNotice(
            result,
            `renamed ${names.map(([from, to]) => `${from} → ${to}`).join(", ")}`,
          );
    }
    case "keep":
      return keep(state, action);
    case "reading": {
      const { [action.read]: _, ...reads } = state.reads;
      return {
        ...state,
        reads:
          action.operation === null
            ? reads
            : { ...reads, [action.read]: action.operation },
      };
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
      // Dropped while an apply may have been published or is in flight; the
      // mint it waited for is over either way.
      if (state.uncertain !== null || frozen(state)) {
        return state.pending === "mint" ? { ...state, pending: null } : state;
      }
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
          names: namesOf(action.request),
          evaluation: action.evaluation,
          accepted: action.result.accepted,
          report: action.result.report,
          spent: false,
        },
        stale: null,
        problem: null,
        pending: null,
      };
    case "apply_started":
      return {
        ...state,
        inFlight: action.flight,
        pending: "apply",
        problem: null,
      };
    case "applied": {
      const { operations } = action.result.report;
      return {
        ...published(state, action.request, operations),
        applied: {
          graph_revision: action.result.graph_revision,
          operations,
          report: action.result.report,
          names: appliedNames(sentOf(action.request), operations, state.kept),
        },
      };
    }
    case "rejected":
      return {
        ...state,
        outcome: {
          source: "apply",
          request: action.request,
          names: namesOf(action.request),
          evaluation: action.evaluation,
          accepted: false,
          report: action.report,
          spent: false,
        },
        stale: null,
        problem: null,
        inFlight: null,
        pending: null,
      };
    case "stale":
      return {
        ...state,
        stale: { expected: action.expected, actual: action.actual },
        problem: null,
        inFlight: null,
        pending: null,
      };
    case "problem":
      return {
        ...state,
        problem: action.problem,
        inFlight: null,
        pending: null,
      };
    case "uncertain":
      return {
        ...state,
        uncertain: action.uncertain,
        outcome:
          state.outcome === null ? null : { ...state.outcome, spent: true },
        problem: null,
        inFlight: null,
        pending: null,
      };
    case "verdict":
      return state.uncertain === null
        ? state
        : {
            ...state,
            uncertain: { ...state.uncertain, verdict: action.verdict },
          };
    case "decide": {
      const { uncertain } = state;
      if (uncertain === null) {
        return state;
      }
      if (!action.present) {
        return { ...state, uncertain: null };
      }
      const outcome =
        uncertain.outcome?.kind === "applied" ? uncertain.outcome : null;
      const operations =
        outcome?.report.operations ??
        (uncertain.verdict?.kind === "present"
          ? uncertain.verdict.operations
          : null);
      return {
        ...published(state, uncertain.request, operations),
        uncertain: null,
        applied:
          operations === null
            ? state.applied
            : {
                graph_revision: outcome?.graph_revision ?? null,
                operations,
                report: outcome?.report ?? null,
                names: appliedNames(
                  sentOf(uncertain.request),
                  operations,
                  state.kept,
                ),
              },
      };
    }
    case "dismiss_applied":
      return { ...state, applied: null };
    case "notice":
      return withNotice(state, action.text, action.offer ?? null);
    case "dismiss_notice":
      return {
        ...state,
        notices: state.notices.filter((notice) => notice.id !== action.id),
      };
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
      let next: ProposalEntry;
      if (picking.role === "predecessor") {
        if (entry.kind !== "node") {
          return { ...state, picking: null };
        }
        // A predecessor needs an intent; revise is the common case and the
        // editor offers branch.
        next = {
          ...entry,
          predecessor: { existing: action.id },
          intent: entry.intent ?? "revise",
        };
      } else {
        if (entry.kind !== "edge") {
          return { ...state, picking: null };
        }
        next = { ...entry, [picking.role]: { existing: action.id } };
      }
      return edited(state, withEntry(entries, index, next), {
        picking: null,
        selected: entry.name,
      });
    }
    case "adopt":
      return adopted(state, action.stored, action.refused);
  }
}
