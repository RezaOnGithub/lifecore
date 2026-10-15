import type {
  EntryId,
  IncidentEdge,
  OperationEntry,
  Proposal,
  ProposalEntry,
  RevisionIntent,
} from "../contract.ts";
import type { DraftState, KeptEdge, Notice, Problem } from "./model.ts";
import { removeOperations, uniqueName, withoutApplied } from "./operations.ts";
import { keptBy, reconnection, stillRevises } from "./reconnect.ts";

// The state transitions the reducer composes.

export interface Keep {
  operation: string;
  predecessor: EntryId;
  intent?: RevisionIntent;
  edges: IncidentEdge[];
}

// Problems about the content go away with the next edit; the others stay
// until the next call or an explicit dismissal.
export function survivesEdit(problem: Problem | null): Problem | null {
  switch (problem?.kind) {
    case "invalid_proposal":
    case "malformed_proposal":
    case "incomplete":
      return null;
    default:
      return problem;
  }
}

export function edited(
  state: DraftState,
  entries: ProposalEntry[],
  more: Partial<DraftState> = {},
): DraftState {
  return {
    ...state,
    proposal: { ...state.proposal, entries },
    problem: survivesEdit(state.problem),
    ...more,
  };
}

export function withNotice(
  state: DraftState,
  text: string,
  offer: Notice["offer"] = null,
): DraftState {
  return {
    ...state,
    notices: [...state.notices, { id: state.nextNotice, text, offer }],
    nextNotice: state.nextNotice + 1,
  };
}

export function withEntry(
  entries: readonly ProposalEntry[],
  index: number,
  entry: ProposalEntry,
): ProposalEntry[] {
  const copy = [...entries];
  copy[index] = entry;
  return copy;
}

export function namesOf(request: string): string[] {
  return (JSON.parse(request) as Proposal).entries.map((entry) => entry.name);
}

// Follows a rename through the client state keyed by operation name.
export function renamed(
  state: DraftState,
  from: string,
  to: string,
): DraftState {
  const follow = (name: string) => (name === from ? to : name);
  const kept: Record<string, KeptEdge> = {};
  for (const [name, edge] of Object.entries(state.kept)) {
    kept[follow(name)] = { ...edge, revision: follow(edge.revision) };
  }
  return {
    ...state,
    kept,
    selected: state.selected === null ? null : follow(state.selected),
    picking:
      state.picking === null
        ? null
        : { ...state.picking, operation: follow(state.picking.operation) },
    outcome:
      state.outcome === null
        ? null
        : { ...state.outcome, names: state.outcome.names.map(follow) },
  };
}

export function withoutKept(
  kept: Record<string, KeptEdge>,
  removed: ReadonlySet<string>,
): Record<string, KeptEdge> {
  return Object.fromEntries(
    Object.entries(kept).filter(([name]) => !removed.has(name)),
  );
}

export function emptied(state: DraftState): DraftState {
  return {
    ...state,
    proposal: { entries: [] },
    kept: {},
    selected: null,
    picking: null,
    outcome: null,
    stale: null,
    problem: null,
  };
}

// Takes out the operations of a published request. What is left needs a new
// plan ID, since the base moved, and a new check.
export function published(
  state: DraftState,
  request: string,
  operations: OperationEntry[] | null,
): DraftState {
  const sent = (JSON.parse(request) as Proposal).entries;
  const { entries, removed } = withoutApplied(
    state.proposal.entries,
    sent,
    operations,
  );
  const next: DraftState = {
    ...state,
    proposal: { entries },
    kept: withoutKept(state.kept, removed),
    selected:
      state.selected !== null && removed.has(state.selected)
        ? null
        : state.selected,
    picking:
      state.picking !== null && removed.has(state.picking.operation)
        ? null
        : state.picking,
    outcome: null,
    stale: null,
    problem: null,
    inFlight: null,
    pending: null,
  };
  return entries.length === 0
    ? next
    : withNotice(
        next,
        `kept ${entries.length} operation${entries.length === 1 ? "" : "s"} that the applied change did not carry; they need a new plan ID and check`,
      );
}

// Names in a notice, cut short when there are many.
function listed(names: readonly string[]): string {
  return names.length <= 4
    ? names.join(", ")
    : `${names.slice(0, 3).join(", ")}, and ${names.length - 3} more`;
}

export function removal(
  state: DraftState,
  names: readonly string[],
): DraftState {
  const gone = new Set(
    names.filter((name) => state.proposal.entries.some((e) => e.name === name)),
  );
  if (gone.size === 0) {
    return state;
  }
  const { entries, affected, edges } = removeOperations(
    state.proposal.entries,
    [...gone],
  );
  const next = edited(state, entries, {
    kept: withoutKept(state.kept, gone),
    selected:
      state.selected !== null && gone.has(state.selected)
        ? null
        : state.selected,
    picking:
      state.picking !== null && gone.has(state.picking.operation)
        ? null
        : state.picking,
  });
  if (affected.length === 0) {
    return next;
  }
  return withNotice(
    next,
    `removed ${listed([...gone])}; ${listed(affected)} referred to ${gone.size === 1 ? "it" : "them"} and must be picked again`,
    edges.length === 0
      ? null
      : {
          label: `Remove ${edges.length === 1 ? "that edge" : `those ${edges.length} edges`} too`,
          remove: edges,
        },
  );
}

// Inserts the kept edges for `operation` after it and the kept edges already
// beside it, so they stay one group in the relationships' order.
export function keep(state: DraftState, action: Keep): DraftState {
  const { entries } = state.proposal;
  const index = entries.findIndex((e) => e.name === action.operation);
  const entry = entries[index];
  if (
    !stillRevises(entry, action.predecessor) ||
    (action.intent !== undefined &&
      entry?.kind === "node" &&
      entry.intent !== action.intent)
  ) {
    return action.edges.length === 0
      ? state
      : withNotice(
          state,
          `the current edges of @${action.predecessor.position} arrived after ${action.operation} was removed or changed; no edge was kept`,
        );
  }
  let at = index + 1;
  while (
    entries[at] !== undefined &&
    state.kept[entries[at]?.name ?? ""]?.revision === action.operation
  ) {
    at += 1;
  }
  const next = [...entries];
  const kept = { ...state.kept };
  for (const incident of action.edges) {
    if (keptBy(next, action.operation, incident) !== null) {
      continue;
    }
    const edge = reconnection(next, action.operation, incident);
    const name = uniqueName(next, edge.name);
    next.splice(at, 0, { ...edge, name });
    kept[name] = {
      revision: action.operation,
      predecessor: action.predecessor,
      edge: incident.edge.id,
    };
    at += 1;
  }
  return next.length === entries.length ? state : edited(state, next, { kept });
}
