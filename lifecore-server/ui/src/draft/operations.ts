import type {
  DecimalString,
  EntryChange,
  EntryId,
  EntryRecord,
  OperationEntry,
  ProposalEntry,
  ProposalReference,
  Reference,
} from "../contract.ts";
import { sameId } from "../model/labels.ts";
import { isUnset, type KeptEdge, sameReference, UNSET } from "./model.ts";

// Pure functions over the ordered operations of a draft.

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

function mapReferences(
  entry: ProposalEntry,
  map: (reference: ProposalReference) => ProposalReference,
): ProposalEntry {
  if (entry.kind === "edge") {
    return { ...entry, from: map(entry.from), to: map(entry.to) };
  }
  return entry.predecessor === undefined
    ? entry
    : { ...entry, predecessor: map(entry.predecessor) };
}

function refersTo(reference: ProposalReference, name: string): boolean {
  return "added" in reference && reference.added === name;
}

export function renameReferences(
  entries: readonly ProposalEntry[],
  from: string,
  to: string,
): ProposalEntry[] {
  return entries.map((entry) =>
    mapReferences(entry, (reference) =>
      refersTo(reference, from) ? { added: to } : reference,
    ),
  );
}

export interface Removal {
  entries: ProposalEntry[];
  // Operations left with an unset reference, in draft order.
  affected: string[];
  // The edges among them, which lost an end.
  edges: string[];
}

// Drops the named operations. Every `added` reference to one of them becomes
// unset, a predecessor included: the node keeps its intent and must be picked
// again rather than silently turning into a root.
export function removeOperations(
  entries: readonly ProposalEntry[],
  names: readonly string[],
): Removal {
  const gone = new Set(names);
  const affected: string[] = [];
  const edges: string[] = [];
  const rest: ProposalEntry[] = [];
  for (const entry of entries) {
    if (gone.has(entry.name)) {
      continue;
    }
    let cleared = false;
    const kept = mapReferences(entry, (reference) => {
      if ("added" in reference && gone.has(reference.added)) {
        cleared = true;
        return UNSET;
      }
      return reference;
    });
    if (cleared) {
      affected.push(entry.name);
      if (entry.kind === "edge") {
        edges.push(entry.name);
      }
    }
    rest.push(kept);
  }
  return { entries: rest, affected, edges };
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

function referencesOf(entry: ProposalEntry): ProposalReference[] {
  if (entry.kind === "edge") {
    return [entry.from, entry.to];
  }
  return entry.predecessor === undefined ? [] : [entry.predecessor];
}

// `referrer → referenced` for every `added` reference to a later operation.
export function forwardReferences(entries: readonly ProposalEntry[]): string[] {
  const found: string[] = [];
  entries.forEach((entry, index) => {
    for (const reference of referencesOf(entry)) {
      if (
        "added" in reference &&
        referenceIssue(entries, index, reference) === "later"
      ) {
        found.push(`${entry.name} → ${reference.added}`);
      }
    }
  });
  return found;
}

// The first reason the draft cannot be sent: an unset endpoint or
// predecessor. Everything else is the server's judgment.
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
// in use, earlier imported ones included, with a numeric suffix. An imported
// `added` reference resolves among the imported operations before it, as it
// would in its own file, and follows that operation's final name; the
// draft's operations never pointed at an imported name, so they are untouched.
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
    const followed = mapReferences(entry, follow);
    const name = uniqueName(combined, entry.name);
    if (name !== entry.name) {
      renames.push([entry.name, name]);
    }
    finals.set(entry.name, name);
    combined.push({ ...followed, name });
  }
  return { entries: combined, renamed: renames };
}

