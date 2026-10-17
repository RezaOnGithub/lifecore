import { useState } from "react";
import type { EntryId } from "../contract.ts";
import {
  clusterFoci,
  type FindingCluster,
  type FindingGroup,
  rangeWords,
  type Selection,
} from "../model/findings.ts";
import { entryKey, sameId } from "../model/labels.ts";
import { counted } from "../model/words.ts";
import type { AnalysisRuns, Ran } from "./analysisRuns.ts";
import { DismissalBar, ReasonForm } from "./Dismissal.tsx";
import type { Bulk } from "./dismissalEdits.ts";
import { AnalyzerName, ENDED, EntryChip, FindingRow } from "./FindingRow.tsx";
import { SuggestedActions } from "./SuggestedActions.tsx";
import { useAnalysisRuns } from "./useAnalysisRuns.ts";

// A failed run keeps the last answer of the same parameters on show.
export const PREVIOUS =
  "This run failed: the findings below are from the previous run of the same parameters.";

// One finding of a stored-database run: its selected observations, the
// ranges it was also emitted at, a suggestion's operations, and its
// dismissal. History mode dims nonsticky advisories that the boundary no
// longer emits; inconsistencies are never dimmed. `about` is the inspected
// entry, whose own focus chip would only repeat it.
export function GroupRow({
  group,
  ran,
  selection = "current",
  about,
  brief,
}: {
  group: FindingGroup;
  ran: Ran;
  selection?: Selection;
  about?: EntryId;
  brief?: string;
}) {
  const { nodes } = ran;
  const boundary = ran.run.analysis.view.end_exclusive;
  const others = group.observations.filter((o) => !group.selected.includes(o));
  const chip = (id: EntryId) => <EntryChip id={id} nodes={nodes} />;
  return (
    <li
      className={`finding-group ${group.key.kind}${group.dismissal === null ? "" : " dismissed"}`}
      data-testid="finding-group"
    >
      <ul className="findings">
        {group.selected.map((observation, index) => {
          const { diagnostic } = observation;
          const ended = rangeWords(observation.evaluated_view, boundary).ended;
          return (
            <FindingRow
              // biome-ignore lint/suspicious/noArrayIndexKey: one group's observations are fixed per run
              key={index}
              observation={observation}
              boundary={boundary}
              ended={ENDED}
              focus={diagnostic.focus}
              subjects={diagnostic.subjects}
              same={sameId}
              chip={chip}
              hideFocus={
                about !== undefined &&
                diagnostic.focus !== null &&
                sameId(diagnostic.focus, about)
              }
              brief={brief}
              dim={
                selection === "history" &&
                diagnostic.kind !== "inconsistency" &&
                !diagnostic.sticky &&
                ended
              }
            >
              {diagnostic.kind === "suggestion" && (
                <SuggestedActions
                  observation={observation}
                  actions={diagnostic.actions}
                  ran={ran}
                />
              )}
            </FindingRow>
          );
        })}
      </ul>
      {others.length > 0 && (
        <p className="muted also">
          also emitted:{" "}
          {others
            .map((o) => rangeWords(o.evaluated_view, boundary).text)
            .join(", ")}
        </p>
      )}
      <DismissalBar group={group} ran={ran} />
    </li>
  );
}

// The last "Dismiss all", kept above the list since its keys leave their
// cluster as they are dismissed. Failures are told by the rows of their
// keys, so a click on them expands the cluster.
export function BulkProgress({
  bulk,
  store,
  onShow,
}: {
  bulk: Bulk;
  store: AnalysisRuns;
  onShow: () => void;
}) {
  return (
    <div className="row bulk" data-testid="bulk-progress">
      <span>
        Dismiss all “{bulk.label}”: {bulk.stored} of {bulk.total}{" "}
        {bulk.running ? "dismissed so far…" : "dismissed"}
      </span>
      {bulk.failed > 0 && (
        <button type="button" className="link" onClick={onShow}>
          {bulk.failed} failed
        </button>
      )}
      {bulk.stopped !== null && (
        <span className="muted">ended early: {bulk.stopped}</span>
      )}
      {bulk.running ? (
        <button
          type="button"
          disabled={bulk.stopping}
          onClick={() => store.stopBulk()}
        >
          {bulk.stopping ? "stopping…" : "Stop"}
        </button>
      ) : (
        <button
          type="button"
          className="link"
          aria-label="close"
          onClick={() => store.clearBulk()}
        >
          ×
        </button>
      )}
    </div>
  );
}

// Findings that read alike, on one line: the message, the analyzer when the
// suite has several, how many keys, the first foci, and how many findings
// those leave unnamed; expanded, one row per key, each dismissed on its own.
// "Dismiss all" takes one reason and note for every undismissed key and
// sends one edit per key, in turn.
export function ClusterRow({
  cluster,
  ran,
  selection,
  analyzers,
  expanded,
  onExpand,
}: {
  cluster: FindingCluster;
  ran: Ran;
  selection: Selection;
  analyzers: number;
  expanded: boolean;
  onExpand: (expanded: boolean) => void;
}) {
  const store = useAnalysisRuns();
  const [dismissing, setDismissing] = useState(false);
  const { groups, kind } = cluster;
  const { foci, rest } = clusterFoci(cluster);
  const undismissed = groups
    .filter((group) => group.dismissal === null)
    .map((group) => group.key);
  return (
    <li
      className={`finding-cluster ${kind}${undismissed.length === 0 ? " dismissed" : ""}`}
      data-testid="finding-cluster"
    >
      <div className="row cluster-head">
        <button
          type="button"
          className="link disclosure"
          aria-expanded={expanded}
          aria-label={expanded ? "collapse" : "expand"}
          onClick={() => onExpand(!expanded)}
        >
          {expanded ? "▾" : "▸"}
        </button>
        <span className={`badge kind-${kind}`}>{kind}</span>
        <span>{cluster.message}</span>
        {analyzers > 1 && <AnalyzerName analyzer={cluster.analyzer} />}
        <span className="badge count" data-testid="cluster-count">
          ×{groups.length}
        </span>
        <ul className="cluster-chips">
          {foci.map((focus) => (
            <li key={entryKey(focus)}>
              <EntryChip id={focus} nodes={ran.nodes} />
            </li>
          ))}
          {foci.length > 0 && rest > 0 && (
            <li>
              <button
                type="button"
                className="link"
                onClick={() => onExpand(true)}
              >
                +{counted(rest, "more finding")}
              </button>
            </li>
          )}
        </ul>
        {kind !== "inconsistency" && undismissed.length === 0 && (
          <span className="badge dismissed" data-testid="cluster-dismissed">
            all dismissed
          </span>
        )}
        {kind !== "inconsistency" && undismissed.length > 0 && !dismissing && (
          <span className="cluster-actions">
            <button
              type="button"
              disabled={store.busy()}
              onClick={() => setDismissing(true)}
            >
              Dismiss all {undismissed.length}…
            </button>
          </span>
        )}
      </div>
      {dismissing && (
        <ReasonForm
          submit={`Dismiss ${undismissed.length}`}
          busy={store.busy()}
          onSubmit={(reason, note) => {
            setDismissing(false);
            void store.dismissAll(
              { id: cluster.id, label: cluster.message },
              undismissed,
              reason,
              note,
            );
          }}
          onCancel={() => setDismissing(false)}
        />
      )}
      {expanded && (
        <ul className="finding-groups">
          {groups.map((group) => (
            <GroupRow
              key={group.id}
              group={group}
              ran={ran}
              selection={selection}
              brief={cluster.message}
            />
          ))}
        </ul>
      )}
    </li>
  );
}
