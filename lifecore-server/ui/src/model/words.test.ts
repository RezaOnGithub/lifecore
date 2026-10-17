import assert from "node:assert/strict";
import { test } from "node:test";
import { counted, plural } from "./words.ts";

test("a count of one keeps the noun, any other takes an English plural", () => {
  assert.equal(plural("Day", 2), "Days");
  assert.equal(plural("Address", 2), "Addresses");
  assert.equal(plural("Story", 0), "Stories");
  assert.equal(plural("Story", 1), "Story");
  assert.equal(counted(1, "inconsistency"), "1 inconsistency");
  assert.equal(counted(3, "inconsistency"), "3 inconsistencies");
  assert.equal(counted(0, "warning"), "0 warnings");
  assert.equal(counted("1", "entry"), "1 entry");
  assert.equal(
    counted("18446744073709551615", "entry"),
    "18446744073709551615 entries",
  );
});
