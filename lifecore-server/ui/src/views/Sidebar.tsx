import { memo, type ReactNode } from "react";
import type { TypeList } from "../contract.ts";
import { type Load, latest, useData, usePlaces } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { useExplore, useOpen } from "../links.tsx";
import { entryKey, sameId, typeName } from "../model/labels.ts";
import { truncate } from "../model/payload.ts";
import { beyondBoundary, type Place } from "../model/places.ts";
import type { ExploreScreen, Screen } from "../navigation.ts";
import { ResolutionBadge } from "./widgets.tsx";

// Sidebar labels are cut shorter than the main pane's.
const PLACE_LIMIT = 40;

function Section({
  id,
  title,
  hint,
  children,
}: {
  id: string;
  title: string;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section aria-labelledby={`side-${id}`}>
      <h2 id={`side-${id}`}>
        {title}
        {hint}
      </h2>
      {children}
    </section>
  );
}

// The sidebar: the types of the view, the Explore and Findings screens,
// and the places kept for this database. The type list on show is the last
// one loaded: a reload only adds a hint, so the entries, and the active
// one, stay where they are. `explore` is the Explore step the link reopens.
export const Sidebar = memo(function Sidebar({
  types,
  screen,
  explore,
}: {
  types: Load<TypeList>;
  screen: Screen;
  explore: ExploreScreen;
}) {
  const open = useOpen();
  const exploreForm = useExplore();
  const list = latest(types);
  const { places, unpin, removeView } = usePlaces();
  return (
    <nav className="side" aria-label="Navigation">
      <Section
        id="types"
        title="Types"
        hint={
          types.status === "loading" &&
          list !== null && <span className="hint"> reloading…</span>
        }
      >
        {types.status === "loading" && list === null && (
          <p className="muted">loading…</p>
        )}
        {types.status === "failed" && (
          <FailureView failure={types.failure} compact />
        )}
        {list !== null && (
          <ul className="types" data-testid="types">
            {list.types.map((type) => (
              <li
                key={type.name}
                className={
                  screen.kind === "table" && screen.ty_name === type.name
                    ? "active"
                    : undefined
                }
              >
                <button
                  type="button"
                  className="link"
                  onClick={() => open({ kind: "table", ty_name: type.name })}
                >
                  {typeName(type.name)}
                </button>{" "}
                <span className="counts" title="current / historical nodes">
                  {type.nodes.current}/{type.nodes.historical}
                </span>{" "}
                <ResolutionBadge resolution={type.resolution} />
              </li>
            ))}
          </ul>
        )}
      </Section>
      <Section id="views" title="Views">
        <ul className="types">
          <li className={screen.kind === "explore" ? "active" : undefined}>
            <button
              type="button"
              className="link"
              title="the last Explore query"
              onClick={() => open(explore)}
            >
              Explore
            </button>
          </li>
          <li className={screen.kind === "findings" ? "active" : undefined}>
            <button
              type="button"
              className="link"
              onClick={() => open({ kind: "findings" })}
            >
              Findings
            </button>
          </li>
        </ul>
      </Section>
      <Section id="pinned" title="Pinned">
        <PlaceList
          places={places.pinned}
          screen={screen}
          testId="pinned"
          empty="☆ in the inspector pins an entry here"
          onRemove={(place) => unpin(place.id)}
        />
      </Section>
      <Section id="saved" title="Saved views">
        {places.views.length === 0 ? (
          <p className="muted side-hint">Save view in Explore keeps a query</p>
        ) : (
          <ul className="types places" data-testid="saved-views">
            {places.views.map((saved) => (
              <li key={saved.name}>
                <button
                  type="button"
                  className="link place-label"
                  title={`open and run "${saved.name}"`}
                  onClick={() => exploreForm(saved.form)}
                >
                  {saved.name}
                </button>
                <button
                  type="button"
                  className="link remove"
                  aria-label={`remove view ${saved.name}`}
                  onClick={() => removeView(saved.name)}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        )}
      </Section>
      <Section id="recent" title="Recent">
        <PlaceList
          places={places.recent}
          screen={screen}
          testId="recent"
          empty="inspected entries appear here"
        />
      </Section>
    </nav>
  );
});

// Entries kept by the sidebar, under the label and type they had when last
// inspected; an edge reads `from —Type→ to`. Under an as-of boundary, those
// past it are dimmed.
function PlaceList({
  places,
  screen,
  testId,
  empty,
  onRemove,
}: {
  places: readonly Place[];
  screen: Screen;
  testId: string;
  empty: string;
  onRemove?: (place: Place) => void;
}) {
  const open = useOpen();
  const { asOf } = useData();
  if (places.length === 0) {
    return <p className="muted side-hint">{empty}</p>;
  }
  return (
    <ul className="types places" data-testid={testId}>
      {places.map((place) => {
        // What was kept of an entry past the boundary shows nothing of it
        // but its position.
        const past = beyondBoundary(place, asOf);
        const position = `@${place.id.position}`;
        const name = past ? position : place.label;
        const active =
          screen.kind === "inspector" && sameId(screen.id, place.id);
        return (
          <li
            key={entryKey(place.id)}
            className={
              [active && "active", past && "beyond"]
                .filter(Boolean)
                .join(" ") || undefined
            }
          >
            <button
              type="button"
              className="link place-label"
              title={
                past
                  ? `${position}\npast the as-of boundary`
                  : `${place.label}\n${position} ${
                      place.kind === "edge" ? "edge " : ""
                    }${typeName(place.ty_name)}`
              }
              onClick={() => open({ kind: "inspector", id: place.id })}
            >
              {truncate(name, PLACE_LIMIT)}
            </button>
            <span className="place-type">
              {past
                ? ""
                : place.kind === "edge"
                  ? position
                  : typeName(place.ty_name)}
            </span>
            {onRemove && (
              <button
                type="button"
                className="link remove"
                aria-label={`unpin ${name}`}
                onClick={() => onRemove(place)}
              >
                ×
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}
