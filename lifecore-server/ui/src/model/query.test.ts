import assert from "node:assert/strict";
import { test } from "node:test";
import {
  addSeed,
  emptyForm,
  parseNames,
  removeSeed,
  toParams,
} from "./query.ts";

const id = (position: string) => ({ origin: "o", position });

test("a form without seeds has no query", () => {
  assert.equal(toParams(emptyForm), null);
});

test("seeds are added once and removed by identity", () => {
  const form = addSeed(addSeed(emptyForm, id("1")), id("1"));
  assert.deepEqual(form.ids, [id("1")]);
  assert.deepEqual(removeSeed(form, id("1")).ids, []);
});

test("the form becomes a query with only the members that are set", () => {
  const form = {
    ...addSeed(emptyForm, id("1")),
    nodes: {
      enabled: true,
      ty_name: " Task ",
      text: "",
      currentness: "all" as const,
    },
    expand: [
      {
        hops: 2,
        direction: "incoming" as const,
        ty_names: "DependsOn, Mentions",
        currentness: "all" as const,
      },
      {
        hops: 1,
        direction: "both" as const,
        ty_names: "",
        currentness: "current" as const,
      },
    ],
    edges: { ty_names: "", currentness: "historical" as const },
  };
  assert.deepEqual(toParams(form), {
    seeds: [
      { kind: "ids", ids: [id("1")] },
      { kind: "nodes", currentness: "all", ty_name: "Task" },
    ],
    expand: [
      {
        hops: 2,
        direction: "incoming",
        currentness: "all",
        ty_names: ["DependsOn", "Mentions"],
      },
      { hops: 1, direction: "both", currentness: "current" },
    ],
    edges: { currentness: "historical" },
  });
});

test("names split on commas and whitespace", () => {
  assert.deepEqual(parseNames(" a, b  c,,"), ["a", "b", "c"]);
  assert.deepEqual(parseNames(""), []);
});
