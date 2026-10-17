import { type ReactNode, useEffect, useState } from "react";
import type {
  AnalyzerIdentity,
  DecimalString,
  Dismissal,
  DismissalReason,
  EntryChange,
  EntryId,
  NodeRecord,
  Observation,
  Reference,
} from "../contract.ts";
import { useData } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { frozen } from "../draft/model.ts";
import { FailureView } from "../failure.tsx";
import { IdLink, NodeLink, OperationLink } from "../links.tsx";
import {
  addBlocker,
  clusterFoci,
  type FindingCluster,
  type FindingGroup,
  keyText,
  rangeWords,
  type Selection,
  shownSubjects,
  unconfirmed,
} from "../model/findings.ts";
import { entryKey, rowLabel, sameId, typeName } from "../model/labels.ts";
import { formatBody } from "../model/payload.ts";
import { counted } from "../model/words.ts";
import type { AnalysisRuns, Bulk, Ran } from "./analysisRuns.ts";
import { ConfirmButton } from "./ConfirmButton.tsx";
import { useAnalysisRuns } from "./useAnalysisRuns.ts";

// What the sticky flag and the historical label do and do not say
// (SPEC §8.3), each in its own tooltip.
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
// there", or, for a sticky advisory, the historical label (SPEC §8.3).
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

const ENDED = (
  <span className="ended" data-testid="ended">
    (ended)
  </span>
);

// What a range that ended before the head of a check or a replay says: only
// that the head does not emit it (SPEC §8.3).
export const NOT_AT_HEAD = (
  <span className="badge not-emitted" data-testid="not-at-head">
    not emitted at the head
  </span>
);

const REASONS: { value: DismissalReason; words: string }[] = [
  { value: "acknowledged", words: "acknowledged" },
  { value: "false_positive", words: "false positive" },
  { value: "no_longer_relevant", words: "no longer relevant" },
];

function reasonWords(reason: DismissalReason): string {
  return REASONS.find((r) => r.value === reason)?.words ?? reason;
}

export const UNDISMISSABLE =
  "Inconsistencies cannot be dismissed: they stay visible in every report and block every change until the history no longer emits them.";

export const MAY_BE_STORED =
  "A dismissal edit may have been stored; Run again to see the stored records.";

// A failed run keeps the last answer of the same parameters on show.
export const PREVIOUS =
  "This run failed: the findings below are from the previous run of the same parameters.";

// A reason and an optional note, kept as typed. It reads its seed once, so
// a form opened on a record closes when that record changes.
function ReasonForm({
  reason: initialReason = "acknowledged",
  note: initialNote = "",
  submit,
  busy,
  onSubmit,
  onCancel,
}: {
  reason?: DismissalReason | undefined;
  note?: string;
  submit: string;
  busy: boolean;
  onSubmit: (reason: DismissalReason, note: string) => void;
  onCancel: () => void;
}) {
  const [reason, setReason] = useState<DismissalReason>(initialReason);
  const [note, setNote] = useState(initialNote);
  return (
    <form
      className="row dismiss-form"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(reason, note);
      }}
    >
      <label>
        reason{" "}
        <select
          value={reason}
          onChange={(event) => setReason(event.target.value as DismissalReason)}
        >
          {REASONS.map((r) => (
            <option key={r.value} value={r.value}>
              {r.words}
            </option>
          ))}
        </select>
      </label>
      <label>
        note{" "}
        <input
          value={note}
          size={30}
          placeholder="optional"
          onChange={(event) => setNote(event.target.value)}
        />
      </label>
      <button type="submit" disabled={busy}>
        {submit}
      </button>
      <button type="button" onClick={onCancel}>
        Cancel
      </button>
    </form>
  );
}

// A failed edit of one key: one known to precede publication changed
// nothing; any other may have stored it, which the banner above says too.
function EditFailureView({
  store,
  group,
}: {
  store: AnalysisRuns;
  group: FindingGroup;
}) {
  const failure = store.editFailure(group.key);
  if (failure === null || failure.kind === "written") {
    return null;
  }
  return (
    <div className="edit-failure" data-testid="edit-failure">
      {failure.kind === "uncertain" && (
        <p className="muted edit-failure-note">
          This edit may have been stored.
        </p>
      )}
      <FailureView failure={failure.failure} compact />
    </div>
  );
}

function sameRecord(a: Dismissal | null, b: Dismissal | null): boolean {
  return a === null || b === null
    ? a === b
    : a.reason === b.reason && a.note === b.note;
}

