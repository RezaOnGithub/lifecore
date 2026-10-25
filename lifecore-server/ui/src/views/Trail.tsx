import { useSyncExternalStore } from "react";
import type { EntryId } from "../contract.ts";
import { useData, useLabels, usePlaces } from "../data.tsx";
import { pastBoundary, sameId, typeName } from "../model/labels.ts";
import { truncate } from "../model/payload.ts";
import { seedName } from "../model/query.ts";
import type { Navigation, NavigationAction, Screen } from "../navigation.ts";

// Steps the trail shows before folding older ones into "…".
const TRAIL = 6;
const TRAIL_LABEL = 28;

// A step as the trail names it: the type of a table, the label of an entry
// as last seen, the seeds of a query.
function stepName(
  screen: Screen,
  label: (id: EntryId) => string | undefined,
): string {
  switch (screen.kind) {
    case "database":
      return "Database";
    case "table":
      return `${typeName(screen.ty_name)} table`;
    case "inspector":
      return label(screen.id) ?? `@${screen.id.position}`;
    case "explore": {
      const ids = screen.ran?.form.ids ?? [];
      const [first] = ids;
      if (first === undefined) {
        return screen.ran === null ? "Explore" : "Explore: node selection";
      }
      return `Explore ${seedName(first, label)}${
        ids.length > 1 ? ` +${ids.length - 1}` : ""
      }`;
    }
    case "findings":
      return "Findings";
    case "valuation":
      return "Plan";
    case "operation":
      return `+${screen.name}`;
    case "import":
      return "Import";
  }
}

// The Back stack as a trail under the top bar: the last steps by name, the
// current one last; a click goes back to that step, dropping the ones after
// it.
export function Trail({
  nav,
  dispatch,
}: {
  nav: Navigation;
  dispatch: (action: NavigationAction) => void;
}) {
  const book = useLabels();
  const { asOf } = useData();
  // Labels are noted as screens read entries; the trail follows the notes.
  useSyncExternalStore(book.subscribe, book.version);
  const { places } = usePlaces();
  // Past the boundary, a step names its entry by position alone.
  const label = (id: EntryId) =>
    pastBoundary(id, asOf)
      ? undefined
      : (book.get(id, asOf) ??
        [...places.recent, ...places.pinned].find((place) =>
          sameId(place.id, id),
        )?.label);
  const { history } = nav;
  const first = Math.max(0, history.length - TRAIL);
  return (
    <ol className="trail" aria-label="Back trail" data-testid="trail">
      {first > 0 && (
        <li className="muted" title={`${first} earlier steps`}>
          …
        </li>
      )}
      {history.slice(first).map((step, offset) => {
        const name = stepName(step.screen, label);
        return (
          <li key={step.id}>
            <button
              type="button"
              className="link"
              title={`back to ${name}`}
              onClick={() => dispatch({ kind: "back", to: first + offset })}
            >
              {truncate(name, TRAIL_LABEL)}
            </button>
          </li>
        );
      })}
      <li aria-current="page">
        <strong>
          {truncate(stepName(nav.current.screen, label), TRAIL_LABEL)}
        </strong>
      </li>
    </ol>
  );
}