// A suggestion's actions as operations to append, each with a default name
// free in `entries` and among the earlier actions. `{added: n}` addresses
// the n-th action and becomes that operation's name; an index outside the
// actions, which a valid suggestion never has, is left unset to be picked.
export function suggestedOperations(
  entries: readonly ProposalEntry[],
  actions: readonly EntryChange[],
): ProposalEntry[] {
  const combined = [...entries];
  const names: string[] = [];
  for (const action of actions) {
    const base =
      action.kind === "edge" && action.ty_name === "" ? "edge" : action.ty_name;
    const name = defaultName(combined, base);
    names.push(name);
    // A placeholder that only reserves the name.
    combined.push({ name, kind: "node", ty_name: "", payload: {} });
  }
  const reference = (r: Reference): ProposalReference => {
    if ("existing" in r) {
      return { existing: r.existing };
    }
    const name = Number.isInteger(r.added) ? names[r.added] : undefined;
    return name === undefined ? UNSET : { added: name };
  };
  return actions.map((action, index): ProposalEntry => {
    const name = names[index] as string;
    if (action.kind === "edge") {
      return {
        name,
        kind: "edge",
        ty_name: action.ty_name,
        from: reference(action.from),
        to: reference(action.to),
      };
    }
    const { ty_name, predecessor, intent, payload } = action;
    return {
      name,
      kind: "node",
      ty_name,
      ...(predecessor === undefined
        ? {}
        : { predecessor: reference(predecessor) }),
      ...(intent === undefined ? {} : { intent }),
      payload,
    };
  });
}

// ---- Supersession inside the draft ----

// The operations that supersede what `reference` names, in draft order.
function supersessors(
  entries: readonly ProposalEntry[],
  reference: ProposalReference,
): { index: number; entry: Extract<ProposalEntry, { kind: "node" }> }[] {
  const found: {
    index: number;
    entry: Extract<ProposalEntry, { kind: "node" }>;
  }[] = [];
  entries.forEach((entry, index) => {
    if (
      entry.kind === "node" &&
      entry.predecessor !== undefined &&
      sameReference(entry.predecessor, reference)
    ) {
      found.push({ index, entry });
    }
  });
  return found;
}

// The newest revise successor of `reference` among the operations before
// `before`, following chains of revisions; null when nothing there revises it.
export function reviseSuccessor(
  entries: readonly ProposalEntry[],
  before: number,
  reference: ProposalReference,
): string | null {
  const earlier = entries.slice(0, Math.max(before, 0));
  let found: string | null = null;
  let target = reference;
  const seen = new Set<string>();
  for (;;) {
    const next = supersessors(earlier, target).find(
      ({ entry }) => entry.intent === "revise" && !seen.has(entry.name),
    );
    if (next === undefined) {
      return found;
    }
    found = next.entry.name;
    seen.add(found);
    target = { added: found };
  }
}

// An edge whose end a draft operation supersedes is historical from that
// operation on (SPEC §5): already when it is appended if the successor comes
// first, later otherwise.
export function endpointWarnings(
  entries: readonly ProposalEntry[],
  index: number,
): string[] {
  const entry = entries[index];
  if (entry === undefined || entry.kind !== "edge") {
    return [];
  }
  const warnings: string[] = [];
  for (const end of ["from", "to"] as const) {
    const reference = entry[end];
    if (isUnset(reference)) {
      continue;
    }
    for (const other of supersessors(entries, reference)) {
      const verb = other.entry.intent === "branch" ? "branched" : "revised";
      warnings.push(
        other.index < index
          ? `the ${end} node is ${verb} earlier in this draft by ${other.entry.name}; this edge would be historical when added`
          : `the ${end} node is ${verb} later in this draft by ${other.entry.name}; this edge will be historical`,
      );
    }
  }
  return warnings;
}

// A kept edge whose revision no longer revises the node it was kept from.
export function keptWarning(
  entries: readonly ProposalEntry[],
  kept: KeptEdge | undefined,
): string | null {
  if (kept === undefined) {
    return null;
  }
  const revision = entries.find((entry) => entry.name === kept.revision);
  if (revision === undefined) {
    return `kept for ${kept.revision}, which is no longer in the draft`;
  }
  if (
    revision.kind !== "node" ||
    revision.predecessor === undefined ||
    !sameReference(revision.predecessor, { existing: kept.predecessor })
  ) {
    return `kept from @${kept.predecessor.position}, which ${kept.revision} no longer revises`;
  }
  return null;
}

