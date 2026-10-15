import { expect, type Page, test } from "@playwright/test";
import type {
  EntryId,
  Method,
  Params,
  ProposalEntry,
  Result,
  RpcResponse,
} from "../src/contract.ts";

// The browsing flow of Milestone 1 against a database this file fills: the
// steps build on one another and share one page, so navigation state kept
// in memory survives between them.
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

const taskType: ProposalEntry = {
  name: "task_type",
  kind: "node",
  ty_name: "Type",
  payload: {
    name: "Task",
    fields: {
      kind: "record",
      value: {
        title: ["mandatory", "text"],
        notes: ["encouraged", "text"],
        status: ["optional", "text"],
        priority: ["optional", "integer"],
        due: ["optional", "timestamp"],
        done: ["optional", "boolean"],
        tags: ["optional", "list", "text"],
        details: [
          "optional",
          "record",
          { kind: "record", value: { owner: ["mandatory", "text"] } },
        ],
      },
    },
    outgoing: ["DependsOn"],
    incoming: ["DependsOn"],
    successors: [],
  },
};

const entries: ProposalEntry[] = [
  taskType,
  {
    name: "report",
    kind: "node",
    ty_name: "Task",
    payload: {
      title: "Write the report",
      priority: { kind: "integer", value: "2" },
    },
  },
  {
    name: "review",
    kind: "node",
    ty_name: "Task",
    payload: { title: "Review the report", notes: "Ask Teto" },
  },
  {
    name: "publish",
    kind: "node",
    ty_name: "Task",
    payload: { title: "Publish the report", tags: ["release"] },
  },
  {
    name: "report_done",
    kind: "node",
    ty_name: "Task",
    predecessor: { added: "report" },
    intent: "revise",
    payload: {
      title: "Write the report",
      status: "done",
      priority: { kind: "integer", value: "2" },
    },
  },
  {
    name: "review_after_report",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "review" },
    to: { added: "report_done" },
  },
  {
    name: "publish_after_review",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "publish" },
    to: { added: "review" },
  },
];

let page: Page;
let ids: Record<string, EntryId>;

test.beforeAll(async ({ browser }) => {
  const applied = await call("proposal.apply", { proposal: { entries } });
  ids = Object.fromEntries(
    applied.report.operations.map((operation) => [
      operation.name,
      operation.entry,
    ]),
  );
  page = await browser.newPage();
  await page.goto(launched().page);
  await expect(page.getByRole("heading", { name: "LifeCore" })).toBeVisible();
});

test.afterAll(async () => {
  await page.close();
});

const sidebar = () => page.getByTestId("types");
const banner = () => page.getByRole("banner");
const main = () => page.getByRole("main");
// Explore stays mounted but hidden under other screens, so CSS locators are
// scoped to the screen on show.
const table = () => page.getByTestId("table");
const inspector = () => page.getByTestId("inspector");

test("the sidebar lists Task with its node counts and opens its table", async () => {
  const task = sidebar().getByRole("listitem").filter({ hasText: "Task" });
  await expect(task).toContainText("3/1");
  await task.getByRole("button", { name: "Task" }).click();
  await expect(main().getByRole("heading", { name: "Task" })).toBeVisible();
  const rows = table().locator("table.entries tbody tr");
  await expect(rows).toHaveCount(3);
  // Declared columns follow status order, alphabetical within a status.
  await expect(table().locator("table.entries thead th")).toContainText([
    "label",
    "@position",
    "current",
    "title",
    "notes",
    "details",
    "done",
    "due",
    "priority",
    "status",
    "tags",
  ]);
  const done = rows.filter({ hasText: "Write the report" });
  await expect(done).toContainText(`@${ids.report_done?.position}`);
  await expect(done).toContainText("done");
  await expect(done).toContainText("2");
});

test("historical rows appear on request and the text filter reaches the backend", async () => {
  await table().getByLabel("show").selectOption("all");
  await expect(table().locator("table.entries tbody tr")).toHaveCount(4);
  await expect(table().locator("tr.historical")).toContainText(
    `@${ids.report?.position}`,
  );
  await table().getByLabel("containing").fill("Review");
  await expect(table().locator("table.entries tbody tr")).toHaveCount(1);
  await table().getByLabel("containing").fill("");
  await table().getByLabel("show").selectOption("current");
  await expect(table().locator("table.entries tbody tr")).toHaveCount(3);
});

