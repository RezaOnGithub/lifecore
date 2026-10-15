import type {
  EntryId,
  Proposal,
  ProposalEntry,
  ProposalReference,
  Published,
} from "../contract.ts";
import {
  type DraftState,
  type InFlight,
  initialDraft,
  type KeptEdge,
  type Uncertain,
} from "./model.ts";

// One draft per database: the key carries the database id, so another
// database's draft is never restored into this one.
export function storageKey(database: string): string {
  return `lifecore.draft.${database}`;
}

// What survives a reload: the proposal, the provenance of its kept edges,
// an apply of uncertain fate, and the marker of an apply in flight.
export interface Stored {
  proposal: Proposal;
  kept: Record<string, KeptEdge>;
  uncertain: Omit<Uncertain, "verdict"> | null;
  in_flight: InFlight | null;
}

type Store = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isId(value: unknown): value is EntryId {
  return (
    isRecord(value) &&
    typeof value.origin === "string" &&
    typeof value.position === "string"
  );
}

function isReference(value: unknown): value is ProposalReference {
  if (!isRecord(value)) {
    return false;
  }
  if ("added" in value) {
    return typeof value.added === "string";
  }
  return isId(value.existing);
}

// The stored shape is this client's own, so the check is structural only:
// names, types, and payload contents are the server's to judge, and a
// rejected part drops the whole draft rather than restoring half of it.
function readEntry(value: unknown): ProposalEntry | null {
  if (
    !isRecord(value) ||
    typeof value.name !== "string" ||
    typeof value.ty_name !== "string"
  ) {
    return null;
  }
  if (value.kind === "node") {
    if (!isRecord(value.payload)) {
      return null;
    }
    if (value.predecessor !== undefined && !isReference(value.predecessor)) {
      return null;
    }
    if (
      value.intent !== undefined &&
      value.intent !== "revise" &&
      value.intent !== "branch"
    ) {
      return null;
    }
    return value as unknown as ProposalEntry;
  }
  if (value.kind === "edge") {
    return isReference(value.from) && isReference(value.to)
      ? (value as unknown as ProposalEntry)
      : null;
  }
  return null;
}

export function readProposal(value: unknown): Proposal | null {
  if (!isRecord(value) || !Array.isArray(value.entries)) {
    return null;
  }
  const entries: ProposalEntry[] = [];
  const names = new Set<string>();
  for (const item of value.entries) {
    const entry = readEntry(item);
    if (entry === null || names.has(entry.name)) {
      return null;
    }
    names.add(entry.name);
    entries.push(entry);
  }
  if (value.plan_id === undefined) {
    return { entries };
  }
  return typeof value.plan_id === "string"
    ? { plan_id: value.plan_id, entries }
    : null;
}

function readKept(value: unknown): Record<string, KeptEdge> | null {
  if (!isRecord(value)) {
    return null;
  }
  const kept: Record<string, KeptEdge> = {};
  for (const [name, edge] of Object.entries(value)) {
    if (
      !isRecord(edge) ||
      typeof edge.revision !== "string" ||
      !isId(edge.predecessor) ||
      !isId(edge.edge)
    ) {
      return null;
    }
    kept[name] = {
      revision: edge.revision,
      predecessor: edge.predecessor,
      edge: edge.edge,
    };
  }
  return kept;
}

function readFlight(value: unknown): InFlight | null {
  if (
    !isRecord(value) ||
    (value.plan_id !== null && typeof value.plan_id !== "string") ||
    typeof value.request !== "string" ||
    (value.base_revision !== null && typeof value.base_revision !== "string") ||
    typeof value.base_end !== "string"
  ) {
    return null;
  }
  return {
    plan_id: value.plan_id,
    request: value.request,
    base_revision: value.base_revision,
    base_end: value.base_end,
  };
}

function readUncertain(value: unknown): Stored["uncertain"] {
  const flight = readFlight(value);
  if (
    flight === null ||
    !isRecord(value) ||
    typeof value.error !== "string" ||
    typeof value.message !== "string" ||
    (value.outcome !== null && !isRecord(value.outcome))
  ) {
    return null;
  }
  return {
    ...flight,
    error: value.error,
    message: value.message,
    outcome: value.outcome as Published | null,
  };
}

export function readStored(value: unknown): Stored | null {
  if (!isRecord(value)) {
    return null;
  }
  const proposal = readProposal(value.proposal);
  const kept = readKept(value.kept);
  const uncertain =
    value.uncertain === null ? null : readUncertain(value.uncertain);
  const in_flight =
    value.in_flight === null ? null : readFlight(value.in_flight);
  if (
    proposal === null ||
    kept === null ||
    (value.uncertain !== null && uncertain === null) ||
    (value.in_flight !== null && in_flight === null)
  ) {
    return null;
  }
  return { proposal, kept, uncertain, in_flight };
}

export function storedOf(state: DraftState): Stored {
  const { uncertain } = state;
  return {
    proposal: state.proposal,
    kept: state.kept,
    uncertain:
      uncertain === null
        ? null
        : {
            plan_id: uncertain.plan_id,
            request: uncertain.request,
            base_revision: uncertain.base_revision,
            base_end: uncertain.base_end,
            error: uncertain.error,
            message: uncertain.message,
            outcome: uncertain.outcome,
          },
    in_flight: state.inFlight,
  };
}

// The stored parts change only through these fields, so comparing them by
// identity tells whether a dispatch needs a save.
export function storedChanged(before: DraftState, after: DraftState): boolean {
  return (
    before.proposal !== after.proposal ||
    before.kept !== after.kept ||
    before.uncertain !== after.uncertain ||
    before.inFlight !== after.inFlight
  );
}

// An apply that was in flight when the page went away may have been
// published: the draft comes back uncertain, never ready to apply again.
// Check results are not stored, so Apply needs a new check either way.
export function restoredState(stored: Stored | null): DraftState {
  if (stored === null) {
    return initialDraft;
  }
  const { proposal, kept, uncertain, in_flight } = stored;
  return {
    ...initialDraft,
    proposal,
    kept,
    uncertain:
      uncertain !== null
        ? { ...uncertain, verdict: null }
        : in_flight === null
          ? null
          : {
              ...in_flight,
              error: "interrupted",
              message:
                "the page was closed or reloaded while this draft was being applied",
              outcome: null,
              verdict: null,
            },
  };
}

// Null for a missing, unreadable, or malformed draft.
export function loadDraft(
  store: Pick<Store, "getItem">,
  database: string,
): Stored | null {
  let text: string | null;
  try {
    text = store.getItem(storageKey(database));
  } catch {
    return null;
  }
  if (text === null) {
    return null;
  }
  try {
    return readStored(JSON.parse(text));
  } catch {
    return null;
  }
}

// A draft with nothing to keep leaves nothing behind. Storage failures
// (quota, private mode) are swallowed: the draft lives on in memory either
// way.
export function saveDraft(store: Store, database: string, stored: Stored) {
  try {
    if (
      stored.proposal.entries.length === 0 &&
      stored.uncertain === null &&
      stored.in_flight === null
    ) {
      store.removeItem(storageKey(database));
    } else {
      store.setItem(storageKey(database), JSON.stringify(stored));
    }
  } catch {
    // Nothing to do: the next save will try again.
  }
}
