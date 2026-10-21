import assert from "node:assert/strict";
import { test } from "node:test";
import type { ProposalEntry } from "../contract.ts";
import {
  type Alias,
  type AppliedName,
  aliasIndex,
  aliasNameError,
  moveAliases,
  namesFor,
  readAliases,
  recordApplied,
  renameAlias,
  takenName,
  withAlias,
  withoutAlias,
} from "./aliases.ts";

const id = (position: string) => ({ origin: "o", position });

const alias = (name: string, position: string, ty_name = "Room"): Alias => ({
  name,
  id: id(position),
  kind: "node",
  ty_name,
});

const applied = (
  name: string,
  position: string,
  more: Partial<AppliedName> = {},
): AppliedName => ({
  ...alias(name, position),
  follows: null,
  derived: false,
  ...more,
});

test("a name names one entry, kept in name order, and an entry may have several", () => {
  let aliases = withAlias([], alias("pantry", "4"));
  aliases = withAlias(aliases, alias("kitchen", "2"));
  aliases = withAlias(aliases, alias("cooking", "2"));
  assert.deepEqual(
    aliases.map((a) => a.name),
    ["cooking", "kitchen", "pantry"],
  );
  assert.deepEqual(namesFor(aliases, id("2")), ["cooking", "kitchen"]);
  // Naming another entry moves the name; the same alias again changes
  // nothing, not even the list's identity.
  const moved = withAlias(aliases, alias("kitchen", "9"));
  assert.deepEqual(namesFor(moved, id("2")), ["cooking"]);
  assert.deepEqual(namesFor(moved, id("9")), ["kitchen"]);
  assert.equal(withAlias(moved, alias("kitchen", "9")), moved);
  assert.deepEqual(namesFor(withoutAlias(moved, "cooking"), id("2")), []);
  assert.equal(withoutAlias(moved, "absent"), moved);
});

test("a name must follow the grammar and be free or already this entry's", () => {
  const aliases = [alias("kitchen", "2")];
  assert.equal(aliasNameError(aliases, "kitchen", id("2")), null);
  assert.equal(aliasNameError(aliases, "pantry", id("2")), null);
  assert.equal(
    aliasNameError(aliases, "kitchen", id("3")),
    "kitchen already names @2",
  );
  assert.match(aliasNameError(aliases, "two words", id("2")) ?? "", /match/);
  assert.equal(aliasNameError(aliases, "", id("2")), "name required");
});

test("a rename keeps the entry and is refused onto a taken or malformed name", () => {
  const aliases = [alias("kitchen", "2"), alias("pantry", "4")];
  const renamed = renameAlias(aliases, "kitchen", "galley");
  assert.deepEqual(
    renamed.map((a) => [a.name, a.id.position]),
    [
      ["galley", "2"],
      ["pantry", "4"],
    ],
  );
  assert.equal(renameAlias(aliases, "kitchen", "pantry"), aliases);
  assert.equal(renameAlias(aliases, "kitchen", "no good"), aliases);
  assert.equal(renameAlias(aliases, "absent", "galley"), aliases);
});

test("moving passes every name of an entry to another", () => {
  const aliases = [
    alias("cooking", "2"),
    alias("kitchen", "2"),
    alias("pantry", "4"),
  ];
  const moved = moveAliases(aliases, id("2"), {
    id: id("7"),
    kind: "node",
    ty_name: "Room",
  });
  assert.deepEqual(namesFor(moved, id("7")), ["cooking", "kitchen"]);
  assert.deepEqual(namesFor(moved, id("4")), ["pantry"]);
  assert.equal(
    moveAliases(aliases, id("5"), {
      id: id("7"),
      kind: "node",
      ty_name: "Room",
    }),
    aliases,
  );
});

test("an apply names its entries, passes names along revisions, and takes names in use", () => {
  const before = [alias("kitchen", "2"), alias("pantry", "4")];
  const after = recordApplied(before, [
    // A revise of the kitchen, then a revise of that revision in the same
    // apply: the kitchen's names reach the end of the chain.
    applied("galley", "10", { follows: id("2") }),
    applied("galley_v2", "11", { follows: id("10") }),
    // A plain new node takes the name pantry from @4.
    applied("pantry", "12"),
  ]);
  assert.deepEqual(
    after.map((a) => [a.name, a.id.position]),
    [
      ["galley", "11"],
      ["galley_v2", "11"],
      ["kitchen", "11"],
      ["pantry", "12"],
    ],
  );
});

test("a kept edge passes on its edge's names and records none of its own", () => {
  const edge: Alias = {
    name: "supplies",
    id: id("3"),
    kind: "edge",
    ty_name: "Supplies",
  };
  const after = recordApplied(
    [edge],
    [
      {
        name: "galley_keep_3",
        id: id("11"),
        kind: "edge",
        ty_name: "Supplies",
        follows: id("3"),
        derived: true,
      },
    ],
  );
  assert.deepEqual(after, [{ ...edge, id: id("11") }]);
});

test("positions show names only for entries inside the as-of boundary", () => {
  const aliases = [alias("kitchen", "2"), alias("galley", "5")];
  const whole = aliasIndex(aliases, null);
  assert.deepEqual(whole.get("o:2"), ["kitchen"]);
  assert.deepEqual(whole.get("o:5"), ["galley"]);
  // Just before the append of @5, and just after it.
  assert.equal(aliasIndex(aliases, "5").get("o:5"), undefined);
  assert.deepEqual(aliasIndex(aliases, "5").get("o:2"), ["kitchen"]);
  assert.deepEqual(aliasIndex(aliases, "6").get("o:5"), ["galley"]);
  assert.equal(aliasIndex(aliases, "2").size, 0);
});

test("an operation takes a name from another entry unless it revises that entry", () => {
  const aliases = [alias("kitchen", "2")];
  type NodeOperation = Extract<ProposalEntry, { kind: "node" }>;
  const node = (more: Partial<NodeOperation>): NodeOperation => ({
    name: "kitchen",
    kind: "node",
    ty_name: "Room",
    payload: {},
    ...more,
  });
  const revise = node({ predecessor: { existing: id("2") }, intent: "revise" });
  assert.equal(takenName(aliases, revise), null);
  assert.equal(
    takenName(aliases, node({ ...revise, intent: "branch" }))?.name,
    "kitchen",
  );
  assert.equal(
    takenName(
      aliases,
      node({ predecessor: { existing: id("3") }, intent: "revise" }),
    )?.name,
    "kitchen",
  );
  assert.equal(takenName(aliases, node({}))?.name, "kitchen");
  assert.equal(takenName(aliases, node({ name: "pantry" })), null);
});

test("a stored list drops malformed items one by one and keeps the first of a name", () => {
  assert.deepEqual(readAliases(null), []);
  assert.deepEqual(
    readAliases([
      alias("pantry", "4"),
      { ...alias("kitchen", "2"), kind: "room" },
      { ...alias("bad name", "2") },
      { ...alias("cellar", "2"), id: { origin: "o", position: "-1" } },
      alias("galley", "5"),
      alias("galley", "6"),
      "kitchen",
    ]),
    [alias("galley", "5"), alias("pantry", "4")],
  );
});
