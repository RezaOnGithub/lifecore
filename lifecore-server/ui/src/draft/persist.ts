import type {
  Proposal,
  ProposalEntry,
  ProposalReference,
} from "../contract.ts";

// One draft per database: the key carries the database id, so another
// database's draft is never restored into this one.
export function storageKey(database: string): string {
  return `lifecore.draft.${database}`;
}

type Store = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isReference(value: unknown): value is ProposalReference {
  if (!isRecord(value)) {
    return false;
  }
  if ("added" in value) {
    return typeof value.added === "string";
  }
  return (
    isRecord(value.existing) &&
    typeof value.existing.origin === "string" &&
    typeof value.existing.position === "string"
  );
}

// The stored shape is this client's own, so the check is structural only:
// names, types, and payload contents are the server's to judge (a draft may
// be saved mid-rename), and a rejected entry drops the whole draft rather
// than restoring half of it.
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

// Null for a missing, unreadable, or malformed draft.
export function loadDraft(store: Pick<Store, "getItem">, database: string) {
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
    return readProposal(JSON.parse(text));
  } catch {
    return null;
  }
}

// An empty draft leaves nothing behind. Storage failures (quota, private
// mode) are swallowed: the draft lives on in memory either way.
export function saveDraft(store: Store, database: string, proposal: Proposal) {
  try {
    if (proposal.entries.length === 0) {
      store.removeItem(storageKey(database));
    } else {
      store.setItem(storageKey(database), JSON.stringify(proposal));
    }
  } catch {
    // Nothing to do: the next save will try again.
  }
}
