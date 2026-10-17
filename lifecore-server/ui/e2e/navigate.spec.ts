import { spawnSync } from "node:child_process";
import { expect, type Page, test } from "@playwright/test";
import type {
  EntryId,
  Method,
  Params,
  ProposalEntry,
  Result,
  RpcResponse,
} from "../src/contract.ts";

// Navigating a portion of the graph: relationship columns in a type table,
// the inspector's relationship outline, Explore's thread across edge types,
// focus and seed actions, pinned, recent, and saved places in the sidebar,
// the rendered graph, and Back returning to where the user was. The file
// declares its own `Area`, `Chore`, `Member`, and `Stage` types, ends by
// declaring the unnamed type, and never touches `Task` or `Job`; the steps
// share one page, so in-memory navigation and Explore state carry over.
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

function declaration(
  name: string,
  label: string,
  outgoing: string[],
  incoming: string[],
): ProposalEntry {
  return {
    name: `${name.toLowerCase()}_type`,
    kind: "node",
    ty_name: "Type",
    payload: {
      name,
      fields: { kind: "record", value: { [label]: ["mandatory", "text"] } },
      outgoing,
      incoming,
      successors: [],
    },
  };
}

function node(name: string, ty_name: string, payload: object): ProposalEntry {
  return { name, kind: "node", ty_name, payload } as ProposalEntry;
}

function edge(
  name: string,
  ty_name: string,
  from: string,
  to: string,
): ProposalEntry {
  return {
    name,
    kind: "edge",
    ty_name,
    from: { added: from },
    to: { added: to },
  };
}

// Sixty stages of a workshop: a table that scrolls, and a hub.
const stages = Array.from(
  { length: 60 },
  (_, index) => `Stage ${String(index + 1).padStart(2, "0")}`,
);

// Four chores in the kitchen, so its list of chores runs past three; the
// mowing is revised without its `Within` edge, which becomes historical.
const entries: ProposalEntry[] = [
  declaration("Area", "title", [], ["Within"]),
  declaration("Member", "name", [], ["DoneBy"]),
  declaration("Chore", "title", ["Within", "Blocks", "DoneBy"], ["Blocks"]),
  declaration("Stage", "title", ["Within"], []),
  node("workshop", "Area", { title: "Workshop" }),
  ...stages.flatMap((title, index) => [
    node(`stage_${index}`, "Stage", { title }),
    edge(`stage_${index}_in`, "Within", `stage_${index}`, "workshop"),
  ]),
  node("kitchen", "Area", { title: "Kitchen" }),
  node("garden", "Area", { title: "Garden" }),
  node("rin", "Member", { name: "Rin" }),
  node("wash", "Chore", { title: "Wash dishes" }),
  node("dry", "Chore", { title: "Dry dishes" }),
  node("wipe", "Chore", { title: "Wipe table" }),
  node("sweep", "Chore", { title: "Sweep floor" }),
  node("mow", "Chore", { title: "Mow lawn" }),
  node("rake", "Chore", { title: "Rake leaves" }),
  edge("wash_in", "Within", "wash", "kitchen"),
  edge("dry_in", "Within", "dry", "kitchen"),
  edge("wipe_in", "Within", "wipe", "kitchen"),
  edge("sweep_in", "Within", "sweep", "kitchen"),
  edge("mow_in", "Within", "mow", "garden"),
  edge("rake_in", "Within", "rake", "garden"),
  edge("wash_blocks_dry", "Blocks", "wash", "dry"),
  edge("wash_by", "DoneBy", "wash", "rin"),
  edge("rake_by", "DoneBy", "rake", "rin"),
  {
    name: "mow_again",
    kind: "node",
    ty_name: "Chore",
    predecessor: { added: "mow" },
    intent: "revise",
    payload: { title: "Mow lawn twice" },
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
  await page?.close();
});

const main = () => page.getByRole("main");
const sidebar = () => page.getByRole("navigation");
const table = () => page.getByTestId("table");
const results = () => page.getByTestId("results");
const openTable = (name: string) =>
  page.getByTestId("types").getByRole("button", { name, exact: true }).click();
