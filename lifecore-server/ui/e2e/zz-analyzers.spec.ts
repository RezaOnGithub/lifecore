import { expect, type Page, type Route, test } from "@playwright/test";
import type {
  EntryId,
  Method,
  Params,
  ProposalEntry,
  Result,
  RpcResponse,
} from "../src/contract.ts";

// Adding an analyzer from the Database screen. The file builds a cycle
// among its own `Ring` nodes, so a cycle check that calls cycles
// inconsistencies is rejected over the existing history, and one that only
// warns is added. Adding changes the shared database's suite for every
// later file, so this file is named to run last; what it adds only warns.
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

const precedes = (name: string, from: string, to: string): ProposalEntry => ({
  name,
  kind: "edge",
  ty_name: "Precedes",
  from: { added: from },
  to: { added: to },
});

const entries: ProposalEntry[] = [
  {
    name: "ring_type",
    kind: "node",
    ty_name: "Type",
    payload: {
      name: "Ring",
      fields: {
        kind: "record",
        value: { title: ["mandatory", "text"] },
      },
      outgoing: ["Precedes"],
      incoming: ["Precedes"],
      successors: [],
    },
  },
  {
    name: "ring_a",
    kind: "node",
    ty_name: "Ring",
    payload: { title: "Ring A" },
  },
  {
    name: "ring_b",
    kind: "node",
    ty_name: "Ring",
    payload: { title: "Ring B" },
  },
  precedes("a_to_b", "ring_a", "ring_b"),
  precedes("b_to_a", "ring_b", "ring_a"),
];

const INCONSISTENT = '{"on_cycle":"inconsistency"}';
// Spaces kept: the configuration is sent and stored as typed.
const WARNING = '{ "on_cycle": "warning" }';

let page: Page;
let ids: Record<string, EntryId>;
// The registered cycle checker, which takes the configuration this file
// sends; the host also registers Reconnect and Planning.
const checker = "DAG_Check";

test.beforeAll(async ({ browser }) => {
  const applied = await call("proposal.apply", { proposal: { entries } });
  ids = Object.fromEntries(
    applied.report.operations.map((operation) => [
      operation.name,
      operation.entry,
    ]),
  );
  const { registered, suite } = await call("database.describe", {});
  expect(registered).toEqual([checker, "Planning", "Reconnect", "Type_Check"]);
  expect(suite.map((config) => config.name)).toEqual(["Type_Check"]);
  page = await browser.newPage();
  await page.goto(launched().page);
  await expect(page.getByRole("heading", { name: "LifeCore" })).toBeVisible();
});

test.afterAll(async () => {
  await page?.close();
});

const main = () => page.getByRole("main");
const position = (name: string) => ids[name]?.position ?? "";

async function storedSuite() {
  return (await call("database.describe", {})).suite;
}

// Opens the form on the Database screen and fills it.
async function fill(configuration: string) {
  await expect(
    main().getByRole("heading", { name: "Database", exact: true }),
  ).toBeVisible();
  const opener = main().getByRole("button", { name: "Add an analyzer…" });
  if (await opener.isVisible()) {
    await opener.click();
  }
  await main()
    .getByRole("combobox", { name: "analyzer", exact: true })
    .selectOption(checker);
  await main()
    .getByRole("textbox", { name: "configuration", exact: true })
    .fill(configuration);
}

// "Add…" asks first; the confirmation states what would be appended.
async function confirm(configuration: string) {
  await main().getByRole("button", { name: "Add…", exact: true }).click();
  const confirmation = main().getByTestId("add-confirmation");
  await expect(confirmation).toContainText(checker);
  await expect(confirmation.locator("code.verbatim")).toHaveText(configuration);
  await expect(confirmation.getByTestId("add-position")).toHaveText(
    "position 2 of the suite as last read, after Type_Check",
  );
  await expect(confirmation).toContainText("no removal or reconfiguration");
  await expect(confirmation.getByTestId("add-plan-note")).toContainText(
    "Replan",
  );
  await confirmation
    .getByRole("button", { name: `Add ${checker}`, exact: true })
    .click();
  await expect(confirmation).toHaveCount(0);
}

test("a configuration the analyzer refuses is an analyzer failure", async () => {
  await fill("{}");
  await confirm("{}");
  const failure = main().getByTestId("add-failure");
  await expect(failure).toContainText("Not added: analyzer failure.");
  await expect(failure).toContainText("on_cycle");
  expect(await storedSuite()).toHaveLength(1);
  await failure.getByRole("button", { name: "Dismiss" }).click();
  await expect(failure).toHaveCount(0);
});

