import { expect, type Page, type Request, test } from "@playwright/test";
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
  // beforeAll may have failed before opening the page.
  await page?.close();
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
  await task.getByRole("button", { name: "Task", exact: true }).click();
  await expect(main().getByRole("heading", { name: "Task" })).toBeVisible();
  const rows = table().locator("table.entries tbody tr");
  await expect(rows).toHaveCount(3);
  // Declared columns follow status order, alphabetical within a status,
  // after the relationships; `title` labels every row, so it has no column
  // of its own, and only current rows need no currentness column.
  const headers = table().locator("table.entries thead th");
  await expect(headers).toContainText([
    "label",
    "@position",
    "→ DependsOn",
    "← DependsOn",
    "notes",
    "details",
    "done",
    "due",
    "priority",
    "status",
    "tags",
  ]);
  await expect(headers).not.toContainText(["current"]);
  await expect(headers).not.toContainText(["title"]);
  // By the label cell: a neighbour's relationship cell names the row too.
  const done = rows.filter({
    has: page.locator("td.label-cell", { hasText: "Write the report" }),
  });
  await expect(done).toContainText(`@${ids.report_done?.position}`);
  const columns = await headers.allTextContents();
  await expect(done.locator("td").nth(columns.indexOf("status"))).toHaveText(
    "done",
  );
  await expect(done.locator("td").nth(columns.indexOf("priority"))).toHaveText(
    "2",
  );
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
  // The label cell: the neighbours' relationship cells name it too.
  await table()
    .locator("td.label-cell")
    .getByRole("button", { name: "Review the report" })
    .click();
  await expect(
    main().getByRole("heading", { name: "Review the report" }),
  ).toBeVisible();
  await expect(inspector().locator("table.payload")).toContainText("Ask Teto");
  await expect(inspector().locator("table.payload")).toContainText("absent");
  await expect(page.getByTestId("lineage").getByRole("listitem")).toHaveCount(
    1,
  );
  // One group per edge type and direction, outgoing first.
  const groups = page
    .getByTestId("relationships")
    .getByTestId("relation-group");
  await expect(groups).toHaveCount(2);
  await expect(groups.nth(0)).toHaveAttribute("data-relation", "→ DependsOn");
  await expect(groups.nth(0).getByTestId("neighbour")).toHaveText([
    /Write the report/,
  ]);
  await expect(groups.nth(1)).toHaveAttribute("data-relation", "← DependsOn");
  await expect(groups.nth(1).getByTestId("neighbour")).toHaveText([
    /Publish the report/,
  ]);
  await expect(main().getByRole("button", { name: "Revise" })).toBeEnabled();
});

test("following a relationship shows the revised node's lineage", async () => {
  await page
    .getByTestId("relationships")
    .getByRole("button", { name: "Write the report", exact: true })
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
    .getByRole("button", {
      name: `@${ids.review_after_report?.position}`,
      exact: true,
    })
    .click();
  await expect(
    main().getByRole("heading", { name: "DependsOn" }),
  ).toBeVisible();
  await banner().getByRole("button", { name: "← Back" }).click();
  await expect(
    main().getByRole("heading", { name: "Write the report" }),
  ).toBeVisible();
});

test("Explore from here seeds and runs a one-hop query grouped by type", async () => {
  await main().getByRole("button", { name: "Explore from here" }).click();
  await expect(main().getByRole("heading", { name: "Explore" })).toBeVisible();
  await expect(page.getByTestId("seeds")).toContainText(
    `@${ids.report_done?.position}`,
  );
  // The query ran on arrival; Run is for later changes.
  const results = page.getByTestId("results");
  await expect(results).toContainText("2 nodes, 1 edge");
  await expect(page.getByTestId("form-changed")).toHaveCount(0);
  await expect(results.getByRole("heading", { name: "Task" })).toBeVisible();
  // Result rows keep append order: the reached node was appended before
  // the revision that seeded the query. The edge it was reached over is
  // drawn in its own direction, here into the seed.
  const rows = results.locator("table.entries tbody tr");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0).locator("td.label-cell")).toHaveText(
    "Review the report",
  );
  await expect(rows.nth(0).locator("td.via-cell")).toHaveText(
    "1 via → DependsOn from Write the report",
  );
  await expect(rows.nth(1).locator("td.label-cell")).toHaveText(
    "Write the report",
  );
  await expect(rows.nth(1).locator("td.via-cell")).toHaveText("seed");
});

test("the Thread display nests the dependency under its parent", async () => {
  await page
    .getByTestId("results")
    .getByRole("button", { name: "Thread", exact: true })
    .click();
  const rows = page.getByTestId("thread").getByRole("listitem");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toHaveAttribute("data-depth", "0");
  await expect(rows.nth(0)).toContainText("Review the report");
  await expect(rows.nth(1)).toHaveAttribute("data-depth", "1");
  await expect(rows.nth(1)).toContainText("Write the report");
  // Reading the edge the other way makes the revised report the root.
  await page.getByTestId("thread-direction").selectOption("incoming");
  await expect(rows.nth(0)).toContainText("Write the report");
  await expect(rows.nth(1)).toContainText("Review the report");
});

