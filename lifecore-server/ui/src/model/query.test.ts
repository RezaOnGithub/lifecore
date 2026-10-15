import assert from "node:assert/strict";
import { test } from "node:test";
import {
  addSeed,
  defaultExpansion,
  type ExploreAction,
  type ExploreForm,
  type ExploreState,
  emptyForm,
  exploreStep,
  formChanged,
  fromHere,
  neighboursForm,
  parseNames,
  querySummary,
  readForm,
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

const blank: ExploreState = { form: emptyForm, ran: null };

// The state an Explore step leads to, whatever it does to the history.
function after(state: ExploreState, action: ExploreAction): ExploreState {
  const step = exploreStep(state, action);
  return step.kind === "none" ? state : step.state;
}

test("seeding runs the query and the form is compared with the one run", () => {
  // Nothing to run without a seed.
  assert.deepEqual(exploreStep(blank, { kind: "run" }), { kind: "none" });
  assert.equal(formChanged(blank), false);
  const seeded = after(blank, { kind: "seed", id: id("1") });
  assert.deepEqual(seeded.form.ids, [id("1")]);
  assert.deepEqual(seeded.ran, { form: seeded.form, run: 0 });
  assert.equal(formChanged(seeded), false);
  const changed = after(seeded, {
    kind: "change",
    form: {
      ...seeded.form,
      edges: { ...seeded.form.edges, currentness: "all" },
    },
  });
  assert.equal(formChanged(changed), true);
  assert.equal(changed.ran, seeded.ran);
  const rerun = after(changed, { kind: "run" });
  assert.equal(formChanged(rerun), false);
  assert.equal(rerun.ran?.form.edges.currentness, "all");
  // An unchanged form runs again under a new counter.
  assert.equal(after(rerun, { kind: "run" }).ran?.run, 1);
});

test("a new query is a Back step that leaves the previous query as it ran", () => {
  const first = after(blank, { kind: "seed", id: id("1") });
  // Editing the form stays on the step.
  const edited: ExploreForm = {
    ...first.form,
    expand: [{ ...defaultExpansion(), hops: 2 }],
  };
  const change = exploreStep(first, { kind: "change", form: edited });
  assert.equal(change.kind, "replace");
  const typing = after(first, { kind: "change", form: edited });
  // Running it pushes a step; the step left shows its own query again,
  // not the edits that became the next one.
  const run = exploreStep(typing, { kind: "run" });
  assert.equal(run.kind, "push");
  if (run.kind === "push") {
    assert.deepEqual(run.leaving, first);
    assert.deepEqual(run.state.ran, { form: edited, run: 0 });
  }
  // A rerun of the query on show is not a step.
  assert.equal(exploreStep(first, { kind: "run" }).kind, "replace");
  // Focus, + seed, and a saved view of another query are steps too.
  for (const action of [
    { kind: "focus", id: id("2") },
    { kind: "seed", id: id("2") },
    { kind: "load", form: addSeed(emptyForm, id("3")) },
  ] as const) {
    const step = exploreStep(first, action);
    assert.equal(step.kind, "push", action.kind);
    if (step.kind === "push") {
      assert.deepEqual(step.leaving, first, action.kind);
    }
  }
  // A saved view of the query on show runs it again in place.
  assert.equal(
    exploreStep(first, { kind: "load", form: first.form }).kind,
    "replace",
  );
});

test("Explore from here replaces the seeds and resets the steps and filters", () => {
  const form = fromHere(id("9"));
  assert.deepEqual(form, { ...emptyForm, ids: [id("9")] });
  assert.deepEqual(toParams(form), {
    seeds: [{ kind: "ids", ids: [id("9")] }],
    expand: [{ hops: 1, direction: "both", currentness: "current" }],
    edges: { currentness: "current" },
  });
  // A hub cell's query: one step over one type and direction.
  assert.deepEqual(
    toParams(neighboursForm(id("9"), "PartOf", "incoming", "all")),
    {
      seeds: [{ kind: "ids", ids: [id("9")] }],
      expand: [
        {
          hops: 1,
          direction: "incoming",
          currentness: "all",
          ty_names: ["PartOf"],
        },
      ],
      edges: { currentness: "all" },
    },
  );
});

test("names split on commas and whitespace", () => {
  assert.deepEqual(parseNames(" a, b  c,,"), ["a", "b", "c"]);
  assert.deepEqual(parseNames(""), []);
});

test("focus replaces every seed and keeps the expansion; load runs a form", () => {
  const form = {
    ...addSeed(addSeed(emptyForm, id("1")), id("2")),
    nodes: { ...emptyForm.nodes, enabled: true, ty_name: "Task" },
    expand: [{ ...defaultExpansion(), hops: 2 }],
  };
  const changed = after(blank, { kind: "change", form });
  const focused = after(changed, { kind: "focus", id: id("3") });
  assert.deepEqual(focused.form.ids, [id("3")]);
  assert.equal(focused.form.nodes.enabled, false);
  assert.equal(focused.form.nodes.ty_name, "Task");
  const params = focused.ran === null ? null : toParams(focused.ran.form);
  assert.deepEqual(params?.seeds, [{ kind: "ids", ids: [id("3")] }]);
  assert.equal(params?.expand?.[0]?.hops, 2);
  const loaded = after(focused, { kind: "load", form });
  assert.equal(loaded.form, form);
  assert.equal(formChanged(loaded), false);
  assert.equal(loaded.ran?.run, 0);
});

test("a stored form reads back only when every member is well formed", () => {
  const form = {
    ...addSeed(emptyForm, id("7")),
    nodes: { ...emptyForm.nodes, enabled: true, text: "hinge" },
  };
  assert.deepEqual(readForm(JSON.parse(JSON.stringify(form))), form);
  assert.equal(readForm(null), null);
  assert.equal(
    readForm({ ...form, ids: [{ origin: "o", position: 7 }] }),
    null,
  );
  assert.equal(readForm({ ...form, expand: [] }), null);
  assert.equal(
    readForm({ ...form, expand: [{ ...form.expand[0], hops: 9 }] }),
    null,
  );
  assert.equal(
    readForm({ ...form, edges: { ty_names: "", currentness: "some" } }),
    null,
  );
});

test("a folded form reads as one line", () => {
  const form = {
    ...addSeed(emptyForm, id("4")),
    nodes: { ...emptyForm.nodes, enabled: true, ty_name: "Task", text: "x" },
    expand: [
      { ...defaultExpansion(), hops: 2, ty_names: "PartOf DependsOn" },
      { ...defaultExpansion(), direction: "incoming" as const },
    ],
  };
  assert.equal(
    querySummary(form),
    '@4 + current Task containing "x", then 2 hops both over PartOf, DependsOn, then 1 hop incoming',
  );
  assert.equal(querySummary(emptyForm), "no seeds, then 1 hop both");
  // Seeds read as the user knows them where a label is known.
  const labels = new Map([["4", "Fix hinge"]]);
  assert.equal(
    querySummary(addSeed(form, id("5")), (seed) => labels.get(seed.position)),
    'Fix hinge @4 + @5 + current Task containing "x", then 2 hops both over PartOf, DependsOn, then 1 hop incoming',
  );
});