type Scope = () => ReturnType<Page["getByTestId"]>;
// A row by its label cell: neighbour links in other cells carry labels too.
const row = (scope: Scope, label: string) =>
  scope()
    .locator("table.entries tbody tr")
    .filter({
      has: page
        .locator("td.label-cell")
        .getByRole("button", { name: label, exact: true }),
    });
const label = (scope: Scope, text: string) =>
  scope()
    .locator("td.label-cell")
    .getByRole("button", { name: text, exact: true });
// The cell of a row under the column headed `header`, once the table shows
// that column.
async function cell(scope: Scope, text: string, header: string) {
  const headers = async () =>
    (await scope().locator("table.entries thead th").allTextContents()).map(
      (text) => text.replace(/ [▲▼]$/, "").trim(),
    );
  await expect.poll(headers, { message: `column ${header}` }).toContain(header);
  const index = (await headers()).indexOf(header);
  return row(scope, text).locator("td").nth(index);
}

test("a type table lists each row's neighbours by edge type and direction", async () => {
  await openTable("Chore");
  await expect(table().locator("table.entries tbody tr")).toHaveCount(6);
  // The relationship columns come before the payload: outgoing first, then
  // by name. Current rows need no currentness column, and `title`, which
  // labels every row, none of its own.
  const headers = table().locator("table.entries thead th");
  await expect(headers).toHaveText([
    "label",
    "@position",
    "→ Blocks",
    "→ DoneBy",
    "→ Within",
    "← Blocks",
  ]);
  await expect(await cell(table, "Wash dishes", "→ Within")).toHaveText(
    "Kitchen",
  );
  await expect(await cell(table, "Wash dishes", "→ DoneBy")).toHaveText("Rin");
  await expect(await cell(table, "Dry dishes", "← Blocks")).toHaveText(
    "Wash dishes",
  );
  // The revision left its `Within` edge behind: current rows meet current
  // edges only.
  await expect(await cell(table, "Mow lawn twice", "→ Within")).toHaveText("");
  // Sorting by a relationship column sorts by its first label; empty cells
  // go last.
  await table().getByRole("button", { name: "→ Within" }).click();
  const rows = table().locator("table.entries tbody tr");
  await expect(rows.first()).toContainText("Rake leaves");
  await expect(rows.last()).toContainText("Mow lawn twice");

  await openTable("Area");
  const kitchen = await cell(table, "Kitchen", "← Within");
  await expect(kitchen.getByRole("button")).toHaveText([
    "Wash dishes",
    "Dry dishes",
    "Wipe table",
    "+1 more",
  ]);
  await kitchen.getByRole("button", { name: "+1 more" }).click();
  await expect(kitchen.getByRole("button")).toHaveText([
    "Wash dishes",
    "Dry dishes",
    "Wipe table",
    "Sweep floor",
    "less",
  ]);
  // Every row, superseded ones too, shows its historical edges as well.
  await table().getByLabel("show").selectOption("all");
  await expect(await cell(table, "Garden", "← Within")).toHaveText(
    "Mow lawn, Rake leaves",
  );
  await expect(headers).toContainText(["label", "@position", "current"]);
  await table().getByLabel("show").selectOption("current");
  // Without the columns, the table reads its rows alone.
  const rowsAlone = page.waitForRequest((request) => {
    const body = request.postDataJSON() as {
      method?: string;
      params?: { expand?: unknown[] };
    } | null;
    return body?.method === "views.query" && body.params?.expand?.length === 0;
  });
  await table().getByLabel("relationships").uncheck();
  await rowsAlone;
  await expect(headers).toHaveText(["label", "@position"]);
  await table().getByLabel("relationships").check();
  await expect(headers.last()).toHaveText("← Within");
});

