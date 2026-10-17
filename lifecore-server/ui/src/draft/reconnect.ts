import type {
  EntryId,
  IncidentEdge,
  ProposalEntry,
  ProposalReference,
} from "../contract.ts";
import { entryKey, sameId } from "../model/labels.ts";
import { sameReference } from "./model.ts";
import { reviseSuccessor } from "./operations.ts";

// Revising a node makes its edges historical (SPEC §5); keeping one means an
// explicit new edge of the same type whose end at the revised node is the
// new operation and whose other end is the other node, or what the draft
// already put in its place.

// Only current edges can be kept: a historical one already has a superseded
// endpoint. A self-loop listed once per direction is kept once.
export function reconnectable<T extends IncidentEdge>(
  edges: readonly T[],
): T[] {
  const seen = new Set<string>();
  return edges.filter(({ edge }) => {
    const key = entryKey(edge.id);
    if (!edge.current || seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}

function selfLoop({ edge }: IncidentEdge): boolean {
  return sameId(edge.from, edge.to);
}

export function keptEdgeName(
  operation: string,
  incident: IncidentEdge,
): string {
  return `${operation}_keep_${incident.edge.id.position}`;
}

// The other end of a kept edge for the revision at `index`: the revision
// itself for a self-loop, which it supersedes; else an earlier operation
// that revises the other node, whose original the draft supersedes before
// the edge lands; else the other node.
export function otherEnd(
  entries: readonly ProposalEntry[],
  index: number,
  operation: string,
  incident: IncidentEdge,
): ProposalReference {
  if (selfLoop(incident)) {
    return { added: operation };
  }
  const other: ProposalReference = { existing: incident.other };
  const successor = reviseSuccessor(entries, index, other);
  return successor === null ? other : { added: successor };
}

export function reconnection(
  entries: readonly ProposalEntry[],
  operation: string,
  incident: IncidentEdge,
): ProposalEntry {
  const index = entries.findIndex((entry) => entry.name === operation);
  const self: ProposalReference = { added: operation };
  const other = otherEnd(entries, index, operation, incident);
  const outgoing = incident.direction === "outgoing";
  return {
    name: keptEdgeName(operation, incident),
    kind: "edge",
    ty_name: incident.edge.ty_name,
    from: outgoing ? self : other,
    to: outgoing ? other : self,
  };
}

// Whether `entry` keeps `incident` for `operation`: matched structurally, not
// by name, so a renamed operation or edge keeps its checkbox. The other end
// may be the other node or the successor `otherEnd` would pick.
export function keepsEdge(
  entries: readonly ProposalEntry[],
  entry: ProposalEntry,
  operation: string,
  incident: IncidentEdge,
): boolean {
  if (entry.kind !== "edge" || entry.ty_name !== incident.edge.ty_name) {
    return false;
  }
  const outgoing = incident.direction === "outgoing";
  const self = outgoing ? entry.from : entry.to;
  const other = outgoing ? entry.to : entry.from;
  if (!sameReference(self, { added: operation })) {
    return false;
  }
  const index = entries.findIndex((e) => e.name === operation);
  return (
    sameReference(other, otherEnd(entries, index, operation, incident)) ||
    (!selfLoop(incident) && sameReference(other, { existing: incident.other }))
  );
}

export function keptBy(
  entries: readonly ProposalEntry[],
  operation: string,
  incident: IncidentEdge,
): ProposalEntry | null {
  return (
    entries.find((entry) => keepsEdge(entries, entry, operation, incident)) ??
    null
  );
}

// The operation still revises or branches `predecessor`.
export function stillRevises(
  entry: ProposalEntry | undefined,
  predecessor: EntryId,
): boolean {
  return (
    entry?.kind === "node" &&
    entry.predecessor !== undefined &&
    sameReference(entry.predecessor, { existing: predecessor })
  );
}
