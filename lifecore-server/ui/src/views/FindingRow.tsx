import { type ReactNode, useState } from "react";
import type {
  AnalyzerIdentity,
  DismissalReason,
  EntryChange,
  EntryId,
  NodeRecord,
  Observation,
  Reference,
} from "../contract.ts";
import { useDraft } from "../draft/context.tsx";
import { frozen } from "../draft/model.ts";
import { FailureView } from "../failure.tsx";
import { IdLink, NodeLink, OperationLink } from "../links.tsx";
import {
  type FindingGroup,
  rangeText,
  shownSubjects,
  unconfirmed,
} from "../model/findings.ts";
import { entryKey, sameId, typeName } from "../model/labels.ts";
import { formatBody } from "../model/payload.ts";
import {
  type AnalysisRuns,
  type Ran,
  useAnalysisRuns,
} from "./analysisRuns.ts";

export function AnalyzerName({
  analyzer,
  sticky = false,
}: {
  analyzer: AnalyzerIdentity;
  sticky?: boolean;
}) {
  return (
    <span
      className="muted"
      title={`configuration ${JSON.stringify(analyzer.configuration)}`}
    >
      {analyzer.name} {analyzer.version}
      {sticky && ", sticky"}
    </span>
  );
}

// One observation as the backend worded it: kind, message, analyzer, its
// range, then the focus and, when they say more, the subjects as chips.
// Targets are entries on the Findings screen and may be draft operations in
// the tray, so `chip` renders them and `same` compares them.
export function FindingRow<T>({
  observation,
  range,
  focus,
  subjects,
  same,
  chip,
  children,
}: {
  observation: Observation;
  range: ReactNode;
  focus: T | null;
  subjects: readonly T[];
  same: (a: T, b: T) => boolean;
  chip: (target: T) => ReactNode;
  children?: ReactNode;
}) {
  const { diagnostic, key } = observation;
  const listed = shownSubjects(focus, subjects, same);
  return (
    <li className={`finding ${diagnostic.kind}`}>
      <span className={`badge kind-${diagnostic.kind}`}>{diagnostic.kind}</span>{" "}
      {diagnostic.message}{" "}
      <AnalyzerName analyzer={key.analyzer} sticky={diagnostic.sticky} />{" "}
      {range}
      {(focus !== null || listed.length > 0) && (
        <ul className="chips">
          {focus !== null && <li>focus {chip(focus)}</li>}
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

function ObservedRange({
  observation,
  ran,
}: {
  observation: Observation;
  ran: Ran;
}) {
  return (
    <span
      className="muted"
      title="the first and last prefix boundary it was emitted at"
    >
      {rangeText(observation.evaluated_view)}
      {unconfirmed(observation, ran.run.analysis) && (
        <>
          {" "}
          <span className="badge unconfirmed">
            historical; applicability unconfirmed
          </span>
        </>
      )}
    </span>
  );
}

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

function DismissForm({
  group,
  store,
  onDone,
}: {
  group: FindingGroup;
  store: AnalysisRuns;
  onDone: () => void;
}) {
  const [reason, setReason] = useState<DismissalReason>(
    group.dismissal?.reason ?? "acknowledged",
  );
  const [note, setNote] = useState(group.dismissal?.note ?? "");
  const busy = store.editing() !== null;
  return (
    <form
      className="row dismiss-form"
      onSubmit={(event) => {
        event.preventDefault();
        void store.dismiss(group.key, reason, note.trim()).then((stored) => {
          if (stored) {
            onDone();
          }
        });
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
        {group.dismissal === null ? "Dismiss" : "Save"}
      </button>
      <button type="button" onClick={onDone}>
        Cancel
      </button>
    </form>
  );
}

// Dismissal is display state only (SPEC §8.3): it hides an advisory from
// reports and never changes what blocks a change.
function DismissalBar({ group }: { group: FindingGroup }) {
  const store = useAnalysisRuns();
  const [editing, setEditing] = useState(false);
  const busy = store.editing() !== null;
  const failure = store.editFailure(group.key);
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
              <q className="dismissal-note">{dismissal.note}</q>
            )}
          </>
        )}
        {!editing && (
          <button
            type="button"
            disabled={busy}
            onClick={() => setEditing(true)}
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
      {editing && (
        <DismissForm
          group={group}
          store={store}
          onDone={() => setEditing(false)}
        />
      )}
      {failure !== null && <FailureView failure={failure} compact />}
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
// acknowledge the finding (PLAN §4.4).
function SuggestedActions({
  actions,
  nodes,
}: {
  actions: EntryChange[];
  nodes: Map<string, NodeRecord>;
}) {
  const { state, actions: draft } = useDraft();
  const [added, setAdded] = useState<string[] | null>(null);
  if (actions.length === 0) {
    return <p className="muted">no suggested operations</p>;
  }
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
      <div className="row">
        <button
          type="button"
          disabled={frozen(state)}
          title="appends these operations to the draft, without acknowledging the finding"
          onClick={() => {
            const names = draft.addSuggested(actions);
            if (names.length > 0) {
              setAdded(names);
            }
          }}
        >
          Add to draft
        </button>
        {added !== null && (
          <span className="muted">
            added{" "}
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
// ranges it was also observed at, a suggestion's operations, and its
// dismissal.
export function GroupRow({ group, ran }: { group: FindingGroup; ran: Ran }) {
  const { nodes } = ran;
  const others = group.observations.filter((o) => !group.selected.includes(o));
  const chip = (id: EntryId) => <EntryChip id={id} nodes={nodes} />;
  return (
    <li
      className={`finding-group ${group.key.kind}${group.dismissal === null ? "" : " dismissed"}`}
      data-testid="finding-group"
    >
      <ul className="findings">
        {group.selected.map((observation, index) => (
          <FindingRow
            // biome-ignore lint/suspicious/noArrayIndexKey: one group's observations are fixed per run
            key={index}
            observation={observation}
            range={<ObservedRange observation={observation} ran={ran} />}
            focus={observation.diagnostic.focus}
            subjects={observation.diagnostic.subjects}
            same={sameId}
            chip={chip}
          >
            {observation.diagnostic.kind === "suggestion" && (
              <SuggestedActions
                actions={observation.diagnostic.actions}
                nodes={nodes}
              />
            )}
          </FindingRow>
        ))}
      </ul>
      {others.length > 0 && (
        <p className="muted also">
          also observed at{" "}
          {others.map((o) => rangeText(o.evaluated_view)).join(", ")}
        </p>
      )}
      <DismissalBar group={group} />
    </li>
  );
}
