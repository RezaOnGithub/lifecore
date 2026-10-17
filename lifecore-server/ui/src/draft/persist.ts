import type {
  EntryId,
  Proposal,
  ProposalEntry,
  ProposalReference,
  Published,
} from "../contract.ts";
import { isRecord } from "../model/guards.ts";
import {
  type DraftState,
  type InFlight,
  initialDraft,
  type KeptEdge,
  proposalOf,
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

export type Store = Pick<Storage, "getItem" | "setItem" | "removeItem">;

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

function isView(value: unknown): boolean {
  return isRecord(value) && typeof value.end_exclusive === "string";
}

// An applied outcome is what a decision that the entries are present takes
// out of the draft and reports, so its report needs the parts that reads:
// the assigned ids, the base, and the analysis's view and observations.
// Other kinds are shown by name only.
function isOutcome(value: unknown): value is Published {
  if (!isRecord(value) || typeof value.kind !== "string") {
    return false;
  }
  if (value.kind !== "applied") {
    return true;
  }
  const { report } = value;
  return (
    typeof value.graph_revision === "string" &&
    isRecord(report) &&
    Array.isArray(report.operations) &&
    report.operations.every(
      (operation) =>
        isRecord(operation) &&
        typeof operation.name === "string" &&
        isId(operation.entry),
    ) &&
    isView(report.base) &&
    isRecord(report.analysis) &&
    isView(report.analysis.view) &&
    Array.isArray(report.analysis.observations)
  );
}

function readUncertain(value: unknown): Stored["uncertain"] {
  const flight = readFlight(value);
  if (
    flight === null ||
    !isRecord(value) ||
    typeof value.error !== "string" ||
    typeof value.message !== "string" ||
    (value.outcome !== null && !isOutcome(value.outcome))
  ) {
    return null;
  }
  return {
    ...flight,
    error: value.error,
    message: value.message,
    outcome: value.outcome,
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

// Built member by member in the order `readStored` builds them, so a record
// read from another tab and saved again is the same text, and a tab tells
// another tab's write from its own by the text alone.
export function storedOf(state: DraftState): Stored {
  const { uncertain } = state;
  return {
    proposal: proposalOf(state),
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

// The stored text, null when there is none, undefined when the store
// refuses to be read.
export function storedText(
  store: Pick<Store, "getItem">,
  database: string,
): string | null | undefined {
  try {
    return store.getItem(storageKey(database));
  } catch {
    return undefined;
  }
}

// Null for a missing, unreadable, or malformed draft.
export function parseStored(text: string | null | undefined): Stored | null {
  if (text === null || text === undefined) {
    return null;
  }
  try {
    return readStored(JSON.parse(text));
  } catch {
    return null;
  }
}

export function loadDraft(
  store: Pick<Store, "getItem">,
  database: string,
): Stored | null {
  return parseStored(storedText(store, database));
}

export type Saved =
  // The text the key holds after the write, null for none.
  | { kind: "written"; text: string | null }
  // Another tab's marker is stored; `stored` is that tab's record.
  | { kind: "refused"; text: string; stored: Stored }
  | { kind: "failed" };

// A draft with nothing to keep leaves nothing behind. Storage failures
// (quota, private mode) are reported, never thrown: the draft lives on in
// memory either way, and the next save tries again.
export function saveDraft(
  store: Store,
  database: string,
  stored: Stored,
): Saved {
  try {
    if (
      stored.proposal.entries.length === 0 &&
      stored.uncertain === null &&
      stored.in_flight === null
    ) {
      store.removeItem(storageKey(database));
      return { kind: "written", text: null };
    }
    const text = JSON.stringify(stored);
    store.setItem(storageKey(database), text);
    return { kind: "written", text };
  } catch {
    return { kind: "failed" };
  }
}

// Markers compare by their JSON: both sides are built by `storedOf` or
// `readStored`, member by member in the same order.
export function sameMarker(a: object | null, b: object | null): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

// Whether `stored` carries an in-flight or uncertain marker that `state`
// does not hold: another tab's apply, which only that tab's answer or the
// decision of a user who was shown it may end.
export function foreignMarker(stored: Stored, state: DraftState): boolean {
  const own = storedOf(state);
  return (
    (stored.in_flight !== null &&
      !sameMarker(stored.in_flight, own.in_flight)) ||
    (stored.uncertain !== null && !sameMarker(stored.uncertain, own.uncertain))
  );
}

// Saves `next` unless another tab wrote since this one last wrote or read
// the key (`known`) and left a marker that `previous`, this tab's state
// before the change, did not hold. Without the check the last writer wins,
// and a tab could erase the marker of an apply in flight in another, so
// that closing that tab restores no uncertain record.
// TODO: two tabs that both write within the storage's cross-tab delay,
// such as two Apply clicks at once, can still overwrite each other's
// marker; a Web Lock held around the apply would close that gap.
export function saveGuarded(
  store: Store,
  database: string,
  known: string | null | undefined,
  previous: DraftState,
  next: DraftState,
): Saved {
  const text = storedText(store, database);
  if (typeof text === "string" && text !== known) {
    const stored = parseStored(text);
    if (stored !== null && foreignMarker(stored, previous)) {
      return { kind: "refused", text, stored };
    }
  }
  return saveDraft(store, database, storedOf(next));
}