test("a changed form says so until Run sends the new query", async () => {
  await main().getByLabel("hops").selectOption("2");
  await expect(page.getByTestId("form-changed")).toBeVisible();
  await main().getByRole("button", { name: "Run", exact: true }).click();
  await expect(page.getByTestId("form-changed")).toHaveCount(0);
  await expect(page.getByTestId("results")).toContainText("3 nodes, 2 edges");
  // The Thread display survived the rerun; read from → to, the second hop's
  // publishing task is the root of the chain.
  await page.getByTestId("thread-direction").selectOption("outgoing");
  const rows = page.getByTestId("thread").getByRole("listitem");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toHaveAttribute("data-depth", "0");
  await expect(rows.nth(0)).toContainText("Publish the report");
  await expect(rows.nth(2)).toHaveAttribute("data-depth", "2");
  await expect(rows.nth(2)).toContainText("Write the report");
});

test("an as-of boundary narrows every read and current resets it", async () => {
  // A boundary past the end is refused before it is sent.
  const total = (await page.getByTestId("entries").textContent()) ?? "0";
  await banner()
    .getByLabel("as of")
    .fill(String(BigInt(total) + 1n));
  await expect(banner().getByRole("button", { name: "Set" })).toBeDisabled();
  await expect(banner()).toContainText(`at most ${total}`);
  // The original report, through the lineage of its revision, the seed.
  await page
    .getByTestId("seeds")
    .getByRole("button", { name: /^Write the report/ })
    .click();
  await page
    .getByTestId("lineage")
    .getByRole("button", { name: "Write the report" })
    .click();
  await expect(
    main().getByRole("heading", { name: "Write the report" }),
  ).toContainText(`@${ids.report?.position}`);
  await expect(page.getByTestId("lineage").getByRole("listitem")).toHaveCount(
    2,
  );
  // "As of here" makes it the last entry in view: one Task, no revision
  // yet. Until the bounded answer arrives, nothing of the wider read shows,
  // and the hidden Explore asks nothing.
  let release = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/rpc", async (route) => {
    const body = route.request().postDataJSON() as {
      method?: string;
      params?: { as_of?: string };
    } | null;
    if (body?.method === "entries.inspect" && body.params?.as_of) {
      await held;
    }
    await route.fallback();
  });
  const queries: string[] = [];
  const onRequest = (request: Request) => {
    const body = request.postDataJSON() as { method?: string } | null;
    if (body?.method === "views.query") {
      queries.push(request.postData() ?? "");
    }
  };
  page.on("request", onRequest);
  const boundary = String(BigInt(ids.report?.position ?? "0") + 1n);
  await main().getByRole("button", { name: "As of here" }).click();
  await expect(page.getByTestId("as-of")).toContainText(
    `showing the first ${boundary} of ${total} entries, view [0, ${boundary})`,
  );
  await expect(banner().getByLabel("as of")).toHaveValue(boundary);
  await expect(main().getByText("loading…", { exact: true })).toBeVisible();
  await expect(page.getByTestId("lineage")).toHaveCount(0);
  release();
  await expect(page.getByTestId("lineage").getByRole("listitem")).toHaveCount(
    1,
  );
  await page.unroute("**/rpc");
  await expect(
    main().getByRole("button", { name: "As of here" }),
  ).toBeDisabled();
  // Drafts apply to the current state, which a bounded read does not show;
  // a branch copies this revision's payload, the same at every boundary.
  await expect(main().getByRole("button", { name: "Revise" })).toBeDisabled();
  await expect(main().getByRole("button", { name: "Branch" })).toBeEnabled();
  const task = sidebar().getByRole("listitem").filter({ hasText: "Task" });
  await expect(task).toContainText("1/0");
  page.off("request", onRequest);
  expect(queries).toEqual([]);
  // Past the boundary, the trail names the revision, and the other tasks
  // it visited, by position alone, though their labels were read before.
  const trail = page.getByTestId("trail");
  await expect(
    trail
      .getByRole("button", {
        name: `@${ids.report_done?.position}`,
        exact: true,
      })
      .first(),
  ).toBeVisible();
  await expect(trail.getByRole("button", { name: /report/ })).toHaveCount(0);
  // Explore asks again on return; its seed is past the boundary, and the
  // failure says so in words, the seed chip naming it by position alone.
  await page
    .getByRole("navigation")
    .getByRole("button", { name: "Explore", exact: true })
    .click();
  await expect(main().getByRole("alert")).toContainText(
    `@${ids.report_done?.position} is not in this view`,
  );
  await expect(page.getByTestId("seeds")).toContainText(
    `@${ids.report_done?.position}`,
  );
  await expect(page.getByTestId("seeds")).not.toContainText("Write the report");
  await task.getByRole("button", { name: "Task", exact: true }).click();
  await expect(table().locator("table.entries tbody tr")).toHaveCount(1);
  await expect(table()).toContainText(`view [0, ${boundary})`);
  await banner().getByRole("button", { name: "current" }).click();
  await expect(page.getByTestId("as-of")).toHaveCount(0);
  await expect(table().locator("table.entries tbody tr")).toHaveCount(3);
});
