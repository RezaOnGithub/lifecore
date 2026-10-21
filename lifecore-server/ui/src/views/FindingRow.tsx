import type { ReactNode } from "react";
import type {
  AnalyzerIdentity,
  DecimalString,
  EntryId,
  NodeRecord,
  Observation,
} from "../contract.ts";
import { IdLink, NodeLink } from "../links.tsx";
import { rangeWords, shownSubjects, unconfirmed } from "../model/findings.ts";
import { entryKey, rowLabel } from "../model/labels.ts";

// What the sticky flag and the historical label do and do not say
// (ANALYZERS §3), each in its own tooltip.
const STICKY =
  "sticky: current reports keep this advisory after the analyzer stops emitting it, until it is dismissed; that does not mean it still applies";
const UNCONFIRMED =
  "a sticky advisory last emitted before this boundary: current reports keep it, but nothing says whether it still applies here";

function Sticky() {
  return (
    <span className="muted" title={STICKY} data-testid="sticky">
      sticky
    </span>
  );
}

export function AnalyzerName({
  analyzer,
  sticky = false,
}: {
  analyzer: AnalyzerIdentity;
  sticky?: boolean;
}) {
  return (
    <span className="muted">
      <span title={`configuration ${JSON.stringify(analyzer.configuration)}`}>
        {analyzer.name} {analyzer.version}
      </span>
      {sticky && (
        <>
          , <Sticky />
        </>
      )}
    </span>
  );
}

// The prefix boundaries an observation was emitted at, worded alike
// everywhere against the boundary of its report (`rangeWords`). A range that
// ended before it carries `ended`, the place's own words for "not emitted
// there", or, for a sticky advisory, the historical label (ANALYZERS §3).
function ObservedRange({
  observation,
  boundary,
  ended,
}: {
  observation: Observation;
  boundary: DecimalString;
  ended: ReactNode;
}) {
  const { evaluated_view } = observation;
  const words = rangeWords(evaluated_view, boundary);
  const title =
    evaluated_view.kind === "initialization"
      ? "emitted at the suite's initialization"
      : words.ended
        ? `emitted at the prefix boundaries ${evaluated_view.first.end_exclusive} to ${evaluated_view.last.end_exclusive}, not at ${boundary}; that alone does not mean it was resolved`
        : `emitted at every prefix boundary from ${evaluated_view.first.end_exclusive} to ${boundary}`;
  return (
    <span className="muted range" title={title} data-testid="range">
      {words.text}
      {words.ended && (
        <>
          {" "}
          {unconfirmed(observation, boundary) ? (
            <span
              className="badge unconfirmed"
              title={UNCONFIRMED}
              data-testid="unconfirmed"
            >
              historical; applicability unconfirmed
            </span>
          ) : (
            ended
          )}
        </>
      )}
    </span>
  );
}

// One observation as the backend worded it: kind, message, analyzer, its
// range, then the focus and, when they say more, the subjects as chips.
// Targets are entries on the Findings screen and may be draft operations in
// the tray, so `chip` renders them and `same` compares them. Inside a
// cluster, `brief` is the cluster's message: kind and analyzer are the
// cluster's, and the message shows only where it differs.
export function FindingRow<T>({
  observation,
  boundary,
  ended,
  focus,
  subjects,
  same,
  chip,
  hideFocus = false,
  brief,
  dim = false,
  children,
}: {
  observation: Observation;
  boundary: DecimalString;
  ended: ReactNode;
  focus: T | null;
  subjects: readonly T[];
  same: (a: T, b: T) => boolean;
  chip: (target: T) => ReactNode;
  hideFocus?: boolean;
  brief?: string | undefined;
  dim?: boolean;
  children?: ReactNode;
}) {
  const { diagnostic, key } = observation;
  const listed = shownSubjects(focus, subjects, same);
  const focusChip = focus !== null && !hideFocus;
  return (
    <li className={`finding ${diagnostic.kind}${dim ? " ended" : ""}`}>
      {brief === undefined && (
        <>
          <span className={`badge kind-${diagnostic.kind}`}>
            {diagnostic.kind}
          </span>{" "}
        </>
      )}
      {brief !== diagnostic.message && <>{diagnostic.message} </>}
      {brief === undefined ? (
        <>
          <AnalyzerName
            analyzer={key.analyzer}
            sticky={diagnostic.sticky}
          />{" "}
        </>
      ) : (
        diagnostic.sticky && (
          <>
            <Sticky />{" "}
          </>
        )
      )}
      <ObservedRange
        observation={observation}
        boundary={boundary}
        ended={ended}
      />
      {(focusChip || listed.length > 0) && (
        <ul className="chips">
          {focus !== null && focusChip && <li>focus {chip(focus)}</li>}
          {listed.map((subject, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: subjects may repeat
            <li key={i}>{chip(subject)}</li>
          ))}
        </ul>
      )}
      {children}
    </li>
  );
}

// A node of the analyzed view by its label, anything else by position.
export function EntryChip({
  id,
  nodes,
}: {
  id: EntryId;
  nodes: Map<string, NodeRecord>;
}) {
  const node = nodes.get(entryKey(id));
  return node === undefined ? <IdLink id={id} /> : <NodeLink node={node} />;
}

// What the text filter reads of an entry: its row label, else its position.
export function labelOf(nodes: Map<string, NodeRecord>) {
  return (id: EntryId): string => {
    const node = nodes.get(entryKey(id));
    return node === undefined ? `@${id.position}` : rowLabel(node);
  };
}

export const ENDED = (
  <span className="ended" data-testid="ended">
    (ended)
  </span>
);

// What a range that ended before the head of a check or a replay says: only
// that the head does not emit it (ANALYZERS §3).
export const NOT_AT_HEAD = (
  <span className="badge not-emitted" data-testid="not-at-head">
    not emitted at the head
  </span>
);
