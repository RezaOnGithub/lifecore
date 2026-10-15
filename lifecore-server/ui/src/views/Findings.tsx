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

function Range({ attached }: { attached: Attached }) {
  const { range } = attached;
  if (range === null) {
    return <span className="muted">suite initialization</span>;
  }
  return (
    <span
      className="muted"
      title="boundaries of the first and last prefix it was emitted at"
    >
      {range.first === range.last
        ? `prefix ${range.first}`
        : `prefixes ${range.first}–${range.last}`}
      {attached.resolvedLater && (
        <span className="badge resolved"> resolved later in this draft</span>
      )}
    </span>
  );
}

// Findings as the backend worded them: kind, message, analyzer, prefix
// range, then the focus and subjects as chips linking to the operation or
// the entry.
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
        {findings.map((attached, index) => {
          const { observation, subjects, focus } = attached;
          const { diagnostic, key } = observation;
          return (
            // Observations are distinct by key, but the key is long; the
            // index is stable for one report.
            // biome-ignore lint/suspicious/noArrayIndexKey: list is rebuilt per report
            <li key={index} className={`finding ${diagnostic.kind}`}>
              <span className={`badge kind-${diagnostic.kind}`}>
                {diagnostic.kind}
              </span>{" "}
              {diagnostic.message}{" "}
              <span className="muted">
                {key.analyzer.name} {key.analyzer.version}
                {diagnostic.sticky && ", sticky"}
              </span>{" "}
              <Range attached={attached} />
              {(focus !== null || subjects.length > 0) && (
                <ul className="chips">
                  {focus !== null && (
                    <li>
                      focus <TargetChip target={focus} names={names} />
                    </li>
                  )}
                  {subjects.map((subject, i) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: subjects may repeat
                    <li key={i}>
                      <TargetChip target={subject} names={names} />
                    </li>
                  ))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    </>
  );
}