test("the inspector's outline expands neighbours two levels deep", async () => {
  await openTable("Chore");
  await label(table, "Wash dishes").click();
  await expect(
    main().getByRole("heading", { name: "Wash dishes" }),
  ).toBeVisible();
  const outline = page.getByTestId("relationships");
  const groups = outline.locator(":scope > li");
  await expect(groups).toHaveCount(3);
  await expect(groups.locator(":scope > .rel-head")).toHaveText([
    /→ Blocks\s*1/,
    /→ DoneBy\s*1/,
    /→ Within\s*1/,
  ]);
  // One level: the kitchen's chores, the inspected one marked on the path
  // and not expandable.
  await outline.getByRole("button", { name: "expand Kitchen" }).click();
  const kitchen = outline.getByTestId("neighbour").filter({
    has: page.getByRole("button", { name: "Kitchen", exact: true }),
  });
  const chores = kitchen.getByTestId("relation-group");
  await expect(chores).toHaveCount(1);
  await expect(chores).toHaveAttribute("data-relation", "← Within");
  await expect(chores.getByTestId("relation-count")).toHaveText("4");
  const back = chores.getByTestId("neighbour").filter({
    has: page.getByRole("button", { name: "Wash dishes", exact: true }),
  });
  await expect(back).toContainText("(on this path)");
  await expect(
    back.getByRole("button", { name: "expand Wash dishes" }),
  ).toHaveCount(0);
  // Two levels: a chore of the kitchen, whose blocker is the inspected
  // node again.
  await chores.getByRole("button", { name: "expand Dry dishes" }).click();
  const dry = chores.getByTestId("neighbour").filter({
    has: page.getByRole("button", { name: "Dry dishes", exact: true }),
  });
  await expect(dry.getByTestId("relation-group")).toHaveCount(2);
  const blocker = dry
    .locator('[data-relation="← Blocks"]')
    .getByTestId("neighbour");
  await expect(blocker).toContainText("Wash dishes");
  await expect(blocker).toContainText("(on this path)");
  await expect(dry.locator('[data-relation="→ Within"]')).toContainText(
    "(on this path)",
  );
  // A neighbour's label opens its inspector.
  await outline.getByRole("button", { name: "Rin", exact: true }).click();
  await expect(main().getByRole("heading", { name: "Rin" })).toBeVisible();
  // A historical edge waits behind its group's toggle.
  await openTable("Area");
  await label(table, "Garden").click();
  const within = outline.locator('[data-relation="← Within"]');
  await expect(within.getByTestId("neighbour")).toHaveText([/Rake leaves/]);
  await within.getByRole("button", { name: "show historical (1)" }).click();
  await expect(within.getByTestId("neighbour")).toHaveCount(2);
  await expect(within.getByTestId("neighbour").nth(0)).toContainText(
    "Mow lawn",
  );
  await expect(within.getByTestId("neighbour").nth(0)).toContainText(
    "historical",
  );
});

test("the thread nests across every edge type with the type on each row", async () => {
  await openTable("Area");
  await label(table, "Kitchen").click();
  await main().getByRole("button", { name: "Explore from here" }).click();
  await expect(results()).toContainText("5 nodes, 5 edges");
  await results().getByRole("button", { name: "Thread", exact: true }).click();
  // Several edge types: all of them, read in the direction that nests the
  // most, which puts the kitchen at the root.
  await expect(page.getByTestId("thread-relation")).toHaveValue("all");
  await expect(page.getByTestId("thread-direction")).toHaveValue("incoming");
  const rows = page.getByTestId("thread").getByRole("listitem");
  await expect(rows.nth(0)).toHaveAttribute("data-depth", "0");
  await expect(rows.nth(0)).toContainText("Kitchen");
  await expect(rows.nth(0).getByTestId("via-type")).toHaveCount(0);
  const wash = rows.filter({ hasText: "Wash dishes" });
  // The prefix reads from the row's own node: washing is within the kitchen.
  await expect(wash.nth(0)).toHaveAttribute("data-depth", "1");
  await expect(wash.nth(0).getByTestId("via-type")).toHaveText("→ Within");
  // Washing blocks drying, so it also hangs under it.
  await expect(wash.nth(1)).toHaveAttribute("data-depth", "2");
  await expect(wash.nth(1).getByTestId("via-type")).toHaveText("→ Blocks");
  // One type at a time has no prefixes.
  await page.getByTestId("thread-relation").selectOption("Within");
  await expect(page.getByTestId("via-type")).toHaveCount(0);
  await page.getByTestId("thread-relation").selectOption("all edge types");
});