// ---- Content comparison ----

// JSON with object keys sorted, so equal content compares equal whatever
// order an edit left its keys in.
export function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map(canonicalJson).join(",")}]`;
  }
  if (typeof value === "object" && value !== null) {
    const record = value as Record<string, unknown>;
    return `{${Object.keys(record)
      .sort()
      .filter((key) => record[key] !== undefined)
      .map((key) => `${JSON.stringify(key)}:${canonicalJson(record[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

// Removes the operations a published request carried, matched by name and
// content, and keeps the rest. A kept operation's `added` reference to a
// removed one becomes the entry it was assigned, or unset when the ids are
// unknown.
export function withoutApplied(
  entries: readonly ProposalEntry[],
  request: readonly ProposalEntry[],
  operations: readonly OperationEntry[] | null,
): { entries: ProposalEntry[]; removed: Set<string> } {
  const sent = new Map(
    request.map((entry) => [entry.name, canonicalJson(entry)]),
  );
  const removed = new Set(
    entries
      .filter((entry) => sent.get(entry.name) === canonicalJson(entry))
      .map((entry) => entry.name),
  );
  const assigned = new Map(
    (operations ?? []).map((operation) => [operation.name, operation.entry]),
  );
  const rest = entries
    .filter((entry) => !removed.has(entry.name))
    .map((entry) =>
      mapReferences(entry, (reference) => {
        if (!("added" in reference) || !removed.has(reference.added)) {
          return reference;
        }
        const id = assigned.get(reference.added);
        return id === undefined ? UNSET : { existing: id };
      }),
    );
  return { entries: rest, removed };
}

// The stored entries a request would have become, matched at the positions
// it would have taken: `[base_end, base_end + n)` in request order, with the
// same kind, type, payload, and references. Null when any differs or is
// missing.
export function findApplied(
  request: readonly ProposalEntry[],
  base_end: DecimalString,
  records: readonly EntryRecord[],
): OperationEntry[] | null {
  const base = BigInt(base_end);
  const byPosition = new Map(
    records.map((record) => [record.id.position, record]),
  );
  const ids: EntryId[] = [];
  const resolve = (reference: ProposalReference): EntryId | undefined =>
    "existing" in reference
      ? reference.existing
      : ids[request.findIndex((entry) => entry.name === reference.added)];
  const same = (reference: ProposalReference, id: EntryId | undefined) => {
    const expected = resolve(reference);
    return expected !== undefined && id !== undefined && sameId(expected, id);
  };
  for (const [index, entry] of request.entries()) {
    const record = byPosition.get(String(base + BigInt(index)));
    if (
      record === undefined ||
      record.kind !== entry.kind ||
      record.ty_name !== entry.ty_name
    ) {
      return null;
    }
    if (record.kind === "node" && entry.kind === "node") {
      const predecessor =
        entry.predecessor === undefined
          ? record.predecessor === undefined
          : same(entry.predecessor, record.predecessor);
      if (
        !predecessor ||
        canonicalJson(record.payload) !== canonicalJson(entry.payload)
      ) {
        return null;
      }
    } else if (record.kind === "edge" && entry.kind === "edge") {
      if (!same(entry.from, record.from) || !same(entry.to, record.to)) {
        return null;
      }
    }
    ids.push(record.id);
  }
  return request.map((entry, index) => ({
    name: entry.name,
    entry: ids[index] as EntryId,
  }));
}

// The operation a decode path such as `entries[3].payload` points into.
export function pathOperation(
  path: string | undefined,
  names: readonly string[],
): string | null {
  const match = path === undefined ? null : /^entries\[(\d+)\]/.exec(path);
  return match === null ? null : (names[Number(match[1])] ?? null);
}
