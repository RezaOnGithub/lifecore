import {
  expect,
  type Page,
  type Request,
  type Route,
  test,
} from "@playwright/test";
import type {
  EntryId,
  Method,
  Params,
  Payload,
  ProposalEntry,
  Result,
  RpcResponse,
} from "../src/contract.ts";

// The Plan screen: the planning valuation of a plan this file records,
// opened from the sidebar under Findings, by tier with the server's reason
// text, linking into the inspector, and following the as-of boundary. The
// suite holds only TypeCheck; the valuation ranks without Planning.
// The file declares planning's types except `Task`: browse.spec.ts, which
// runs earlier, declares a `Task` of its own without planning's labels, and
// a second current declaration would make the name ambiguous, so TypeCheck
// would reject `Task` entries. The plan therefore has no tasks and tier 1
// stays empty. Browse's tasks rank anyway, in tier 0, since the valuation
// goes by type name, so rows are found by their labels. The plan: three
// windows, sealed by two reviews, the last one open at the head.
test.describe.configure({ mode: "serial" });

function launched(): { page: string; rpc: string; token: string } {
  const url = process.env.LIFECORE_URL;
  if (!url) {
    throw new Error("the global setup did not start lifecore serve");
  }
  const base = url.slice(0, url.indexOf("#"));
  const token = new URLSearchParams(url.slice(url.indexOf("#") + 1)).get(
    "token",
  );
  if (!token) {
    throw new Error(`no token in ${url}`);
  }
  return { page: url, rpc: `${base}rpc`, token };
}

