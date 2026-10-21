import assert from "node:assert/strict";
import { test } from "node:test";
import {
  beyondBoundary,
  emptyPlaces,
  isPinned,
  loadPlaces,
  type Place,
  placesKey,
  RECENT_LIMIT,
  readPlaces,
  removeView,
  saveView,
  storePlaces,
  togglePin,
  unpin,
  visit,
} from "./places.ts";
import { addSeed, emptyForm } from "./query.ts";

const place = (position: string, label = `Task ${position}`): Place => ({
  id: { origin: "o", position },
  kind: "node",
  label,
  ty_name: "Task",
});

test("recent keeps the last visits, most recent first, without duplicates", () => {
  let places = emptyPlaces;
  for (let i = 0; i < RECENT_LIMIT + 3; i += 1) {
    places = visit(places, place(String(i)));
  }
  assert.equal(places.recent.length, RECENT_LIMIT);
  assert.equal(places.recent[0]?.id.position, String(RECENT_LIMIT + 2));
  places = visit(places, place("10"));
  assert.deepEqual(
    places.recent.slice(0, 3).map((recent) => recent.id.position),
    ["10", String(RECENT_LIMIT + 2), String(RECENT_LIMIT + 1)],
  );
  assert.equal(
    places.recent.filter((recent) => recent.id.position === "10").length,
    1,
  );
  // The same visit again changes nothing, not even the object.
  assert.equal(visit(places, place("10")), places);
});

test("pins toggle, keep their order, and take the label of a new visit", () => {
  let places = togglePin(togglePin(emptyPlaces, place("1")), place("2"));
  assert.deepEqual(
    places.pinned.map((pinned) => pinned.id.position),
    ["1", "2"],
  );
  assert.equal(isPinned(places, place("2").id), true);
  places = visit(places, place("1", "Renamed"));
  assert.equal(places.pinned[0]?.label, "Renamed");
  assert.equal(places.recent[0]?.label, "Renamed");
  places = togglePin(places, place("1"));
  assert.deepEqual(
    places.pinned.map((pinned) => pinned.id.position),
    ["2"],
  );
  assert.deepEqual(unpin(places, place("2").id).pinned, []);
});

test("saving a view under a name in use replaces it in place", () => {
  const one = addSeed(emptyForm, { origin: "o", position: "1" });
  const two = addSeed(emptyForm, { origin: "o", position: "2" });
  let places = saveView(saveView(emptyPlaces, "a", one), "b", one);
  places = saveView(places, "a", two);
  assert.deepEqual(
    places.views.map((view) => [view.name, view.form.ids[0]?.position]),
    [
      ["a", "2"],
      ["b", "1"],
    ],
  );
  assert.deepEqual(
    removeView(places, "a").views.map((view) => view.name),
    ["b"],
  );
});

test("places are stored per database and malformed items are dropped", () => {
  const items = new Map<string, string>();
  const store = {
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => {
      items.set(key, value);
    },
  };
  const places = saveView(
    togglePin(visit(emptyPlaces, place("1")), place("1")),
    "seeded",
    addSeed(emptyForm, { origin: "o", position: "1" }),
  );
  storePlaces(store, "db-a", places);
  assert.deepEqual(loadPlaces(store, "db-a"), places);
  assert.deepEqual(loadPlaces(store, "db-b"), emptyPlaces);
  items.set(placesKey("db-c"), "{not json");
  assert.deepEqual(loadPlaces(store, "db-c"), emptyPlaces);
  assert.deepEqual(
    readPlaces({
      recent: [place("1"), { id: { origin: "o", position: "x" } }, 7],
      pinned: "nope",
      views: [{ name: "broken", form: {} }],
    }),
    { recent: [place("1")], pinned: [], views: [], aliases: [] },
  );
});

test("names are stored with the places, in name order", () => {
  const items = new Map<string, string>();
  const store = {
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => {
      items.set(key, value);
    },
  };
  const kitchen = {
    name: "kitchen",
    id: { origin: "o", position: "2" },
    kind: "node" as const,
    ty_name: "Room",
  };
  const galley = { ...kitchen, name: "galley" };
  storePlaces(store, "db-a", { ...emptyPlaces, aliases: [galley, kitchen] });
  assert.deepEqual(loadPlaces(store, "db-a").aliases, [galley, kitchen]);
  assert.deepEqual(loadPlaces(store, "db-b").aliases, []);
  assert.deepEqual(readPlaces({ aliases: [kitchen, galley, 3] }).aliases, [
    galley,
    kitchen,
  ]);
});

test("under an as-of boundary, places at or past it are dimmed", () => {
  assert.equal(beyondBoundary(place("9"), null), false);
  assert.equal(beyondBoundary(place("9"), "10"), false);
  assert.equal(beyondBoundary(place("10"), "10"), true);
  // Positions compare as integers, not as text, beyond 2^53 too.
  assert.equal(beyondBoundary(place("100"), "99"), true);
  assert.equal(
    beyondBoundary(place("9007199254740993"), "9007199254740993"),
    true,
  );
  assert.equal(
    beyondBoundary(place("9007199254740992"), "9007199254740993"),
    false,
  );
});
