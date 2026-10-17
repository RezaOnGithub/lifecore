import assert from "node:assert/strict";
import { test } from "node:test";
import { withItem, withMember } from "./collections.ts";

test("withMember sets or replaces a key in a copy", () => {
  const map: ReadonlyMap<string, number> = new Map([["a", 1]]);
  assert.deepEqual(
    withMember(map, "b", 2),
    new Map([
      ["a", 1],
      ["b", 2],
    ]),
  );
  assert.deepEqual(withMember(map, "a", 3), new Map([["a", 3]]));
  assert.deepEqual(map, new Map([["a", 1]]));
});

test("withMember removes a key given null", () => {
  const map: ReadonlyMap<string, number> = new Map([
    ["a", 1],
    ["b", 2],
  ]);
  assert.deepEqual(withMember(map, "a", null), new Map([["b", 2]]));
  const unchanged = withMember(map, "c", null);
  assert.deepEqual(unchanged, map);
  assert.notEqual(unchanged, map);
  assert.equal(map.size, 2);
});

test("withItem adds or removes an item in a copy", () => {
  const set: ReadonlySet<string> = new Set(["a"]);
  assert.deepEqual(withItem(set, "b", true), new Set(["a", "b"]));
  assert.deepEqual(withItem(set, "a", false), new Set());
  const unchanged = withItem(set, "a", true);
  assert.deepEqual(unchanged, set);
  assert.notEqual(unchanged, set);
  assert.deepEqual(withItem(set, "c", false), set);
  assert.deepEqual(set, new Set(["a"]));
});
