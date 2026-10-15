import type {
  IncidentEdge,
  ProposalEntry,
  ProposalReference,
  RevisionIntent,
} from "../contract.ts";
import { sameId } from "../model/labels.ts";

// Revising a node makes its edges historical (SPEC §5); keeping one means an
// explicit new edge of the same type whose end at the revised node is the
// new operation and whose other end stays the existing node.

// Only current edges can be kept: a historical one already has a superseded
// endpoint.
export function reconnectable(edges: readonly IncidentEdge[]): IncidentEdge[] {
  return edges.filter(({ edge }) => edge.current);
}

export function keptEdgeName(
  operation: string,
  incident: IncidentEdge,
): string {
  return `${operation}_keep_${incident.edge.id.position}`;
}

export function reconnection(
  operation: string,
  incident: IncidentEdge,
): ProposalEntry {
  const { edge, direction, other } = incident;
  const self: ProposalReference = { added: operation };
  const kept: ProposalReference = { existing: other.id };
  return {
    name: keptEdgeName(operation, incident),
    kind: "edge",
    ty_name: edge.ty_name,
    from: direction === "outgoing" ? self : kept,
    to: direction === "outgoing" ? kept : self,
  };
}

// The edge operations a fresh revision starts with: every current edge for
// revise, none for branch.
export function reconnections(
  operation: string,
  edges: readonly IncidentEdge[],
  intent: RevisionIntent,
): ProposalEntry[] {
  return intent === "revise"
    ? reconnectable(edges).map((incident) => reconnection(operation, incident))
    : [];
}

// Whether `entry` keeps `incident` for `operation`: matched structurally, not
// by name, so a renamed operation or edge keeps its checkbox.
export function keepsEdge(
  entry: ProposalEntry,
  operation: string,
  incident: IncidentEdge,
): boolean {
  if (entry.kind !== "edge" || entry.ty_name !== incident.edge.ty_name) {
    return false;
  }
  const self = incident.direction === "outgoing" ? entry.from : entry.to;
  const kept = incident.direction === "outgoing" ? entry.to : entry.from;
  return (
    "added" in self &&
    self.added === operation &&
    "existing" in kept &&
    sameId(kept.existing, incident.other.id)
  );
}

export function keptBy(
  entries: readonly ProposalEntry[],
  operation: string,
  incident: IncidentEdge,
): ProposalEntry | null {
  return entries.find((entry) => keepsEdge(entry, operation, incident)) ?? null;
}
