import { useDraft } from "../draft/context.tsx";
import type { Attached, Counts, Target } from "../draft/findings.ts";
import {
  attachFindings,
  countFindings,
  statusWords,
} from "../draft/findings.ts";
import type { DraftState } from "../draft/model.ts";
import { requestOf } from "../draft/model.ts";
import { IdLink, OperationLink } from "../links.tsx";
import { sameId } from "../model/labels.ts";
import { FindingRow } from "./FindingRow.tsx";

// The findings of the last check, as the views need them: per operation by
// its current name, whether they judged the draft as it is now, and the
// names of the checked operations by index for provisional targets.
export interface Judged {
  findings: (name: string) => Attached[] | undefined;
  // Findings of checked operations that are no longer in the draft, and
  // those no operation of the report introduced.
  unplaced: Attached[];
  names: readonly string[];
  current: boolean;
}

export function judged(state: DraftState): Judged | null {
  const { outcome } = state;
  if (outcome === null) {
    return null;
  }
  const attachment = attachFindings(outcome.report);
  const present = new Set(state.proposal.entries.map((entry) => entry.name));
  const unplaced = [...attachment.unplaced];
  for (const [index, list] of attachment.operations) {
    const name = outcome.names[index];
    if (name === undefined || !present.has(name)) {
      unplaced.push(...list);
    }
  }
  return {
    findings: (name) => {
      const index = outcome.names.indexOf(name);
      return index < 0 ? undefined : (attachment.operations.get(index) ?? []);
    },
    unplaced,
    names: outcome.names,
    current: outcome.request === requestOf(state),
  };
}

export const EARLIER = "from a check of an earlier version of the draft";

export function TargetChip({
  target,
  names,
}: {
  target: Target;
  names: readonly string[];
}) {
  if (target.kind === "existing") {
    return <IdLink id={target.id} />;
  }
  const name = names[target.index];
  return name === undefined ? (
    <span className="muted">operation {target.index + 1}</span>
  ) : (
    <OperationLink name={name} />
  );
}

// One check status badge: red with inconsistencies, amber with warnings only,
// green when clean; dimmed when it judged other content.
export function CheckStatus({
  counts,
  current = true,
}: {
  counts: Counts;
  current?: boolean;
}) {
  const tone =
    counts.inconsistency > 0 ? "bad" : counts.warning > 0 ? "warn" : "ok";
  return (
    <span
      className={`badge check-${tone}${current ? "" : " dim"}`}
      data-testid="check-status"
      title={current ? undefined : EARLIER}
    >
      {statusWords(counts)}
    </span>
  );
}

export function OperationStatus({ name }: { name: string }) {
  const { state } = useDraft();
  const judgment = judged(state);
  const findings = judgment?.findings(name);
  if (judgment === null || findings === undefined) {
    return null;
  }
  return (
    <CheckStatus counts={countFindings(findings)} current={judgment.current} />
  );
}

// What a range that ended before the candidate's head says: only that the
// head does not emit it (SPEC §8.3).
const NOT_AT_HEAD = (
  <span className="badge not-emitted" data-testid="not-at-head">
    not emitted at the candidate's head
  </span>
);

function sameTarget(a: Target, b: Target): boolean {
  if (a.kind === "operation") {
    return b.kind === "operation" && a.index === b.index;
  }
  return b.kind === "existing" && sameId(a.id, b.id);
}

// Findings as the backend worded them, in the shared row, with provisional
// targets linking to their operations.
// `note` says why a list is dimmed; the tray says it once above its rows.
export function FindingList({
  findings,
  names,
  current = true,
  note = true,
}: {
  findings: readonly Attached[];
  names: readonly string[];
  current?: boolean;
  note?: boolean;
}) {
  return (
    <>
      {!current && note && <p className="muted dim-note">{EARLIER}</p>}
      <ul className={`findings${current ? "" : " dim"}`} data-testid="findings">
        {findings.map((attached, index) => (
          <FindingRow
            // Observations are distinct by key, but the key is long; the
            // index is stable for one report.
            // biome-ignore lint/suspicious/noArrayIndexKey: list is rebuilt per report
            key={index}
            observation={attached.observation}
            boundary={attached.boundary}
            ended={NOT_AT_HEAD}
            focus={attached.focus}
            subjects={attached.subjects}
            same={sameTarget}
            chip={(target) => <TargetChip target={target} names={names} />}
          />
        ))}
      </ul>
    </>
  );
}