test("focus and + seed rerun the query from a result row", async () => {
  await results().getByRole("button", { name: "Table", exact: true }).click();
  // The via column reads the edge from the row's node: washing is within
  // the kitchen it was reached from.
  const washRow = row(results, "Wash dishes");
  await expect(washRow.locator("td.via-cell")).toHaveText(
    "1 via → Within from Kitchen",
  );
  await washRow.getByRole("button", { name: "+ seed" }).click();
  await expect(page.getByTestId("seeds").getByRole("listitem")).toHaveCount(2);
  await expect(results()).toContainText("6 nodes");
  const rinRow = row(results, "Rin");
  await expect(rinRow.locator("td.via-cell")).toHaveText(
    "1 via ← DoneBy from Wash dishes",
  );
  await rinRow.getByRole("button", { name: "focus" }).click();
  await expect(page.getByTestId("seeds").getByRole("listitem")).toHaveCount(1);
  await expect(page.getByTestId("seeds")).toContainText(
    `@${ids.rin?.position}`,
  );
  await expect(results()).toContainText("3 nodes, 2 edges");
  await expect(page.getByTestId("form-changed")).toHaveCount(0);
});

test("pinned, recent, and saved places return to where the user was", async () => {
  await main().getByRole("button", { name: "Save view" }).click();
  await main().getByLabel("view name").fill("Rin's chores");
  await main().getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByTestId("saved-views")).toContainText("Rin's chores");

  await label(results, "Rake leaves").click();
  await expect(
    main().getByRole("heading", { name: "Rake leaves" }),
  ).toBeVisible();
  await main().getByRole("button", { name: "pin", exact: true }).click();
  await expect(main().getByRole("button", { name: "unpin" })).toHaveText("★");
  await expect(page.getByTestId("pinned")).toContainText("Rake leaves");
  // Recent is most recent first, without repeats.
  const recent = page.getByTestId("recent").getByRole("listitem");
  await expect(recent.nth(0)).toContainText("Rake leaves");
  await expect(recent.nth(1)).toContainText("Kitchen");

  // Elsewhere, the saved view restores its form and runs it.
  await openTable("Area");
  await page
    .getByTestId("saved-views")
    .getByRole("button", { name: "Rin's chores", exact: true })
    .click();
  await expect(main().getByRole("heading", { name: "Explore" })).toBeVisible();
  await expect(page.getByTestId("seeds").getByRole("listitem")).toHaveCount(1);
  await expect(results()).toContainText("3 nodes, 2 edges");
  await page
    .getByTestId("pinned")
    .getByRole("button", { name: "Rake leaves", exact: true })
    .click();
  await expect(
    main().getByRole("heading", { name: "Rake leaves" }),
  ).toBeVisible();

  // The places are kept per database across a reload; the in-memory
  // Explore state is not.
  await page.reload();
  await expect(page.getByTestId("pinned")).toContainText("Rake leaves");
  await expect(page.getByTestId("saved-views")).toContainText("Rin's chores");
  await expect(recent.nth(0)).toContainText("Rake leaves");
  await page
    .getByTestId("pinned")
    .getByRole("button", { name: "unpin Rake leaves" })
    .click();
  await expect(page.getByTestId("pinned")).toHaveCount(0);
  await expect(sidebar()).toContainText(
    "☆ in the inspector pins an entry here",
  );
});

