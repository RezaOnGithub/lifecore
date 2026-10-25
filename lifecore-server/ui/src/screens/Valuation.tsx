import { useCallback, useEffect, useState } from "react";
import type {
  AnalyzerConfig,
  NodeRecord,
  Ranked,
  Valuation,
} from "../contract.ts";
import { latest, useAsOf, useCall, useData, useLabels } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { IdLink, NodeLink, useEdit } from "../links.tsx";
import { entryKey, labelField, typeName, viewWords } from "../model/labels.ts";
import {
  configuresPlanning,
  emphasis,
  rankedIds,
  sampleChoice,
  type TierGroup,
  tierGroups,
  tierList,
  valuationParams,
} from "../model/valuation.ts";
import { defaultValuationView, type ValuationView } from "../navigation.ts";
import type { Call } from "../rpc.ts";
import { readLabels } from "../views/entryLabels.ts";

interface Labels {
  nodes: Map<string, NodeRecord>;
  failure: unknown;
}

// Labels of a valuation's rows, read at its boundary as the Findings screen
// reads those of a run. A read that succeeded stays with the valuation
// object, which the result cache hands back for an identical answer, so a
// screen returned to names its rows at once; a failed one is read again on
// the next refresh.
const READ = new WeakMap<Valuation, Labels>();

// The labels for `valuation`, or, until they arrive, those of the last
// valuation this screen read. Either serves only the rows of `valuation`,
// all inside its boundary, and a revision's label and type are the same at
// every boundary that holds it.
function useRankedLabels(valuation: Valuation): Labels | null {
  const { client, refresh } = useData();
  const book = useLabels();
  const [last, setLast] = useState<Labels | null>(null);
  // `refresh` retries a failed read; a kept one is not read again.
  // biome-ignore lint/correctness/useExhaustiveDependencies: see above
  useEffect(() => {
    if (READ.has(valuation)) {
      return;
    }
    let active = true;
    const call: Call = (method, params) => client.call(method, params);
    void readLabels(
      call,
      rankedIds(valuation.ranked),
      valuation.boundary.end_exclusive,
    ).then((labels) => {
      if (labels.failure === null) {
        READ.set(valuation, labels);
      }
      book.noteNodes(labels.nodes.values());
      if (active) {
        setLast(labels);
      }
    });
    return () => {
      active = false;
    };
  }, [client, book, valuation, refresh]);
  return READ.get(valuation) ?? last;
}

