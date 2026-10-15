import assert from "node:assert/strict";
import { test } from "node:test";
import {
  initialNavigation,
  inspectHref,
  navigate,
  type Screen,
} from "./navigation.ts";

const table: Screen = { kind: "table", ty_name: "Task" };
const inspector: Screen = {
  kind: "inspector",
  id: { origin: "o", position: "1" },
};

test("opening pushes the previous screen and back pops it", () => {
  const opened = navigate(initialNavigation, { kind: "open", screen: table });
  assert.deepEqual(opened, { current: table, history: [{ kind: "database" }] });
  const deeper = navigate(opened, { kind: "open", screen: inspector });
  assert.deepEqual(deeper.history, [{ kind: "database" }, table]);
  const back = navigate(deeper, { kind: "back" });
  assert.deepEqual(back, opened);
});

test("reopening the current screen and backing out of the first are no-ops", () => {
  const opened = navigate(initialNavigation, { kind: "open", screen: table });
  assert.equal(
    navigate(opened, {
      kind: "open",
      screen: { kind: "table", ty_name: "Task" },
    }),
    opened,
  );
  assert.equal(
    navigate(initialNavigation, { kind: "back" }),
    initialNavigation,
  );
  assert.notEqual(
    navigate(opened, {
      kind: "open",
      screen: { kind: "table", ty_name: "Type" },
    }),
    opened,
  );
});

test("operations are screens by name and replace swaps without a step", () => {
  const operation: Screen = { kind: "operation", name: "task_1" };
  const opened = navigate(initialNavigation, {
    kind: "open",
    screen: operation,
  });
  assert.equal(
    navigate(opened, {
      kind: "open",
      screen: { kind: "operation", name: "task_1" },
    }),
    opened,
  );
  const renamed = navigate(opened, {
    kind: "replace",
    screen: { kind: "operation", name: "task_2" },
  });
  assert.deepEqual(renamed, {
    current: { kind: "operation", name: "task_2" },
    history: [{ kind: "database" }],
  });
  assert.notEqual(
    navigate(renamed, { kind: "open", screen: { kind: "import" } }),
    renamed,
  );
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
