import type {
  DecimalString,
  EntryId,
  MessageType,
  Observation,
  ProposalReport,
} from "../contract.ts";
import { sameId } from "../model/labels.ts";
import { counted } from "../model/words.ts";

// A subject or focus of a finding, with provisional ids mapped back to the
// index of the operation that would create them.
export type Target =
  | { kind: "operation"; index: number }
  | { kind: "existing"; id: EntryId };

export interface Attached {
  observation: Observation;
  subjects: Target[];
  focus: Target | null;
  // The boundaries of the first and last prefix it was emitted at; null for
  // a suite-level initialization finding.
  range: { first: DecimalString; last: DecimalString } | null;
  // The candidate's head, the boundary its range is read against.
  boundary: DecimalString;
  // Last emitted before the candidate's head. Not emitted is all it says:
  // nothing infers that the draft resolved it (SPEC §8.3).
  notAtHead: boolean;
}

export interface Attachment {
  // Keyed by the index of the operation in the checked proposal, whose
  // entry sits at `base + index`; `Outcome.names` names it in the draft.
  operations: Map<number, Attached[]>;
  // First emitted in the committed history and still at the base boundary.
  history: Attached[];
  // First emitted in the committed history and no longer at the base
  // boundary: the inconsistencies are listed, since they never fold away,
  // and the advisories only counted.
  ended: Attached[];
  endedAdvisories: number;
  initialization: Attached[];
  unplaced: Attached[];
}

const attachments = new WeakMap<ProposalReport, Attachment>();

// Attaches each observation to the operation that introduced it: the newest
// entry of the first prefix it was emitted at is at position
// `first.end_exclusive - 1`; at or past the base boundary that is the entry
// of the report operation at that position, before it the finding already
// held in the committed history. Reports are immutable, so the attachment
// is computed once per report.
export function attachFindings(report: ProposalReport): Attachment {
  const known = attachments.get(report);
  if (known !== undefined) {
    return known;
  }
  const base = BigInt(report.base.end_exclusive);
  const head = BigInt(report.analysis.view.end_exclusive);
  const indexAt = (position: bigint): number | null => {
    const found = report.operations.some(
      (op) => BigInt(op.entry.position) === position,
    );
    return found && position >= base ? Number(position - base) : null;
  };
  const target = (id: EntryId): Target => {
    const operation = report.operations.find((op) => sameId(op.entry, id));
    const index =
      operation === undefined
        ? null
        : indexAt(BigInt(operation.entry.position));
    return index === null
      ? { kind: "existing", id }
      : { kind: "operation", index };
  };
  const attachment: Attachment = {
    operations: new Map(),
    history: [],
    ended: [],
    endedAdvisories: 0,
    initialization: [],
    unplaced: [],
  };
  for (const observation of report.analysis.observations) {
    const { diagnostic, evaluated_view } = observation;
    const common = {
      observation,
      subjects: diagnostic.subjects.map(target),
      focus: diagnostic.focus === null ? null : target(diagnostic.focus),
    };
    if (evaluated_view.kind === "initialization") {
      attachment.initialization.push({
        ...common,
        range: null,
        boundary: report.analysis.view.end_exclusive,
        notAtHead: false,
      });
      continue;
    }
    const first = BigInt(evaluated_view.first.end_exclusive);
    const last = BigInt(evaluated_view.last.end_exclusive);
    const attached: Attached = {
      ...common,
      range: {
        first: evaluated_view.first.end_exclusive,
        last: evaluated_view.last.end_exclusive,
      },
      boundary: report.analysis.view.end_exclusive,
      notAtHead: last < head,
    };
    const newest = first - 1n;
    if (newest < base) {
      if (last >= base) {
        attachment.history.push(attached);
      } else if (diagnostic.kind === "inconsistency") {
        attachment.ended.push(attached);
      } else {
        attachment.endedAdvisories += 1;
      }
      continue;
    }
    const index = indexAt(newest);
    if (index === null) {
      attachment.unplaced.push(attached);
      continue;
    }
    const list = attachment.operations.get(index);
    if (list === undefined) {
      attachment.operations.set(index, [attached]);
    } else {
      list.push(attached);
    }
  }
  attachments.set(report, attachment);
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

// The findings of a whole report, and those the plan's operations introduced.
export function reportCounts(report: ProposalReport): {
  total: Counts;
  introduced: Counts;
} {
  const attachment = attachFindings(report);
  const total: Counts = { inconsistency: 0, warning: 0, suggestion: 0 };
  for (const observation of report.analysis.observations) {
    total[observation.diagnostic.kind] += 1;
  }
  return {
    total,
    introduced: countFindings([...attachment.operations.values()].flat()),
  };
}

// The per-operation status after a check: inconsistencies block, warnings
// are advisory, suggestions are not counted.
export function statusWords(counts: Counts): string {
  const words: string[] = [];
  if (counts.inconsistency > 0) {
    words.push(counted(counts.inconsistency, "inconsistency"));
  }
  if (counts.warning > 0) {
    words.push(counted(counts.warning, "warning"));
  }
  return words.length === 0 ? "ok" : words.join(", ");
}

export function countWords(counts: Counts): string {
  return [
    counted(counts.inconsistency, "inconsistency"),
    counted(counts.warning, "warning"),
    counted(counts.suggestion, "suggestion"),
  ].join(", ");
}
