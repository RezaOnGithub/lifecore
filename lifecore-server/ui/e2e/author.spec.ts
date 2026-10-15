import { expect, type Page, test } from "@playwright/test";
import type {
  EntryId,
  Method,
  Params,
  ProposalEntry,
  Result,
  RpcResponse,
} from "../src/contract.ts";

// The authoring flow of Milestone 2: create, check, and apply through the
// draft tray, revise with reconnection, recover from a stale base, and
// import. Files run in name order, so this one runs before browse.spec in
// the same database; it declares and uses its own `Job` type and never
// touches `Task`, whose counts browse.spec asserts. Payloads are entered as
// raw JSON, so only the editor's pinned labels are relied on.
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

const entries: ProposalEntry[] = [
  {
    name: "job_type",
    kind: "node",
    ty_name: "Type",
    payload: {
      name: "Job",
      fields: {
        kind: "record",
        value: {
          title: ["mandatory", "text"],
          priority: ["optional", "integer"],
          due: ["optional", "timestamp"],
        },
      },
      outgoing: ["DependsOn"],
      incoming: ["DependsOn"],
      successors: [],
    },
  },
  {
    name: "agenda",
    kind: "node",
    ty_name: "Job",
    payload: { title: "Draft agenda" },
  },
  {
    name: "room",
    kind: "node",
    ty_name: "Job",
    payload: { title: "Book room" },
  },
  {
    name: "invites",
    kind: "node",
    ty_name: "Job",
    payload: { title: "Send invites" },
  },
  {
    name: "invites_after_agenda",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "invites" },
    to: { added: "agenda" },
  },
  {
    name: "agenda_after_room",
    kind: "edge",
    ty_name: "DependsOn",
    from: { added: "agenda" },
    to: { added: "room" },
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
const tray = () => page.getByTestId("tray");
const table = () => page.getByTestId("table");
const status = () => page.getByTestId("draft-status");
const trayButton = (name: string) =>
  tray().getByRole("button", { name, exact: true });
const operation = (name: string) =>
  tray()
    .getByTestId("operation")
    .filter({ has: page.getByRole("button", { name, exact: true }) });

async function openJobs() {
  await page
    .getByTestId("types")
    .getByRole("button", { name: "Job", exact: true })
    .click();
  await expect(main().getByRole("heading", { name: "Job" })).toBeVisible();
}

// The raw editor applies its text on blur.
async function setPayload(json: unknown) {
  await main().getByRole("button", { name: "Raw JSON" }).click();
  const raw = main().getByLabel("payload JSON");
  await raw.fill(JSON.stringify(json, null, 2));
  await raw.blur();
  await expect(main().getByRole("button", { name: "Form" })).toBeVisible();
}

async function checkAccepted() {
  await trayButton("Check").click();
  await expect(status()).toHaveText("accepted");
  await expect(trayButton("Apply")).toBeEnabled();
}

test("a new Job from its table is checked, applied, and listed", async () => {
  await openJobs();
  await expect(table().locator("table.entries tbody tr")).toHaveCount(3);
  await main().getByRole("button", { name: "New node of this type" }).click();
  await expect(main().getByLabel("name", { exact: true })).toHaveValue("job_1");
  await expect(main().getByLabel("type", { exact: true })).toHaveValue("Job");
  await expect(page.getByTestId("plan-id")).toContainText("lc1:");
  await expect(trayButton("Apply")).toBeDisabled();
  await setPayload({
    title: "Water the plants",
    priority: { kind: "integer", value: "3" },
    due: { kind: "timestamp", value: "2026-11-01T09:00:00.000000001Z" },
  });
  await checkAccepted();
  await expect(operation("job_1").getByTestId("check-status")).toHaveText("ok");
  await trayButton("Apply").click();
  const applied = page.getByTestId("applied");
  await expect(applied).toContainText("job_1 → @");
  await expect(page.getByTestId("draft-count")).toHaveText("0 operations");
  // Apply reloaded every screen: the table lists the new row.
  await openJobs();
  await expect(table().locator("table.entries tbody tr")).toHaveCount(4);
  await expect(
    table().getByRole("button", { name: "Water the plants" }),
  ).toBeVisible();
  await applied.getByRole("button", { name: "Dismiss" }).click();
  await expect(applied).toHaveCount(0);
});

test("revising a Job keeps the chosen edge and leaves the old ones historical", async () => {
  await table().getByRole("button", { name: "Draft agenda" }).click();
  await expect(
    main().getByRole("heading", { name: "Draft agenda" }),
  ).toBeVisible();
  await main().getByRole("button", { name: "Revise", exact: true }).click();
  const reconnect = page.getByTestId("reconnect");
  const items = reconnect.getByRole("listitem");
  await expect(items).toHaveCount(2);
  // Revise keeps every current edge by default.
  await expect(reconnect.getByRole("checkbox", { checked: true })).toHaveCount(
    2,
  );
  await expect(page.getByTestId("draft-count")).toHaveText("3 operations");
  await items.filter({ hasText: "Book room" }).getByRole("checkbox").uncheck();
  await expect(page.getByTestId("draft-count")).toHaveText("2 operations");
  const kept = `job_1_keep_${ids.invites_after_agenda?.position}`;
  await expect(operation(kept)).toContainText("DependsOn");
  await setPayload({ title: "Draft the agenda" });
  await checkAccepted();
  await trayButton("Apply").click();
  const applied = page.getByTestId("applied");
  await expect(applied).toContainText(`${kept} → @`);
  // The first assigned id is the revision.
  await applied.getByRole("listitem").first().getByRole("button").click();
  await expect(
    main().getByRole("heading", { name: "Draft the agenda" }),
  ).toBeVisible();
  const lineage = page.getByTestId("lineage").getByRole("listitem");
  await expect(lineage).toHaveCount(2);
  await expect(lineage.nth(0)).toContainText("superseded");
  await expect(lineage.nth(1)).toContainText("(this)");
  const relationships = page.getByTestId("relationships").locator("tbody tr");
  await expect(relationships).toHaveCount(1);
  await expect(relationships.nth(0)).toContainText("Send invites");
  await expect(relationships.nth(0)).toContainText("current");
  // The superseded revision keeps both of its edges, now historical.
  await lineage.nth(0).getByRole("button", { name: "Draft agenda" }).click();
  await expect(
    main().getByRole("heading", { name: "Draft agenda" }),
  ).toBeVisible();
  await expect(relationships).toHaveCount(2);
  for (const row of await relationships.all()) {
    await expect(row).toContainText("historical");
  }
  await applied.getByRole("button", { name: "Dismiss" }).click();
});

test("a stale base keeps the draft until Replan", async () => {
  await openJobs();
  await main().getByRole("button", { name: "New node of this type" }).click();
  await expect(page.getByTestId("plan-id")).toContainText("lc1:");
  await setPayload({ title: "Late job" });
  // Another writer moves the graph past the draft's plan ID.
  await call("proposal.apply", {
    proposal: {
      entries: [
        {
          name: "sneaked",
          kind: "node",
          ty_name: "Job",
          payload: { title: "Sneaked in" },
        },
      ],
    },
  });
  const plan = await page.getByTestId("plan-id").textContent();
  await trayButton("Check").click();
  const stale = page.getByTestId("stale");
  await expect(stale).toBeVisible();
  await expect(status()).toHaveText("stale base");
  await expect(page.getByTestId("draft-count")).toHaveText("1 operation");
  await expect(trayButton("Apply")).toBeDisabled();
  await stale.getByRole("button", { name: "Replan" }).click();
  await expect(stale).toHaveCount(0);
  await expect(page.getByTestId("plan-id")).not.toHaveText(plan ?? "");
  await checkAccepted();
  await trayButton("Clear").click();
  await expect(page.getByTestId("draft-count")).toHaveText("0 operations");
});

test("an imported bare integer is a decimal string and an undeclared type blocks Apply", async () => {
  await trayButton("Import").click();
  const importer = page.getByTestId("import");
  await importer.getByLabel("format").selectOption("toml");
  await importer
    .getByLabel("proposal text")
    .fill(
      [
        "[[entries]]",
        'name = "imported"',
        'kind = "node"',
        'ty_name = "Job"',
        "",
        "[entries.payload]",
        'title = "Imported job"',
        "priority = 9223372036854775807",
        "",
      ].join("\n"),
    );
  await importer.getByRole("button", { name: "Decode" }).click();
  await importer.getByRole("button", { name: "Replace draft" }).click();
  await expect(main().getByLabel("name", { exact: true })).toHaveValue(
    "imported",
  );
  await expect(main().getByLabel("priority", { exact: true })).toHaveValue(
    "9223372036854775807",
  );
  await main().getByRole("button", { name: "Raw JSON" }).click();
  await expect(main().getByLabel("payload JSON")).toHaveValue(
    /"priority": \{\s*"kind": "integer",\s*"value": "9223372036854775807"\s*\}/,
  );
  // A copy of another type: one the view has not declared.
  await main().getByRole("button", { name: "Duplicate" }).click();
  await trayButton("imported_2").click();
  await expect(main().getByLabel("name", { exact: true })).toHaveValue(
    "imported_2",
  );
  await main()
    .getByLabel("type", { exact: true })
    .selectOption({ label: "other…" });
  await main().getByLabel("type name").fill("Gizmo");
  await trayButton("Check").click();
  await expect(status()).toHaveText("rejected");
  const gizmo = operation("imported_2");
  await expect(gizmo.getByTestId("check-status")).toHaveText("1 inconsistency");
  await expect(gizmo.getByTestId("findings")).toContainText(
    'Type "Gizmo" is not declared',
  );
  await expect(operation("imported").getByTestId("check-status")).toHaveText(
    "ok",
  );
  await expect(trayButton("Apply")).toBeDisabled();
  await trayButton("Clear").click();
  await expect(page.getByTestId("draft-count")).toHaveText("0 operations");
});
