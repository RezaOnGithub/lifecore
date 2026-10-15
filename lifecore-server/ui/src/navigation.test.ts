import assert from "node:assert/strict";
import { test } from "node:test";
import { addSeed, emptyForm } from "./model/query.ts";
import {
  defaultTableScreenView,
  exploreScreen,
  initialExplore,
  initialNavigation,
  inspectHref,
  type Navigation,
  navigate,
  type Screen,
} from "./navigation.ts";

const table: Screen = { kind: "table", ty_name: "Task" };
const inspector: Screen = {
  kind: "inspector",
  id: { origin: "o", position: "1" },
};
const open = (state: Navigation, screen: Screen, scroll = 0) =>
  navigate(state, { kind: "open", screen, scroll });
const screens = (state: Navigation) => ({
  current: state.current.screen,
  history: state.history.map((step) => step.screen),
});

test("opening pushes the previous screen and back pops it", () => {
  const opened = open(initialNavigation, table);
  assert.deepEqual(screens(opened), {
    current: table,
    history: [{ kind: "database" }],
  });
  const deeper = open(opened, inspector);
  assert.deepEqual(screens(deeper).history, [{ kind: "database" }, table]);
  const back = navigate(deeper, { kind: "back" });
  assert.deepEqual(screens(back), screens(opened));
  assert.equal(back.current.id, opened.current.id);
});

test("reopening the current screen and backing out of the first are no-ops", () => {
  const opened = open(initialNavigation, table);
  assert.equal(open(opened, { kind: "table", ty_name: "Task" }), opened);
  assert.equal(
    navigate(initialNavigation, { kind: "back" }),
    initialNavigation,
  );
  assert.notEqual(open(opened, { kind: "table", ty_name: "Type" }), opened);
});

test("operations are screens by name and replace swaps without a step", () => {
  const operation: Screen = { kind: "operation", name: "task_1" };
  const opened = open(initialNavigation, operation);
  assert.equal(open(opened, { kind: "operation", name: "task_1" }), opened);
  const renamed = navigate(opened, {
    kind: "replace",
    screen: { kind: "operation", name: "task_2" },
  });
  assert.deepEqual(screens(renamed), {
    current: { kind: "operation", name: "task_2" },
    history: [{ kind: "database" }],
  });
  assert.notEqual(open(renamed, { kind: "import" }), renamed);
});

test("a step keeps its view and scroll for Back; edits reach only the current step", () => {
  const onTable = open(initialNavigation, table);
  const step = onTable.current.id;
  const sorted = navigate(onTable, {
    kind: "edit",
    step,
    edit: (screen) =>
      screen.kind === "table"
        ? {
            ...screen,
            view: {
              ...defaultTableScreenView,
              text: "hinge",
              table: {
                ...defaultTableScreenView.table,
                sort: { column: "label", descending: true },
              },
            },
          }
        : screen,
  });
  // Leaving records the scroll offset of the step left.
  const away = open(sorted, inspector, 840);
  // An edit for a step no longer current changes nothing.
  assert.equal(navigate(away, { kind: "edit", step, edit: () => table }), away);
  const back = navigate(away, { kind: "back" });
  assert.equal(back.current.id, step);
  assert.equal(back.current.scroll, 840);
  const shown = back.current.screen;
  assert.equal(shown.kind === "table" && shown.view?.text, "hinge");
  assert.deepEqual(shown.kind === "table" && shown.view?.table.sort, {
    column: "label",
    descending: true,
  });
  // A new step starts at the top.
  assert.equal(away.current.scroll, 0);
});

test("the trail goes back to any step, dropping the ones after it", () => {
  const a = open(initialNavigation, table, 10);
  const b = open(a, inspector, 20);
  const c = open(b, { kind: "findings" }, 30);
  const jumped = navigate(c, { kind: "back", to: 1 });
  assert.deepEqual(screens(jumped), {
    current: table,
    history: [{ kind: "database" }],
  });
  assert.equal(jumped.current.scroll, 20);
  assert.equal(navigate(c, { kind: "back", to: 7 }), c);
});

test("Explore steps differ by the query they ran, and the last one is kept", () => {
  const first = exploreScreen(
    addSeed(emptyForm, { origin: "o", position: "1" }),
  );
  const second = exploreScreen(
    addSeed(emptyForm, { origin: "o", position: "2" }),
  );
  assert.equal(initialNavigation.explore, initialExplore);
  const one = open(initialNavigation, first);
  assert.equal(one.explore, first);
  // The same query on show is the same place.
  assert.equal(open(one, { ...first, view: { ...first.view } }), one);
  // A new query leaves the previous one as given and becomes a step.
  const edited = { ...first, form: second.form };
  const two = navigate(one, {
    kind: "open",
    screen: second,
    scroll: 0,
    leaving: first,
  });
  assert.equal(two.explore, second);
  assert.equal(two.history.at(-1)?.screen, first);
  assert.notEqual(two.history.at(-1)?.screen, edited);
  // A detour keeps the last Explore step for the sidebar and the hidden
  // screen; Back to the first query makes it the last again.
  const detour = open(two, inspector);
  assert.equal(detour.explore, second);
  const back = navigate(navigate(detour, { kind: "back" }), { kind: "back" });
  assert.equal(back.explore, first);
});

test("a rendered graph's links name entries by origin and position", () => {
  assert.deepEqual(
    inspectHref("#inspect/00000000-0000-4000-8000-000000000005/12"),
    { origin: "00000000-0000-4000-8000-000000000005", position: "12" },
  );
  assert.equal(inspectHref("#inspect/o/012"), null);
  assert.equal(inspectHref("#inspect/o"), null);
  assert.equal(inspectHref("https://example.org/#inspect/o/1"), null);
  assert.equal(inspectHref(null), null);
});