test("an analyzer that finds an inconsistency in history is not added", async () => {
  await fill(INCONSISTENT);
  await confirm(INCONSISTENT);
  const rejected = main().getByTestId("add-rejected");
  await expect(rejected).toContainText("Not added.");
  // Other files' cycles, if any, are listed too; this file's is one row,
  // focused on its first node, by label.
  const cycle = rejected
    .getByTestId("add-inconsistencies")
    .locator("li.finding")
    .filter({
      hasText: `nodes [${position("ring_a")}, ${position("ring_b")}] and edges [${position("a_to_b")}, ${position("b_to_a")}]`,
    });
  await expect(cycle).toHaveCount(1);
  await expect(cycle.locator(".badge.kind-inconsistency")).toHaveCount(1);
  await expect(cycle).toContainText(checker);
  await expect(cycle.getByTestId("range")).toHaveText(
    `since ${Number(position("b_to_a")) + 1}`,
  );
  // Labels are read for the entries it names: its focus, then its subjects.
  await expect(cycle.locator(".chips > li").first()).toContainText(
    "focus Ring A",
  );
  await expect(cycle.getByRole("button", { name: "Ring B" })).toBeVisible();
  await expect(rejected.getByTestId("add-other-findings")).toContainText(
    "not listed",
  );
  expect(await storedSuite()).toHaveLength(1);
  await expect(main().getByTestId("suite").locator("tbody tr")).toHaveCount(1);
  // The outcome survives a detour through the inspector.
  await cycle.getByRole("button", { name: "Ring A" }).first().click();
  await expect(main().getByRole("heading", { name: "Ring A" })).toBeVisible();
  await page
    .getByRole("banner")
    .getByRole("button", { name: "← Back" })
    .click();
  await expect(main().getByTestId("add-rejected")).toBeVisible();
});

test("a lost answer may have added it, and Add waits for a Refresh", async () => {
  const { rpc } = launched();
  const method = (route: Route) =>
    (route.request().postDataJSON() as { method?: string } | null)?.method;
  // The request reaches the server, which rejects it, but the answer is
  // lost on the way back.
  await page.route(rpc, async (route) => {
    if (method(route) !== "analyzers.add") {
      await route.fallback();
      return;
    }
    await route.fetch();
    await route.abort();
  });
  await fill(INCONSISTENT);
  await confirm(INCONSISTENT);
  const uncertain = main().getByTestId("add-uncertain");
  await expect(uncertain).toContainText("The analyzer may have been added.");
  await expect(main().getByTestId("add-rejected")).toHaveCount(0);
  await expect(
    main().getByRole("button", { name: "Add…", exact: true }),
  ).toBeDisabled();
  await expect(main().getByTestId("add-blocker")).toHaveText(
    "the last add may have been stored: Refresh first",
  );
  await expect(uncertain.getByRole("button", { name: "Dismiss" })).toHaveCount(
    0,
  );
  await page.unroute(rpc);
  await uncertain.getByRole("button", { name: "Refresh" }).click();
  await expect(main().getByTestId("add-verdict")).toHaveText(
    "The suite read since does not hold it: it was not added, unless the server is still running the call.",
  );
  await expect(
    main().getByRole("button", { name: "Add…", exact: true }),
  ).toBeEnabled();
  await main()
    .getByTestId("add-uncertain")
    .getByRole("button", { name: "Dismiss" })
    .click();
  await expect(main().getByTestId("add-uncertain")).toHaveCount(0);
});

test("an analyzer that only warns is added and the suite shows it", async () => {
  const revision = (await call("database.describe", {})).graph_revision;
  await fill(WARNING);
  await confirm(WARNING);
  const added = main().getByTestId("add-added");
  await expect(added).toContainText(`Added ${checker}`);
  await expect(added).toContainText("as analyzer 2 of the suite");
  await expect(added.getByTestId("added-suite").locator("li")).toHaveText([
    "Type_Check {}",
    `${checker} ${WARNING}`,
  ]);
  await expect(added.getByTestId("add-findings")).toContainText(
    new RegExp(
      `from ${checker}: 0 inconsistencies, [1-9]\\d* warnings?, 0 suggestions`,
    ),
  );
  const stored = await call("database.describe", {});
  expect(stored.suite).toEqual([
    { name: "Type_Check", configuration: "{}" },
    { name: checker, configuration: WARNING },
  ]);
  expect(stored.graph_revision).not.toBe(revision);
  // Every screen reloaded: the suite table and the revision are the new
  // ones.
  const rows = main().getByTestId("suite").locator("tbody tr");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(1)).toContainText(checker);
  await expect(rows.nth(1).locator("code.verbatim")).toHaveText(WARNING);
  await expect(main()).toContainText(stored.graph_revision);
});
