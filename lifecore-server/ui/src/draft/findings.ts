import type {
  EntryId,
  MessageType,
  Observation,
  ProposalReport,
} from "../contract.ts";
import { sameId } from "../model/labels.ts";

// A subject or focus of a finding, with provisional ids mapped back to the
// operation that would create them.
export type Target =
  | { kind: "operation"; name: string }
  | { kind: "existing"; id: EntryId };

export interface Attached {
  observation: Observation;
  subjects: Target[];
  focus: Target | null;
}

export interface Attachment {
  // Keyed by the operation name as of the report, so a reordered draft keeps
  // its findings and a renamed or removed operation's end up in `unplaced`.
  operations: Map<string, Attached[]>;
  history: Attached[];
  initialization: Attached[];
  unplaced: Attached[];
}

// Attaches each observation to the operation that introduced it: the newest
// entry of the first prefix it was emitted at is at position
// `first.end_exclusive - 1`; at or past the base boundary that is the plan's
// operation `position - base.end_exclusive`, before it the finding already
// held in the committed history.
export function attachFindings(report: ProposalReport): Attachment {
  const base = BigInt(report.base.end_exclusive);
  const target = (id: EntryId): Target => {
    const operation = report.operations.find((op) => sameId(op.entry, id));
    return operation === undefined
      ? { kind: "existing", id }
      : { kind: "operation", name: operation.name };
  };
  const attachment: Attachment = {
    operations: new Map(report.operations.map((op) => [op.name, []])),
    history: [],
    initialization: [],
    unplaced: [],
  };
  for (const observation of report.analysis.observations) {
    const { diagnostic, evaluated_view } = observation;
    const attached: Attached = {
      observation,
      subjects: diagnostic.subjects.map(target),
      focus: diagnostic.focus === null ? null : target(diagnostic.focus),
    };
    if (evaluated_view.kind === "initialization") {
      attachment.initialization.push(attached);
      continue;
    }
    const newest = BigInt(evaluated_view.first.end_exclusive) - 1n;
    if (newest < base) {
      attachment.history.push(attached);
      continue;
    }
    const operation = report.operations[Number(newest - base)];
    const list =
      operation === undefined
        ? undefined
        : attachment.operations.get(operation.name);
    if (list === undefined) {
      attachment.unplaced.push(attached);
    } else {
      list.push(attached);
    }
  }
  return attachment;
}

export type Counts = { [K in MessageType]: number };

export function countFindings(findings: readonly Attached[]): Counts {
  const counts: Counts = { inconsistency: 0, warning: 0, suggestion: 0 };
  for (const { observation } of findings) {
    counts[observation.diagnostic.kind] += 1;
  }
  return counts;
}

// The per-operation status after a check: inconsistencies block, warnings
// are advisory, suggestions are not counted.
export function statusWords(counts: Counts): string {
  const words: string[] = [];
  if (counts.inconsistency > 0) {
    words.push(
      `${counts.inconsistency} inconsistenc${counts.inconsistency === 1 ? "y" : "ies"}`,
    );
  }
  if (counts.warning > 0) {
    words.push(`${counts.warning} warning${counts.warning === 1 ? "" : "s"}`);
  }
  return words.length === 0 ? "ok" : words.join(", ");
}
