import type { Attached, Counts, Target } from "../draft/findings.ts";
import { statusWords } from "../draft/findings.ts";
import { IdLink, OperationLink } from "../links.tsx";

export function TargetChip({ target }: { target: Target }) {
  return target.kind === "operation" ? (
    <OperationLink name={target.name} />
  ) : (
    <IdLink id={target.id} />
  );
}

// One check status badge: red with inconsistencies, amber with warnings only,
// green when clean.
export function CheckStatus({ counts }: { counts: Counts }) {
  const tone =
    counts.inconsistency > 0 ? "bad" : counts.warning > 0 ? "warn" : "ok";
  return (
    <span className={`badge check-${tone}`} data-testid="check-status">
      {statusWords(counts)}
    </span>
  );
}

// Findings as the backend worded them: message, kind, analyzer, then the
// focus and subjects as chips linking to the operation or the entry.
export function FindingList({ findings }: { findings: readonly Attached[] }) {
  return (
    <ul className="findings" data-testid="findings">
      {findings.map(({ observation, subjects, focus }, index) => {
        const { diagnostic, key } = observation;
        return (
          // Observations are distinct by key, but the key is long; the index
          // is stable for one report.
          // biome-ignore lint/suspicious/noArrayIndexKey: list is rebuilt per report
          <li key={index} className={`finding ${diagnostic.kind}`}>
            <span className={`badge kind-${diagnostic.kind}`}>
              {diagnostic.kind}
            </span>{" "}
            {diagnostic.message}{" "}
            <span className="muted">
              {key.analyzer.name} {key.analyzer.version}
              {diagnostic.sticky && ", sticky"}
            </span>
            {(focus !== null || subjects.length > 0) && (
              <ul className="chips">
                {focus !== null && (
                  <li>
                    focus <TargetChip target={focus} />
                  </li>
                )}
                {subjects.map((subject, i) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: subjects may repeat
                  <li key={i}>
                    <TargetChip target={subject} />
                  </li>
                ))}
              </ul>
            )}
          </li>
        );
      })}
    </ul>
  );
}
