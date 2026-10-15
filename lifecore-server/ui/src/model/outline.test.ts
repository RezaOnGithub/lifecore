import assert from "node:assert/strict";
import { test } from "node:test";
import {
  COLLAPSE_ABOVE,
  emptyOutline,
  groupId,
  groupState,
  historicalByDefault,
  onPath,
  rowPath,
  setExpanded,
  setFilter,
  setGroupOpen,
  setHistorical,
} from "./outline.ts";

const id = (position: string) => ({ origin: "o", position });
const item = (label: string, current = true) => ({
  label,
  edge: { current },
});
const label = ({ label }: { label: string }) => label;

test("rows are named by the edges leading to them, groups by row and relation", () => {
  const first = rowPath("", id("12"));
  assert.equal(first, "o:12");
  assert.equal(rowPath(first, id("30")), "o:12/o:30");
  assert.equal(
    groupId(first, { ty_name: "PartOf", direction: "incoming" }),
    "o:12|incoming:PartOf",
  );
});

test("a node on the path from the inspected one is marked, not expanded", () => {
  const nodes = ["o:1", "o:5"];
  assert.equal(onPath(nodes, id("1")), true);
  assert.equal(onPath(nodes, id("5")), true);
  assert.equal(onPath(nodes, id("7")), false);
});

test("historical edges wait behind a toggle, except under a superseded node", () => {
  assert.equal(historicalByDefault({ current: true }), false);
  assert.equal(historicalByDefault({ current: false }), true);
  const items = [item("a"), item("b", false), item("c")];
  const current = groupState(
    emptyOutline,
    "g",
    { current: true },
    items,
    label,
  );
  assert.deepEqual(current.shown.map(label), ["a", "c"]);
  assert.equal(current.historical, 1);
  assert.equal(current.withHistorical, false);
  const superseded = groupState(
    emptyOutline,
    "g",
    { current: false },
    items,
    label,
  );
  assert.deepEqual(superseded.shown.map(label), ["a", "b", "c"]);
  // The toggle is kept only while it differs from the default.
  const shown = setHistorical(emptyOutline, "g", true, { current: true });
  assert.deepEqual(shown.historical, { g: true });
  assert.deepEqual(
    groupState(shown, "g", { current: true }, items, label).shown.map(label),
    ["a", "b", "c"],
  );
  assert.deepEqual(
    setHistorical(shown, "g", false, { current: true }).historical,
    {},
  );
});

test("large groups start collapsed and filter by label", () => {
  const many = Array.from({ length: COLLAPSE_ABOVE + 5 }, (_, index) =>
    item(`Stage ${index}`),
  );
  const state = groupState(emptyOutline, "g", { current: true }, many, label);
  assert.equal(state.open, false);
  assert.equal(state.filterable, true);
  assert.equal(state.total, many.length);
  const opened = setGroupOpen(emptyOutline, "g", true, false);
  const filtered = setFilter(opened, "g", "stage 1");
  const narrowed = groupState(filtered, "g", { current: true }, many, label);
  assert.equal(narrowed.open, true);
  assert.deepEqual(narrowed.shown.map(label), [
    "Stage 1",
    ...Array.from({ length: 10 }, (_, i) => `Stage ${10 + i}`),
  ]);
  assert.equal(narrowed.total, many.length);
  // A small group neither collapses nor filters.
  const few = groupState(
    filtered,
    "g",
    { current: true },
    many.slice(0, 3),
    label,
  );
  assert.equal(few.filterable, false);
  assert.equal(few.shown.length, 3);
  assert.deepEqual(setFilter(filtered, "g", "").filters, {});
  // Closing a group open by default is remembered; reopening forgets it.
  const closed = setGroupOpen(emptyOutline, "h", false, true);
  assert.deepEqual(closed.groups, { h: false });
  assert.deepEqual(setGroupOpen(closed, "h", true, true).groups, {});
});

test("expanded rows are kept by path, once each", () => {
  const one = setExpanded(emptyOutline, "o:1", true);
  const two = setExpanded(setExpanded(one, "o:1/o:2", true), "o:1", true);
  assert.deepEqual(two.expanded, ["o:1/o:2", "o:1"]);
  assert.deepEqual(setExpanded(two, "o:1", false).expanded, ["o:1/o:2"]);
});
