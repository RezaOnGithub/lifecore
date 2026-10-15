import assert from "node:assert/strict";
import { test } from "node:test";
import { initialNavigation, navigate, type Screen } from "./navigation.ts";

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
