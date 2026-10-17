import { useState } from "react";
import type { Dismissal, DismissalReason } from "../contract.ts";
import { FailureView } from "../failure.tsx";
import type { FindingGroup } from "../model/findings.ts";
import type { AnalysisRuns, Ran } from "./analysisRuns.ts";
import { useAnalysisRuns } from "./useAnalysisRuns.ts";

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

// A reason and an optional note, kept as typed. It reads its seed once, so
// a form opened on a record closes when that record changes.
export function ReasonForm({
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
export function DismissalBar({
  group,
  ran,
}: {
  group: FindingGroup;
  ran: Ran;
}) {
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