test("the graph renders on request and a node opens its inspector", async () => {
  const dot = spawnSync("dot", ["-V"]);
  test.skip(
    dot.error !== undefined || dot.status !== 0,
    "Graphviz's dot is not installed; views.render cannot draw SVG",
  );
  await page
    .getByTestId("saved-views")
    .getByRole("button", { name: "Rin's chores", exact: true })
    .click();
  await expect(results()).toContainText("3 nodes, 2 edges");
  await results().getByRole("button", { name: "Graph", exact: true }).click();
  // Nothing is drawn until Render.
  await expect(page.getByTestId("graph-svg")).toHaveCount(0);
  await expect(page.getByTestId("graph-size")).toHaveText("3 nodes, 2 edges");
  // Drawn while another display is on show: the drawing takes its size
  // from its own markup, and fits once it is shown.
  await results().getByRole("button", { name: "Render" }).click();
  await results().getByRole("button", { name: "Table", exact: true }).click();
  await expect(page.getByTestId("graph-svg").locator("svg")).toHaveCount(1);
  await results().getByRole("button", { name: "Graph", exact: true }).click();
  const svg = page.getByTestId("graph-svg").locator("svg");
  await expect(svg).toBeVisible();
  const sizer = await page.locator(".graph-sizer").boundingBox();
  expect(sizer?.height ?? 0).toBeGreaterThan(40);
  expect(sizer?.width ?? 0).toBeGreaterThan(40);
  await expect(page.getByTestId("zoom")).toHaveText("100%");
  await expect(page.getByTestId("graph-view")).toContainText("drawn at view");
  // The seed is outlined.
  await expect(svg.locator("g.node.seed")).toHaveCount(1);
  await expect(svg.locator("g.node.seed")).toContainText("Rin");
  await results().getByLabel("DOT").check();
  await expect(page.getByTestId("dot")).toContainText("digraph lifecore");
  // A click inside a node's shape, away from its text, opens it.
  const shape = await svg
    .locator("g.node")
    .filter({ hasText: "Rake leaves" })
    .locator("ellipse")
    .boundingBox();
  expect(shape).not.toBeNull();
  if (shape !== null) {
    await page.mouse.click(
      shape.x + shape.width * 0.1,
      shape.y + shape.height / 2,
    );
  }
  await expect(
    main().getByRole("heading", { name: "Rake leaves" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "← Back" }).click();
  // The drawing survived the detour.
  await expect(svg).toBeVisible();
});

test("Back returns to a table's sort and scroll and to an expanded outline", async () => {
  await openTable("Stage");
  await expect(table().locator("table.entries tbody tr")).toHaveCount(60);
  // The arrow is drawn, not named: the sorted header says how through
  // `aria-sort`.
  const sortByLabel = table()
    .locator("thead")
    .getByRole("button", { name: "label", exact: true });
  const labelHeader = table()
    .locator("thead th")
    .filter({ has: page.getByRole("button", { name: "label", exact: true }) });
  await expect(labelHeader).not.toHaveAttribute("aria-sort");
  await sortByLabel.click();
  await expect(labelHeader).toHaveAttribute("aria-sort", "ascending");
  await sortByLabel.click();
  await expect(sortByLabel).toHaveText("label ▼");
  await expect(labelHeader).toHaveAttribute("aria-sort", "descending");
  await expect(table().locator("tbody tr").first()).toContainText("Stage 60");
  const pane = main();
  await pane.evaluate((element) => {
    element.scrollTop = 400;
  });
  const offset = await pane.evaluate((element) => element.scrollTop);
  expect(offset).toBeGreaterThan(300);
  // A click that does not scroll the row into view first.
  await label(table, "Stage 30").dispatchEvent("click");
  await expect(main().getByRole("heading", { name: "Stage 30" })).toBeVisible();
  await page.getByRole("button", { name: "← Back" }).click();
  await expect(sortByLabel).toHaveText("label ▼");
  await expect
    .poll(() => pane.evaluate((element) => element.scrollTop))
    .toBe(offset);

  // An outline expanded two levels comes back expanded.
  await openTable("Chore");
  await label(table, "Wash dishes").click();
  const outline = page.getByTestId("relationships");
  await outline.getByRole("button", { name: "expand Kitchen" }).click();
  const kitchen = outline.getByTestId("neighbour").filter({
    has: page.getByRole("button", { name: "Kitchen", exact: true }),
  });
  await kitchen.getByRole("button", { name: "expand Dry dishes" }).click();
  // The kitchen's chores, and under drying its own two groups.
  await expect(kitchen.getByTestId("relation-group")).toHaveCount(3);
  await kitchen
    .getByRole("button", { name: "Sweep floor", exact: true })
    .click();
  await expect(
    main().getByRole("heading", { name: "Sweep floor" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "← Back" }).click();
  await expect(
    main().getByRole("heading", { name: "Wash dishes" }),
  ).toBeVisible();
  await expect(kitchen.getByTestId("relation-group")).toHaveCount(3);
  await expect(
    outline.getByRole("button", { name: "collapse Kitchen" }),
  ).toBeVisible();
  await expect(
    kitchen.getByRole("button", { name: "collapse Dry dishes" }),
  ).toBeVisible();
});

test("each Explore query is a Back step, and the trail jumps back", async () => {
  await main().getByRole("button", { name: "Explore from here" }).click();
  // From here replaces the seeds and resets the form.
  await expect(page.getByTestId("seeds").getByRole("listitem")).toHaveCount(1);
  await expect(page.getByTestId("seeds")).toContainText("Wash dishes");
  await expect(results()).toContainText("4 nodes, 4 edges");
  // The folded form names its seeds, and a new query keeps it folded.
  await main().getByRole("button", { name: "fold the form" }).click();
  await expect(page.getByTestId("query-summary")).toContainText(
    `Wash dishes @${ids.wash?.position}`,
  );
  await row(results, "Kitchen").getByRole("button", { name: "focus" }).click();
  await expect(page.getByTestId("query-summary")).toContainText(
    `Kitchen @${ids.kitchen?.position}`,
  );
  await expect(results()).toContainText("5 nodes, 5 edges");
  // Back is the previous query with its results.
  await page.getByRole("button", { name: "← Back" }).click();
  await expect(page.getByTestId("query-summary")).toContainText(
    `Wash dishes @${ids.wash?.position}`,
  );
  await expect(results()).toContainText("4 nodes, 4 edges");
  // The trail lists the steps before this one; a click goes back there.
  const trail = page.getByTestId("trail");
  await expect(trail).toContainText("Wash dishes");
  await trail.getByRole("button", { name: "Chore table" }).last().click();
  await expect(main().getByRole("heading", { name: "Chore" })).toBeVisible();
});

test("a hub's cell counts its neighbours and opens them in Explore", async () => {
  await openTable("Area");
  const hub = await cell(table, "Workshop", "← Within");
  await expect(hub.getByTestId("cell-count")).toHaveText("60 Stages:");
  await hub.getByRole("button", { name: "+57 in Explore" }).click();
  await expect(page.getByTestId("seeds")).toContainText("Workshop");
  await expect(results()).toContainText("61 nodes, 60 edges");
  // The outline counts by type and filters a large group.
  await label(results, "Workshop").click();
  const outline = page.getByTestId("relationships");
  const within = outline.locator('[data-relation="← Within"]');
  await expect(within.getByTestId("relation-types")).toHaveText("· Stage");
  await within.getByRole("button", { name: /← Within/ }).click();
  await within.getByLabel("filter ← Within").fill("Stage 4");
  await expect(within.getByTestId("neighbour")).toHaveCount(10);
});

test("the unnamed type is an Explore seed of its own, apart from any type", async () => {
  await call("proposal.apply", {
    proposal: {
      entries: [
        { ...declaration("", "title", [], []), name: "unnamed_type" },
        node("nameless", "", { title: "Nameless thing" }),
      ],
    },
  });
  await page
    .getByRole("banner")
    .getByRole("button", { name: "Refresh" })
    .click();
  await sidebar().getByRole("button", { name: "Explore", exact: true }).click();
  const unfold = main().getByRole("button", { name: "unfold the form" });
  if (await unfold.isVisible()) {
    await unfold.click();
  }
  const remove = page
    .getByTestId("seeds")
    .getByRole("button", { name: /^remove @/ });
  while ((await remove.count()) > 0) {
    await remove.first().click();
  }
  await main().getByLabel("nodes of type").check();
  const type = main().getByLabel("seed node type");
  const seeded = (ty_name: string | undefined) =>
    page.waitForRequest((request) => {
      const body = request.postDataJSON() as {
        method?: string;
        params?: { seeds?: { kind: string; ty_name?: string }[] };
      } | null;
      const seeds = body?.params?.seeds ?? [];
      return (
        body?.method === "views.query" &&
        seeds.length === 1 &&
        seeds[0]?.kind === "nodes" &&
        seeds[0].ty_name === ty_name
      );
    });
  await type.selectOption({ label: "(unnamed)" });
  const unnamed = seeded("");
  await main().getByRole("button", { name: "Run", exact: true }).click();
  await unnamed;
  await expect(results()).toContainText("Nameless thing");
  await expect(results()).not.toContainText("Workshop");
  await type.selectOption({ label: "any type" });
  const any = seeded(undefined);
  await main().getByRole("button", { name: "Run", exact: true }).click();
  await any;
  await expect(results()).toContainText("Workshop");
  await expect(results()).toContainText("Nameless thing");
});