async function call<M extends Method>(
  method: M,
  params: Params<M>,
): Promise<Result<M>> {
  const { rpc, token } = launched();
  const response = await fetch(rpc, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  const body = (await response.json()) as RpcResponse<M>;
  if ("error" in body) {
    throw new Error(`${method}: ${JSON.stringify(body.error)}`);
  }
  return body.result;
}

const at = (date: string): Payload => ({
  kind: "timestamp",
  value: `${date}T09:00:00Z`,
});
const integer = (value: string): Payload => ({ kind: "integer", value });

// Planning's canonical declaration of `name` (PLANNING §11).
function declaration(
  name: string,
  fields: Record<string, Payload>,
  outgoing: string[],
  incoming: string[],
): ProposalEntry {
  return {
    name: `${name.toLowerCase()}_type`,
    kind: "node",
    ty_name: "Type",
    payload: {
      name,
      fields: { kind: "record", value: fields },
      outgoing,
      incoming,
      successors: [],
    },
  };
}

const node = (
  name: string,
  ty_name: string,
  payload: Record<string, Payload>,
): ProposalEntry => ({ name, kind: "node", ty_name, payload });

const edge = (
  name: string,
  ty_name: string,
  from: string,
  to: string,
): ProposalEntry => ({
  name,
  kind: "edge",
  ty_name,
  from: { added: from },
  to: { added: to },
});

const title: Payload = ["mandatory", "text"];
const entries: ProposalEntry[] = [
  declaration(
    "Goal",
    {
      title,
      criterion: ["optional", "text"],
      deadline: ["optional", "timestamp"],
    },
    [],
    ["Enables", "Helps", "About"],
  ),
  declaration(
    "Project",
    {
      title,
      deliverable: ["optional", "text"],
      deadline: ["optional", "timestamp"],
    },
    ["Enables", "Helps"],
    ["Enables", "Helps", "About", "Until"],
  ),
  declaration(
    "Habit",
    { title, quota: ["mandatory", "integer"] },
    ["Helps"],
    ["About"],
  ),
  declaration("External", { title }, ["Enables", "Helps"], ["About", "Until"]),
  declaration(
    "Stance",
    {
      kind: ["mandatory", "text"],
      at: ["mandatory", "timestamp"],
      cost: ["optional", "integer"],
      horizon: ["optional", "integer"],
      reason: ["optional", "text"],
    },
    ["About", "Until"],
    [],
  ),
  declaration(
    "Did",
    { at: ["mandatory", "timestamp"], note: ["optional", "text"] },
    ["About"],
    [],
  ),
  declaration(
    "Review",
    {
      budget: ["mandatory", "integer"],
      at: ["mandatory", "timestamp"],
      note: ["optional", "text"],
    },
    [],
    [],
  ),
  // Window 0: a goal with no plan, a move waiting on a permit, a run that
  // helps a race, and a deferred goal.
  node("cook", "Goal", { title: "Learn to cook" }),
  node("move", "Goal", { title: "Move to Lisbon" }),
  node("flat", "Project", { title: "Find a flat in Lisbon" }),
  edge("flat_move", "Enables", "flat", "move"),
  node("permit", "External", { title: "Residence permit" }),
  edge("permit_flat", "Enables", "permit", "flat"),
  node("race", "Goal", { title: "Run a half marathon" }),
  node("run", "Habit", { title: "Run", quota: integer("3") }),
  edge("run_race", "Helps", "run", "race"),
  node("guitar", "Goal", { title: "Learn the guitar" }),
  node("later", "Stance", {
    kind: "deferred",
    horizon: integer("2"),
    at: at("2026-01-04"),
  }),
  edge("later_about", "About", "later", "guitar"),
  node("wait", "Stance", {
    kind: "waiting",
    horizon: integer("3"),
    at: at("2026-01-04"),
  }),
  edge("wait_about", "About", "wait", "permit"),
  node("first_review", "Review", {
    budget: integer("5"),
    at: at("2026-01-05"),
  }),
  // Window 1.
  node("ran", "Did", { at: at("2026-01-07") }),
  edge("ran_about", "About", "ran", "run"),
  node("second_review", "Review", {
    budget: integer("5"),
    at: at("2026-01-12"),
  }),
  // Window 2, open at the head.
  node("ran_again", "Did", { at: at("2026-01-14") }),
  edge("ran_again_about", "About", "ran_again", "run"),
  node("stretch", "Habit", { title: "Stretch", quota: integer("2") }),
];

let page: Page;
let ids: Record<string, EntryId>;
let head: string;

const position = (name: string) => ids[name]?.position ?? "";

test.beforeAll(async ({ browser }) => {
  const applied = await call("proposal.apply", { proposal: { entries } });
  ids = Object.fromEntries(
    applied.report.operations.map((operation) => [
      operation.name,
      operation.entry,
    ]),
  );
  head = (await call("database.describe", {})).view.end_exclusive;
  page = await browser.newPage();
  await page.goto(launched().page);
  await expect(page.getByRole("heading", { name: "LifeCore" })).toBeVisible();
});

test.afterAll(async () => {
  await page?.close();
});

const main = () => page.getByRole("main");
const tier = (n: number) => main().getByTestId(`tier-${n}`);
const rows = (n: number) => tier(n).getByTestId("ranked");
const row = (n: number, label: string) =>
  rows(n).filter({
    has: page.getByRole("button", { name: label, exact: true }),
  });
const reason = (n: number, label: string, name: string) =>
  row(n, label).locator(`[data-reason="${name}"]`);

async function setAsOf(boundary: string | null) {
  const banner = page.getByRole("banner");
  if (boundary === null) {
    await banner.getByRole("button", { name: "current", exact: true }).click();
    await expect(page.getByTestId("as-of")).toHaveCount(0);
    return;
  }
  await banner.getByLabel("as of").fill(boundary);
  await banner.getByRole("button", { name: "Set", exact: true }).click();
  await expect(page.getByTestId("as-of")).toContainText(`[0, ${boundary})`);
}

const methodOf = (route: Route) =>
  (route.request().postDataJSON() as { method?: string } | null)?.method;

test("the sidebar opens Plan under Findings, by tier with the server's reasons", async () => {
  await page
    .getByRole("navigation")
    .getByRole("button", { name: "Plan", exact: true })
    .click();
  await expect(
    main().getByRole("heading", { name: "Plan", exact: true }),
  ).toBeVisible();
  await expect(main().getByTestId("valuation-view")).toHaveText(
    `view [0, ${head})`,
  );
  // Only TypeCheck runs here, which says nothing about the ranking.
  await expect(main().getByTestId("planning-unconfigured")).toBeVisible();
  // Goals first in tier 0; browse's undecided tasks follow.
  await expect(rows(0).first()).toContainText("Learn to cook");
  await expect(rows(0).first()).toContainText(`@${position("cook")}`);
  await expect(rows(0).first()).toContainText("Goal");
  await expect(reason(0, "Learn to cook", "broken_plan")).toHaveText(
    "broken plan: nothing in its strict subgraph can advance it",
  );
  await expect(reason(0, "Learn to cook", "broken_plan")).toHaveClass("urgent");
  await expect(reason(2, "Run", "shortfall")).toHaveText(
    "1 of 3 so far this window",
  );
  // The deferral lapsing at the next seal comes first.
  await expect(rows(3)).toHaveCount(2);
  await expect(rows(3).nth(0)).toContainText("Learn the guitar");
  await expect(row(3, "Learn the guitar").locator("[data-reason]")).toHaveText([
    "deferred, deciding the next 1 seal",
    "lapses at the next seal",
    "open for 2 seals",
  ]);
  await expect(
    reason(3, "Learn the guitar", "lapses_at_next_seal"),
  ).toHaveClass("soon");
  await expect(row(3, "Residence permit").locator("[data-reason]")).toHaveText([
    "waiting, deciding the next 2 seals",
    "open for 2 seals",
  ]);
  await expect(rows(4)).toHaveText([
    /Move to Lisbon.*no commitment toward it sealed for 1 seal/,
    /Find a flat in Lisbon.*no commitment toward it sealed for 1 seal/,
    /Run a half marathon.*no commitment toward it sealed for 1 seal/,
  ]);
  await expect(reason(5, "Stretch", "new")).toHaveText(
    "new: the next seal gives it a grace period",
  );
  await expect(tier(1)).toHaveCount(0);
  await expect(main().getByTestId("empty-tiers")).toHaveText(
    "Nothing in tier 1 (standing commitments).",
  );
});

test("a row opens the inspector, and Back shows the ranking again", async () => {
  await row(4, "Move to Lisbon")
    .getByRole("button", { name: "Move to Lisbon", exact: true })
    .click();
  await expect(
    main().getByRole("heading", { name: "Move to Lisbon" }),
  ).toBeVisible();
  await expect(page.getByTestId("trail")).toContainText("Plan");
  await page.getByRole("button", { name: "← Back" }).click();
  await expect(
    main().getByRole("heading", { name: "Plan", exact: true }),
  ).toBeVisible();
  await expect(rows(4)).toHaveCount(3);
});

test("the ranking follows the as-of boundary, down to an empty view", async () => {
  // Before the second review: the first one has sealed, the second not yet.
  await setAsOf(position("second_review"));
  await expect(main().getByTestId("valuation-view")).toHaveText(
    `view [0, ${position("second_review")})`,
  );
  await expect(row(3, "Learn the guitar").locator("[data-reason]")).toHaveText([
    "deferred, deciding the next 2 seals",
    "open for 1 seal",
  ]);
  await expect(rows(4).first()).toContainText(
    "no commitment toward it sealed for 0 seals",
  );
  await expect(main().getByRole("button", { name: "Stretch" })).toHaveCount(0);
  // Before the first review, nothing is sealed: the deferral is fresh, so
  // the guitar is dormant and unranked, and the run is new.
  await setAsOf(position("first_review"));
  await expect(main().getByTestId("valuation-view")).toHaveText(
    `view [0, ${position("first_review")})`,
  );
  await expect(main().getByTestId("empty-tiers")).toHaveText(
    "Nothing in tier 1 (standing commitments), tier 2 (habits and concerns below quota), tier 3 (deferred or waiting), or tier 4 (starved goals and projects).",
  );
  await expect(reason(5, "Run", "new")).toBeVisible();
  await expect(row(5, "Move to Lisbon")).toHaveCount(1);
  await expect(
    main().getByRole("button", { name: "Learn the guitar" }),
  ).toHaveCount(0);
  await expect(rows(0).first()).toContainText("Learn to cook");
  // Before any entry, nothing ranks.
  await setAsOf("0");
  await expect(main().getByTestId("nothing-ranks")).toContainText(
    "Nothing ranks in the view [0, 0)",
  );
  await setAsOf(null);
  await expect(main().getByTestId("valuation-view")).toHaveText(
    `view [0, ${head})`,
  );
  await expect(rows(3)).toHaveCount(2);
});

type SampleParams = { deadline_sample_windows?: number };

// The params of a `planning.valuation` request, else null.
function valuationParams(request: Request): SampleParams | null {
  let body: { method?: string; params?: SampleParams } | null = null;
  try {
    body = request.postDataJSON();
  } catch {
    return null;
  }
  return body?.method === "planning.valuation" ? (body.params ?? {}) : null;
}

const sent = (windows: number | undefined) =>
  page.waitForRequest(
    (request) => valuationParams(request)?.deadline_sample_windows === windows,
  );

// Every `planning.valuation` request's params from now until `stop`.
function watchValuations() {
  const seen: SampleParams[] = [];
  const listener = (request: Request) => {
    const params = valuationParams(request);
    if (params !== null) {
      seen.push(params);
    }
  };
  page.on("request", listener);
  return { seen, stop: () => page.off("request", listener) };
}

// Longer than the pause after which typing applies.
const PAUSE = 800;

test("the deadline sample applies as typed, and text that is not a number keeps it, across Back too", async () => {
  const sample = main().getByLabel("deadline sample windows");
  const three = sent(3);
  await sample.fill("3");
  await three;
  // Text that is not a number sends nothing, however long it stays.
  const quiet = watchValuations();
  await sample.fill("3x");
  await expect(main().getByTestId("sample-invalid")).toHaveText(
    "a whole number of windows, 0 or more",
  );
  await expect(sample).toHaveAttribute("aria-invalid", "true");
  await page.waitForTimeout(PAUSE);
  quiet.stop();
  expect(quiet.seen).toEqual([]);
  // A step returned to reads with the sample it applied, under the text
  // as it was left.
  await row(4, "Move to Lisbon")
    .getByRole("button", { name: "Move to Lisbon", exact: true })
    .click();
  await expect(
    main().getByRole("heading", { name: "Move to Lisbon" }),
  ).toBeVisible();
  const back = watchValuations();
  await page.getByRole("button", { name: "← Back" }).click();
  await expect(sample).toHaveValue("3x");
  await expect(main().getByTestId("sample-invalid")).toBeVisible();
  await expect.poll(() => back.seen.length).toBeGreaterThan(0);
  await page.waitForTimeout(PAUSE);
  back.stop();
  for (const params of back.seen) {
    expect(params).toEqual({ deadline_sample_windows: 3 });
  }
  // Emptied, the field asks for the server's default.
  const cleared = sent(undefined);
  await sample.fill("");
  await cleared;
  await expect(main().getByTestId("sample-invalid")).toHaveCount(0);
  await expect(sample).toHaveAttribute("aria-invalid", "false");
});

test("a planning payload the ranking cannot read names its entry and field", async () => {
  const { rpc } = launched();
  const review = ids.first_review;
  await page.route(rpc, async (route) => {
    if (methodOf(route) !== "planning.valuation") {
      await route.fallback();
      return;
    }
    const request = route.request().postDataJSON() as { id: number };
    await route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: request.id,
        error: {
          code: 4007,
          message: "cannot rank the plan: field `at` of Review … is missing",
          data: {
            kind: "valuation_failed",
            entry: review,
            ty_name: "Review",
            field: "at",
            problem: { kind: "missing" },
          },
        },
      },
    });
  });
  await page
    .getByRole("banner")
    .getByRole("button", { name: "Refresh" })
    .click();
  const alert = main().getByRole("alert");
  await expect(
    alert.getByRole("heading", { name: "The plan cannot be ranked" }),
  ).toBeVisible();
  await expect(alert.getByRole("paragraph").first()).toHaveText(
    `Field at of Review @${position("first_review")} is missing, and the ranking reads that field.`,
  );
  await page.unroute(rpc);
  await alert
    .getByRole("button", { name: `@${position("first_review")}` })
    .click();
  await expect(main().getByTestId("inspector")).toContainText(
    `@${position("first_review")}`,
  );
  await expect(main().getByRole("heading").first()).toContainText("Review");
  await page.getByRole("button", { name: "← Back" }).click();
  await expect(rows(3)).toHaveCount(2);
});
