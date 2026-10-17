import { type FormEvent, useState } from "react";
import type { DecimalString, Description } from "../contract.ts";
import type { Load } from "../data.tsx";
import { useOpen } from "../links.tsx";
import { viewWords } from "../model/labels.ts";
import type { Navigation, NavigationAction } from "../navigation.ts";

const BOUNDARY = /^(0|[1-9][0-9]*)$/;

export function TopBar({
  description,
  nav,
  dispatch,
  asOf,
  onAsOf,
  onRefresh,
}: {
  description: Load<Description>;
  nav: Navigation;
  dispatch: (action: NavigationAction) => void;
  asOf: DecimalString | null;
  onAsOf: (asOf: DecimalString | null) => void;
  onRefresh: () => void;
}) {
  const open = useOpen();
  const [draft, setDraft] = useState(asOf ?? "");
  // A boundary set elsewhere, such as the inspector's "As of here", or
  // cleared for another database, shows in the field.
  const [shown, setShown] = useState(asOf);
  if (shown !== asOf) {
    setShown(asOf);
    setDraft(asOf ?? "");
  }
  const valid = BOUNDARY.test(draft);
  const visible =
    description.status === "ready"
      ? description.result.view.end_exclusive
      : null;
  // The server refuses a boundary past the end as well; checking here only
  // spares a request that is bound to fail.
  const beyond = valid && visible !== null && BigInt(draft) > BigInt(visible);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (valid && !beyond) {
      onAsOf(draft);
    }
  };
  return (
    <header className="top">
      <h1>LifeCore</h1>
      {description.status === "ready" && (
        <>
          <button
            type="button"
            className="link"
            title={`database ${description.result.database}`}
            onClick={() => open({ kind: "database" })}
          >
            <code className="db-id" data-testid="database">
              {description.result.database}
            </code>
          </button>
          {asOf === null ? (
            <span title="the whole history">
              entries{" "}
              <strong data-testid="entries">
                {description.result.view.end_exclusive}
              </strong>
            </span>
          ) : (
            <span
              className="badge asof"
              data-testid="as-of"
              title={`Every screen shows ${viewWords(asOf)}; "current" shows the whole history again.`}
            >
              showing the first {asOf} of{" "}
              <strong data-testid="entries">
                {description.result.view.end_exclusive}
              </strong>{" "}
              entries, view [0, {asOf})
            </span>
          )}
        </>
      )}
      <form className="row" onSubmit={submit}>
        <label title="Show the database as it was after this many entries were appended. The inspector's “As of here” sets it at an entry.">
          as of{" "}
          <input
            name="boundary"
            inputMode="numeric"
            size={8}
            placeholder="entry count"
            value={draft}
            onChange={(event) => setDraft(event.target.value.trim())}
          />
        </label>
        <button type="submit" disabled={!valid || beyond}>
          Set
        </button>
        {beyond && (
          <span className="muted">at most {visible}, the whole history</span>
        )}
        {asOf !== null && (
          <button
            type="button"
            title="show the whole history again"
            onClick={() => onAsOf(null)}
          >
            current
          </button>
        )}
      </form>
      <span className="spacer" />
      <button
        type="button"
        disabled={nav.history.length === 0}
        onClick={() => dispatch({ kind: "back" })}
      >
        ← Back
      </button>
      <button type="button" onClick={onRefresh}>
        Refresh
      </button>
    </header>
  );
}