// Dismissal is display state only (SPEC §8.3): it hides an advisory from
// reports and never changes what blocks a change.
function DismissalBar({ group, ran }: { group: FindingGroup; ran: Ran }) {
  const store = useAnalysisRuns();
  // The record an open form was seeded from. When an edit's answer or a
  // newer run replaces it, the form closes: it would otherwise send values
  // read before, and seeding it again would discard what was typed.
  const [editing, setEditing] = useState<{ seed: Dismissal | null } | null>(
    null,
  );
  if (editing !== null && !sameRecord(editing.seed, group.dismissal)) {
    setEditing(null);
  }
  const busy = store.busy();
  if (group.key.kind === "inconsistency") {
    return (
      // A disabled button shows no tooltip in every browser, so its
      // wrapper carries it too.
      <div className="row dismissal" title={UNDISMISSABLE}>
        <button type="button" disabled title={UNDISMISSABLE}>
          Dismiss
        </button>
        <span className="muted">cannot be dismissed</span>
      </div>
    );
  }
  const { dismissal } = group;
  return (
    <>
      <div className="row dismissal">
        {dismissal !== null && (
          <>
            <span className="badge dismissed" data-testid="dismissed">
              dismissed: {reasonWords(dismissal.reason)}
            </span>
            {dismissal.note !== null && (
              <q className="dismissal-note" data-testid="dismissal-note">
                {dismissal.note}
              </q>
            )}
          </>
        )}
        {store.written(ran, group.key) && (
          <span
            className="badge unconfirmed"
            data-testid="written"
            title="the server published this edit but could not confirm it is durable"
          >
            written; durability unconfirmed
          </span>
        )}
        {editing === null && (
          <button
            type="button"
            disabled={busy}
            onClick={() => setEditing({ seed: dismissal })}
          >
            {dismissal === null ? "Dismiss…" : "Change…"}
          </button>
        )}
        {dismissal !== null && (
          <button
            type="button"
            disabled={busy}
            onClick={() => void store.restore(group.key)}
          >
            Restore
          </button>
        )}
        {store.editing() === group.id && <span className="muted">saving…</span>}
      </div>
      {editing !== null && (
        <ReasonForm
          reason={editing.seed?.reason}
          note={editing.seed?.note ?? ""}
          submit={editing.seed === null ? "Dismiss" : "Save"}
          busy={busy}
          onSubmit={(reason, note) =>
            void store.dismiss(group.key, reason, note).then((result) => {
              if (result === "stored" || result === "written") {
                setEditing(null);
              }
            })
          }
          onCancel={() => setEditing(null)}
        />
      )}
      <EditFailureView store={store} group={group} />
    </>
  );
}

function ActionReference({
  reference,
  nodes,
}: {
  reference: Reference;
  nodes: Map<string, NodeRecord>;
}) {
  return "existing" in reference ? (
    <EntryChip id={reference.existing} nodes={nodes} />
  ) : (
    <span className="ref-chip">action {reference.added + 1}</span>
  );
}

// A suggestion's operations, read-only, and a way to start a draft from
// them. Added operations are ordinary ones: the proposal does not
// acknowledge the finding (PLAN §4.4). The draft applies to the current
// history, so only actions judged at the boundary of a whole-history run of
// this reload can be added; a second addition asks first.
function SuggestedActions({
  observation,
  actions,
  ran,
}: {
  observation: Observation;
  actions: EntryChange[];
  ran: Ran;
}) {
  const { state, actions: draft } = useDraft();
  const { refresh } = useData();
  const store = useAnalysisRuns();
  const { nodes } = ran;
  const id = `${keyText(observation.key)}${JSON.stringify(observation.evaluated_view)}`;
  const recorded = store.added(id);
  const inDraft = new Set(state.proposal.entries.map((entry) => entry.name));
  const added = recorded?.filter((name) => inDraft.has(name)) ?? [];
  const gone = recorded !== null && added.length === 0;
  // Once none of them is in the draft, as after an apply or a clear, the
  // record goes, so a later operation of one of their names is not taken
  // for them.
  useEffect(() => {
    if (gone) {
      store.forgetAdded(id);
    }
  }, [store, id, gone]);
  if (actions.length === 0) {
    return <p className="muted">no suggested operations</p>;
  }
  const blocker = addBlocker(observation, ran.run.analysis, {
    whole: ran.params.as_of === undefined,
    current: ran.refresh === refresh,
  });
  const locked = frozen(state);
  const title =
    blocker ??
    (locked
      ? "an apply is in flight"
      : "appends these operations to the draft, without acknowledging the finding");
  const add = () => {
    const names = draft.addSuggested(actions);
    if (names.length > 0) {
      store.setAdded(id, names);
    }
  };
  return (
    <div className="suggested">
      <ol className="suggested-actions" data-testid="suggested-actions">
        {actions.map((action, index) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: actions are positional
          <li key={index}>
            {action.kind === "node" ? (
              <>
                node{" "}
                <code title={formatBody(action.payload)}>
                  {typeName(action.ty_name)}
                </code>
                {action.predecessor !== undefined && (
                  <>
                    {" "}
                    {action.intent === "branch" ? "branches" : "revises"}{" "}
                    <ActionReference
                      reference={action.predecessor}
                      nodes={nodes}
                    />
                  </>
                )}
              </>
            ) : (
              <>
                edge <code>{typeName(action.ty_name)}</code>{" "}
                <ActionReference reference={action.from} nodes={nodes} /> →{" "}
                <ActionReference reference={action.to} nodes={nodes} />
              </>
            )}
          </li>
        ))}
      </ol>
      <div className="row" title={blocker ?? undefined}>
        {added.length === 0 ? (
          <button
            type="button"
            disabled={locked || blocker !== null}
            title={title}
            onClick={add}
          >
            Add to draft
          </button>
        ) : (
          <ConfirmButton
            confirm="Add them again?"
            disabled={locked || blocker !== null}
            title={title}
            onConfirm={add}
          >
            added ({counted(added.length, "operation")})
          </ConfirmButton>
        )}
        {added.length > 0 && (
          <span className="muted">
            {added.map((name) => (
              <span key={name}>
                <OperationLink name={name} />{" "}
              </span>
            ))}
          </span>
        )}
      </div>
    </div>
  );
}

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