test("a row opens the inspector with payload, lineage, and relationships", async () => {
  await table().getByRole("button", { name: "Review the report" }).click();
  await expect(
    main().getByRole("heading", { name: "Review the report" }),
  ).toBeVisible();
  await expect(inspector().locator("table.payload")).toContainText("Ask Teto");
  await expect(inspector().locator("table.payload")).toContainText("absent");
  await expect(page.getByTestId("lineage").getByRole("listitem")).toHaveCount(
    1,
  );
  const relationships = page.getByTestId("relationships").locator("tbody tr");
  await expect(relationships).toHaveCount(2);
  await expect(relationships.nth(0)).toContainText("→");
  await expect(relationships.nth(0)).toContainText("Write the report");
  await expect(relationships.nth(1)).toContainText("←");
  await expect(relationships.nth(1)).toContainText("Publish the report");
  await expect(main().getByRole("button", { name: "Revise" })).toBeDisabled();
});

test("following a relationship shows the revised node's lineage", async () => {
  await page
    .getByTestId("relationships")
    .getByRole("button", { name: "Write the report" })
    .click();
  await expect(
    main().getByRole("heading", { name: "Write the report" }),
  ).toContainText(`@${ids.report_done?.position}`);
  const lineage = page.getByTestId("lineage").getByRole("listitem");
  await expect(lineage).toHaveCount(2);
  await expect(lineage.nth(0)).toHaveAttribute("data-depth", "0");
  await expect(lineage.nth(0)).toContainText("superseded");
  await expect(lineage.nth(1)).toHaveAttribute("data-depth", "1");
  await expect(lineage.nth(1)).toContainText("(this)");
  await expect(inspector().locator("table.payload")).toContainText("done");
  // The edge itself can be inspected and returned from.
  await page
    .getByTestId("relationships")
    .locator("tbody tr td:last-child button")
    .click();
  await expect(
    main().getByRole("heading", { name: "DependsOn" }),
  ).toBeVisible();
  await banner().getByRole("button", { name: "← Back" }).click();
  await expect(
    main().getByRole("heading", { name: "Write the report" }),
  ).toBeVisible();
});

test("Explore from here seeds a one-hop query and groups the results by type", async () => {
  await main().getByRole("button", { name: "Explore from here" }).click();
  await expect(main().getByRole("heading", { name: "Explore" })).toBeVisible();
  await expect(page.getByTestId("seeds")).toContainText(
    `@${ids.report_done?.position}`,
  );
  await main().getByRole("button", { name: "Run" }).click();
  const results = page.getByTestId("results");
  await expect(results).toContainText("2 nodes, 1 edge");
  await expect(results.getByRole("heading", { name: "Task" })).toBeVisible();
  // Result rows keep append order: the reached node was appended before
  // the revision that seeded the query.
  const rows = results.locator("table.entries tbody tr");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText("Review the report");
  await expect(rows.nth(0)).toContainText(
    "hop 1 ← DependsOn from Write the report",
  );
  await expect(rows.nth(1)).toContainText("Write the report");
  await expect(rows.nth(1)).toContainText("seed");
});

test("the Thread display nests the dependency under its parent", async () => {
  await page
    .getByTestId("results")
    .getByRole("button", { name: "Thread" })
    .click();
  const rows = page.getByTestId("thread").getByRole("listitem");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toHaveAttribute("data-depth", "0");
  await expect(rows.nth(0)).toContainText("Review the report");
  await expect(rows.nth(1)).toHaveAttribute("data-depth", "1");
  await expect(rows.nth(1)).toContainText("Write the report");
  // Reading the edge the other way makes the revised report the root.
  await page
    .getByTestId("results")
    .getByLabel("direction")
    .selectOption("incoming");
  await expect(rows.nth(0)).toContainText("Write the report");
  await expect(rows.nth(1)).toContainText("Review the report");
});

test("an as-of boundary narrows every read and current resets it", async () => {
  // Up to and including the original report: one Task, no revision yet.
  const boundary = String(BigInt(ids.report?.position ?? "0") + 1n);
  await banner().getByLabel("as of").fill(boundary);
  await banner().getByRole("button", { name: "Set" }).click();
  await expect(page.getByTestId("as-of")).toHaveText(`as of ${boundary}`);
  const task = sidebar().getByRole("listitem").filter({ hasText: "Task" });
  await expect(task).toContainText("1/0");
  await task.getByRole("button", { name: "Task" }).click();
  await expect(table().locator("table.entries tbody tr")).toHaveCount(1);
  await banner().getByRole("button", { name: "current" }).click();
  await expect(page.getByTestId("as-of")).toHaveCount(0);
  await expect(table().locator("table.entries tbody tr")).toHaveCount(3);
});