// One ranked revision: its row label linking to the inspector, `@position`,
// and type, then each reason in the server's words. The structured reason
// only decides emphasis. Without a label read, the revision shows by
// position alone.
function RankedRow({
  row,
  node,
}: {
  row: Ranked;
  node: NodeRecord | undefined;
}) {
  return (
    <li className="ranked" data-testid="ranked">
      <span className="ranked-item">
        {node === undefined ? (
          <IdLink id={row.revision} />
        ) : (
          <>
            <NodeLink node={node} />
            {/* A fallback label already reads `<type> @<position>`. */}
            {labelField(node) !== null && (
              <>
                {" "}
                <span className="muted ranked-type">
                  {typeName(node.ty_name)}
                </span>
              </>
            )}
          </>
        )}
      </span>
      {row.reasons.length > 0 && (
        <ul className="reasons">
          {row.reasons.map(({ reason, text }, index) => (
            <li
              // biome-ignore lint/suspicious/noArrayIndexKey: reasons may repeat
              key={index}
              className={emphasis(reason) ?? undefined}
              data-reason={reason.name}
            >
              {text}
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

function TierSection({
  group,
  nodes,
}: {
  group: TierGroup;
  nodes: Map<string, NodeRecord>;
}) {
  const { tier, title, summary, rows } = group;
  return (
    <section
      className={`tier tier-${tier}`}
      aria-labelledby={`tier-${tier}`}
      data-testid={`tier-${tier}`}
    >
      <h3 id={`tier-${tier}`}>
        Tier {tier}: {title}{" "}
        <span className="badge count" title="rows in this tier">
          {rows.length}
        </span>
      </h3>
      <p className="muted tier-summary">{summary}</p>
      <ol className="ranking">
        {rows.map((row) => (
          <RankedRow
            key={entryKey(row.revision)}
            row={row}
            node={nodes.get(entryKey(row.revision))}
          />
        ))}
      </ol>
    </section>
  );
}

function Ranking({ valuation }: { valuation: Valuation }) {
  const labels = useRankedLabels(valuation);
  const { ranked, boundary } = valuation;
  if (ranked.length === 0) {
    return (
      <p className="muted" data-testid="nothing-ranks">
        Nothing ranks in the view [0, {boundary.end_exclusive}): no item of
        planning's types (Goal, Concern, Project, Task, Habit, External) is in
        play or deferred there.
      </p>
    );
  }
  // Rows wait for their first labels rather than flash by position.
  if (labels === null) {
    return <p className="muted">loading…</p>;
  }
  const { shown, empty } = tierGroups(ranked);
  return (
    <>
      {labels.failure !== null && (
        <p className="muted">
          Some entries are shown by position: their labels could not be read.
        </p>
      )}
      {shown.map((group) => (
        <TierSection key={group.tier} group={group} nodes={labels.nodes} />
      ))}
      {empty.length > 0 && (
        <p className="muted" data-testid="empty-tiers">
          Nothing in {tierList(empty)}.
        </p>
      )}
    </>
  );
}

// The planning valuation (PLANNING §9) at the top bar's boundary: read on
// open and on every refresh or boundary change, like a table, since it
// costs about one snapshot load. It has no identity, so nothing here is
// run, filtered, or dismissed.
export function ValuationScreen({
  view,
  suite,
}: {
  view: ValuationView;
  suite: readonly AnalyzerConfig[] | null;
}) {
  const asOf = useAsOf();
  const edit = useEdit();
  const setWindows = useCallback(
    (windows: string) =>
      edit((screen) =>
        screen.kind === "valuation"
          ? {
              ...screen,
              view: { ...(screen.view ?? defaultValuationView), windows },
            }
          : screen,
      ),
    [edit],
  );
  const choice = sampleChoice(view.windows);
  // The sample applies a moment after the last keystroke, as a table's text
  // filter does; one that is not a number leaves the last one applied. A
  // step returned to applies its own at once.
  const [applied, setApplied] = useState(() =>
    choice.kind === "windows" ? choice.windows : null,
  );
  useEffect(() => {
    const timer = setTimeout(() => {
      const chosen = sampleChoice(view.windows);
      if (chosen.kind !== "invalid") {
        setApplied(chosen.kind === "windows" ? chosen.windows : null);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [view.windows]);
  const load = useCall("planning.valuation", valuationParams(asOf, applied));
  const valuation = latest(load);
  return (
    <section aria-label="Plan" data-testid="valuation-screen">
      <h2>Plan</h2>
      <p className="muted intro">
        The planning valuation ranks what presses at this boundary, from what
        the next seal would reject down to the rest in play. A ranking has no
        identity and nothing to dismiss: only an appended entry, such as a
        stance, an event, or a review, moves a row.
      </p>
      {suite !== null && !configuresPlanning(suite) && (
        <p className="muted" data-testid="planning-unconfigured">
          This database's suite does not configure Planning, so no review is
          judged. The ranking reads the graph alone and is the same either way.
        </p>
      )}
      <div className="row">
        <label title="The deadline estimate's sample: the last this many windows between reviews whose times increase. Empty for the default, 6.">
          deadlines estimated from the last{" "}
          <input
            aria-label="deadline sample windows"
            inputMode="numeric"
            size={3}
            placeholder="6"
            value={view.windows}
            aria-invalid={choice.kind === "invalid"}
            onChange={(event) => setWindows(event.target.value)}
          />{" "}
          windows
        </label>
        {choice.kind === "invalid" && (
          <span className="field-error" data-testid="sample-invalid">
            {choice.problem}
          </span>
        )}
        {valuation !== null && (
          <>
            <span className="muted">{valuation.ranked.length} ranked</span>
            <span
              className="muted"
              title={`read as ${viewWords(valuation.boundary.end_exclusive)}`}
              data-testid="valuation-view"
            >
              view [0, {valuation.boundary.end_exclusive})
            </span>
          </>
        )}
        {load.status === "loading" && (
          <span className="muted">
            {valuation === null ? "loading…" : "reloading…"}
          </span>
        )}
      </div>
      {load.status === "failed" && <FailureView failure={load.failure} />}
      {valuation !== null && <Ranking valuation={valuation} />}
    </section>
  );
}
