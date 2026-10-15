import type { EntryId, EntryKind } from "../contract.ts";
import { sameId } from "./labels.ts";
import { type ExploreForm, readEntryId, readForm } from "./query.ts";

// A place to return to: an entry with the label and type it had when it was
// last inspected, so the sidebar can name it without a request.
export interface Place {
  id: EntryId;
  kind: EntryKind;
  label: string;
  ty_name: string;
}

export interface SavedView {
  name: string;
  form: ExploreForm;
}

// Per database: recently inspected entries, most recent first; pinned
// entries and saved Explore forms in the order they were added.
export interface Places {
  recent: Place[];
  pinned: Place[];
  views: SavedView[];
}

export const RECENT_LIMIT = 15;

export const emptyPlaces: Places = { recent: [], pinned: [], views: [] };

export function placesKey(database: string): string {
  return `lifecore.places.${database}`;
}

function samePlace(a: Place, b: Place): boolean {
  return (
    sameId(a.id, b.id) &&
    a.kind === b.kind &&
    a.label === b.label &&
    a.ty_name === b.ty_name
  );
}

// An inspection moves the entry to the front of Recent and refreshes its
// label wherever it is kept. An unchanged list is returned as is, so a
// repeated visit stores and renders nothing.
export function visit(places: Places, place: Place): Places {
  const first = places.recent[0];
  const pinnedFresh = places.pinned.every(
    (pinned) => !sameId(pinned.id, place.id) || samePlace(pinned, place),
  );
  if (first !== undefined && samePlace(first, place) && pinnedFresh) {
    return places;
  }
  return {
    ...places,
    recent: [
      place,
      ...places.recent.filter((recent) => !sameId(recent.id, place.id)),
    ].slice(0, RECENT_LIMIT),
    pinned: pinnedFresh
      ? places.pinned
      : places.pinned.map((pinned) =>
          sameId(pinned.id, place.id) ? place : pinned,
        ),
  };
}

export function isPinned(places: Places, id: EntryId): boolean {
  return places.pinned.some((pinned) => sameId(pinned.id, id));
}

export function togglePin(places: Places, place: Place): Places {
  return isPinned(places, place.id)
    ? unpin(places, place.id)
    : { ...places, pinned: [...places.pinned, place] };
}

export function unpin(places: Places, id: EntryId): Places {
  return {
    ...places,
    pinned: places.pinned.filter((pinned) => !sameId(pinned.id, id)),
  };
}

// A name in use is replaced where it stands.
export function saveView(
  places: Places,
  name: string,
  form: ExploreForm,
): Places {
  const view = { name, form };
  const index = places.views.findIndex((saved) => saved.name === name);
  return {
    ...places,
    views:
      index < 0
        ? [...places.views, view]
        : places.views.map((saved, i) => (i === index ? view : saved)),
  };
}

export function removeView(places: Places, name: string): Places {
  return {
    ...places,
    views: places.views.filter((saved) => saved.name !== name),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readPlace(value: unknown): Place | null {
  if (!isRecord(value)) {
    return null;
  }
  const id = readEntryId(value.id);
  if (
    id === null ||
    (value.kind !== "node" && value.kind !== "edge") ||
    typeof value.label !== "string" ||
    typeof value.ty_name !== "string"
  ) {
    return null;
  }
  return { id, kind: value.kind, label: value.label, ty_name: value.ty_name };
}

function readView(value: unknown): SavedView | null {
  if (!isRecord(value) || typeof value.name !== "string") {
    return null;
  }
  const form = readForm(value.form);
  return form === null ? null : { name: value.name, form };
}

function readList<T>(value: unknown, read: (item: unknown) => T | null): T[] {
  return Array.isArray(value)
    ? value.map(read).filter((item): item is T => item !== null)
    : [];
}

// Whatever of a stored value still reads; a malformed item is dropped, not
// the whole list.
export function readPlaces(value: unknown): Places {
  if (!isRecord(value)) {
    return emptyPlaces;
  }
  return {
    recent: readList(value.recent, readPlace).slice(0, RECENT_LIMIT),
    pinned: readList(value.pinned, readPlace),
    views: readList(value.views, readView),
  };
}

type Store = Pick<Storage, "getItem" | "setItem">;

export function loadPlaces(
  store: Pick<Store, "getItem">,
  database: string,
): Places {
  try {
    const text = store.getItem(placesKey(database));
    return text === null ? emptyPlaces : readPlaces(JSON.parse(text));
  } catch {
    return emptyPlaces;
  }
}

// Storage failures (quota, private mode) leave the places in memory only.
export function storePlaces(store: Store, database: string, places: Places) {
  try {
    store.setItem(placesKey(database), JSON.stringify(places));
  } catch {
    // Nothing to do: the next change will try again.
  }
}
