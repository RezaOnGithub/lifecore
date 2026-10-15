import { expect, type Page, test } from "@playwright/test";
import type {
  EntryId,
  FindingKey,
  Method,
  Params,
  ProposalEntry,
  Result,
  RpcResponse,
} from "../src/contract.ts";

// Findings of the stored database: a run on the Findings screen lists a
// warning about a node this file adds, which is dismissed with a note,
// hidden, shown again, and restored; the inspector lists the findings
// about one entry. The file declares its own `Gadget` type and never
// touches `Task` or `Job`; other files' findings share the report, so rows
// are found by their focus. Analyzer wording is not asserted. An
// inconsistency cannot be stored, since every database starts with
// TypeCheck and each append is checked at every prefix, so the refused
// dismissal of one is left to unit and backend tests.
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
    name: "gadget_type",
    kind: "node",
    ty_name: "Type",
    payload: {
      name: "Gadget",
      fields: {
        kind: "record",
        value: {
          title: ["mandatory", "text"],
          owner: ["encouraged", "text"],
        },
      },
      outgoing: [],
      incoming: [],
      successors: [],
    },
  },
  // Without the encouraged owner: a warning focused on it.
  {
    name: "sprocket",
    kind: "node",
    ty_name: "Gadget",
    payload: { title: "Sprocket F1" },
  },
  {
    name: "widget",
    kind: "node",
    ty_name: "Gadget",
    payload: { title: "Widget F1", owner: "Teto" },
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
const sprocket = () =>
  main()
    .getByTestId("finding-group")
    .filter({ has: page.getByRole("button", { name: "Sprocket F1" }) });

// The stored record of the warning about the sprocket, read over RPC.
async function storedRecord() {
  const { analysis, dismissals } = await call("analysis.run", {});
  const sprocketId = ids.sprocket;
  const key = analysis.observations.find(
    (o) =>
      o.diagnostic.kind === "warning" &&
      o.diagnostic.focus?.position === sprocketId?.position,
  )?.key;
  expect(key).toBeDefined();
  const same = (other: FindingKey) =>
    JSON.stringify(other) === JSON.stringify(key);
  return dismissals.find((record) => same(record.key)) ?? null;
}

async function openFindings() {
  await page
    .getByRole("navigation")
    .getByRole("button", { name: "Findings", exact: true })
    .click();
  await expect(
    main().getByRole("heading", { name: "Findings", exact: true }),
  ).toBeVisible();
}

async function run() {
  await main().getByRole("button", { name: "Run", exact: true }).click();
  await expect(main().getByTestId("findings-context")).toBeVisible();
  await expect(
    main().getByRole("button", { name: "Run", exact: true }),
  ).toBeEnabled();
}

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

test("a run lists the warning about a node only from the boundary after it", async () => {
  const position = ids.sprocket?.position ?? "";
  await openFindings();
  await expect(main().getByTestId("finding-group")).toHaveCount(0);
  // Before the sprocket was appended, and right after it.
  await setAsOf(position);
  await run();
  await expect(main().getByTestId("findings-context")).toContainText(
    `[0, ${position})`,
  );
  await expect(sprocket()).toHaveCount(0);
  await setAsOf(String(BigInt(position) + 1n));
  await expect(main().getByTestId("findings-moved")).toBeVisible();
  await run();
  await expect(sprocket()).toHaveCount(1);
  await expect(sprocket()).toContainText(`boundary ${BigInt(position) + 1n}`);
  await setAsOf(null);
  await run();
  await expect(main().getByTestId("findings-moved")).toHaveCount(0);
  await expect(sprocket().locator(".badge.kind-warning")).toHaveCount(1);
  await expect(
    main()
      .getByTestId("finding-group")
      .filter({ has: page.getByRole("button", { name: "Widget F1" }) }),
  ).toHaveCount(0);
});

test("a dismissed warning hides, shows on request with its note, and restores", async () => {
  await sprocket().getByRole("button", { name: "Dismiss…" }).click();
  await sprocket().getByLabel("reason").selectOption("false_positive");
  await sprocket().getByLabel("note").fill("the owner is decided later");
  await sprocket()
    .getByRole("button", { name: "Dismiss", exact: true })
    .click();
  await expect(sprocket()).toHaveCount(0);
  expect(await storedRecord()).toMatchObject({
    reason: "false_positive",
    note: "the owner is decided later",
  });
  await expect(main().getByTestId("findings-filters")).toContainText(
    "dismissed)",
  );
  await main().getByLabel("show dismissed").check();
  await expect(sprocket().getByTestId("dismissed")).toHaveText(
    "dismissed: false positive",
  );
  await expect(sprocket()).toContainText("the owner is decided later");
  await sprocket().getByRole("button", { name: "Restore" }).click();
  await expect(sprocket().getByTestId("dismissed")).toHaveCount(0);
  await expect(
    sprocket().getByRole("button", { name: "Dismiss…" }),
  ).toBeVisible();
  expect(await storedRecord()).toBeNull();
  await main().getByLabel("show dismissed").uncheck();
  await expect(sprocket()).toHaveCount(1);
});

async function inspectGadget(label: string) {
  await page
    .getByTestId("types")
    .getByRole("button", { name: "Gadget", exact: true })
    .click();
  await main().getByRole("button", { name: label }).first().click();
  await expect(main().getByRole("heading", { name: label })).toBeVisible();
}

test("the focus opens the inspector, whose section lists the entry's findings", async () => {
  await sprocket().getByRole("button", { name: "Sprocket F1" }).click();
  await expect(
    main().getByRole("heading", { name: "Sprocket F1" }),
  ).toBeVisible();
  const section = main().getByTestId("entry-findings");
  await section
    .getByRole("button", { name: /Findings about this entry/ })
    .click();
  const groups = section.getByTestId("finding-group");
  await expect(groups).toHaveCount(1);
  await expect(groups.locator(".badge.kind-warning")).toHaveCount(1);
  // The section stays open for the next inspection.
  await inspectGadget("Widget F1");
  await expect(main().getByTestId("no-entry-findings")).toBeVisible();
});

test("the tray folds the findings already in history into one line", async () => {
  const tray = page.getByTestId("tray");
  await inspectGadget("Sprocket F1");
  await main().getByRole("button", { name: "Revise", exact: true }).click();
  await tray.getByRole("button", { name: "Check", exact: true }).click();
  await expect(page.getByTestId("draft-status")).toHaveText("accepted");
  const report = tray.getByTestId("report");
  const history = report.getByTestId("history-findings");
  // The sprocket's own warning ends at the base: its revision supersedes it.
  await expect(history).toHaveText(/^already in history: \d+ warnings? ▸$/);
  await expect(report.getByTestId("findings")).toHaveCount(0);
  await history.click();
  await expect(report.getByTestId("findings")).toHaveCount(1);
  await expect(
    report.getByTestId("findings").getByText("resolved later in this draft"),
  ).toHaveCount(1);
  await tray.getByRole("button", { name: "Clear", exact: true }).click();
  await tray
    .getByRole("button", { name: "Clear the whole draft?", exact: true })
    .click();
  await expect(page.getByTestId("draft-count")).toHaveText("0 operations");
});
