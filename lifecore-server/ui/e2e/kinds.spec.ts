import { expect, type Page, test } from "@playwright/test";
import type {
  Method,
  Params,
  ProposalEntry,
  Result,
  RpcResponse,
} from "../src/contract.ts";

// The value kinds that have no editor yet (UI, "Kinds without an editor"):
// a type using every one of them still has a table and an inspector, its
// values show as the server spelled them, and a form shows those fields
// read-only while the draft's raw JSON changes them. This file declares and
// uses its own `Stance` type.
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
    name: "stance_type",
    kind: "node",
    ty_name: "Type",
    payload: {
      name: "Stance",
      fields: {
        kind: "record",
        value: {
          title: ["mandatory", "text"],
          cost: ["optional", "duration"],
          deadline: ["optional", "date"],
          at: ["optional", "offset_timestamp"],
          per: ["mandatory", "enum", ["week", "day"]],
          decision: [
            "optional",
            "variant",
            {
              kind: "record",
              value: {
                committed: {
                  kind: "record",
                  value: { effort: ["mandatory", "duration"] },
                },
                resumed: { kind: "record", value: {} },
              },
            },
          ],
        },
      },
      outgoing: [],
      incoming: [],
      successors: [],
    },
  },
  {
    name: "walk",
    kind: "node",
    ty_name: "Stance",
    payload: {
      title: "Walk daily",
      // Spelled loosely; the server stores the canonical form.
      cost: { kind: "duration", value: "90m" },
      deadline: { kind: "date", value: "2026-11-06" },
      at: { kind: "offset_timestamp", value: "2026-10-29 08:15+01:00" },
      per: "day",
      decision: {
        kind: "variant",
        tag: "committed",
        value: { effort: { kind: "duration", value: "PT20M" } },
      },
    },
  },
];

let page: Page;

test.beforeAll(async ({ browser }) => {
  await call("proposal.apply", { proposal: { entries } });
  page = await (await browser.newContext()).newPage();
  await page.goto(launched().page);
  await expect(page.getByRole("heading", { name: "LifeCore" })).toBeVisible();
});

test.afterAll(async () => {
  await page?.context().close();
});

const main = () => page.getByRole("main");
const tray = () => page.getByTestId("tray");
const table = () => page.getByTestId("table");
const trayButton = (name: string) =>
  tray().getByRole("button", { name, exact: true });

async function openType(name: string) {
  await page
    .getByTestId("types")
    .getByRole("button", { name, exact: true })
    .click();
  await expect(main().getByRole("heading", { name })).toBeVisible();
}

test("the table and the inspector show each new kind as the server spelled it", async () => {
  await openType("Stance");
  const row = table().locator("table.entries tbody tr");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("PT1H30M");
  await expect(row).toContainText("2026-11-06");
  await expect(row).toContainText("2026-10-29T08:15:00+01:00");
  await expect(row).toContainText("committed {effort: PT20M}");
  await table()
    .locator("td.label-cell")
    .getByRole("button", { name: "Walk daily" })
    .click();
  await expect(
    main().getByRole("heading", { name: "Walk daily" }),
  ).toBeVisible();
  const payload = main().locator("table.payload").first();
  await expect(payload).toContainText("enum day | week");
  await expect(payload).toContainText(
    "variant committed {effort: duration} | resumed {}",
  );
  await expect(payload).toContainText("PT1H30M");
});

test("a revision shows the fields without an editor read-only and edits them as JSON", async () => {
  await main().getByRole("button", { name: "Revise", exact: true }).click();
  const cost = main().getByLabel("cost (read-only)");
  await expect(cost).toHaveValue('{"kind":"duration","value":"PT1H30M"}');
  await expect(cost).toHaveAttribute("readonly", "");
  await expect(main().getByLabel("per (read-only)")).toHaveValue('"day"');
  await expect(main().getByLabel("decision (read-only)")).toHaveValue(
    '{"kind":"variant","tag":"committed","value":{"effort":{"kind":"duration","value":"PT20M"}}}',
  );
  await expect(main().getByLabel("cost present")).toBeDisabled();
  await expect(main().getByText(/no editor for duration yet/)).toBeVisible();
  // Text keeps its editor beside them.
  await main().getByLabel("title", { exact: true }).fill("Walk every day");
  // The raw JSON reaches every shape; the time text stays as typed until
  // the server reads it.
  await main().getByRole("button", { name: "Raw JSON" }).click();
  const raw = main().getByLabel("payload JSON");
  const text = await raw.inputValue();
  await raw.fill(
    text
      .replace('"PT1H30M"', '"2h"')
      .replace('"committed"', '"resumed"')
      .replace(/"effort": \{[^}]*\}/, ""),
  );
  await main().getByRole("button", { name: "Form" }).click();
  await expect(main().getByLabel("cost (read-only)")).toHaveValue(
    '{"kind":"duration","value":"2h"}',
  );
  await trayButton("Check").click();
  await expect(page.getByTestId("draft-status")).toHaveText("accepted");
  await trayButton("Apply").click();
  await expect(page.getByTestId("applied")).toContainText("→ @");
  await openType("Stance");
  const row = table().locator("table.entries tbody tr");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("PT2H");
  await expect(row).toContainText("resumed {}");
});

test("a declaration using the new words shows those shapes read-only", async () => {
  await page
    .getByTestId("applied")
    .getByRole("button", { name: "Dismiss" })
    .click();
  await openType("Type");
  await table()
    .locator("td.label-cell")
    .getByRole("button", { name: "Stance" })
    .click();
  await main().getByRole("button", { name: "Revise", exact: true }).click();
  await expect(main().getByLabel("per shape (read-only)")).toHaveValue(
    '["enum",["day","week"]]',
  );
  await expect(main().getByLabel("cost shape (read-only)")).toHaveValue(
    '["duration"]',
  );
  // The words the builder offers keep their select.
  await expect(main().getByLabel("title shape")).toHaveValue("text");
  await trayButton("Clear").click();
  await trayButton("Clear the whole draft?").click();
  await expect(page.getByTestId("draft-count")).toHaveText("0 operations");
});
