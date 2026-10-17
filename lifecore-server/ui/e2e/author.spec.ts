import { expect, type Page, test } from "@playwright/test";
import type {
  EntryId,
  Method,
  Params,
  ProposalEntry,
  Result,
  RpcResponse,
} from "../src/contract.ts";

// The authoring flow of Milestone 2: create through the form, check, and
// apply through the draft tray, revise with reconnection, recover from a
// stale base on Check and on Apply, share a draft with a second tab, and
// import. Files run in name order, so this one runs before browse.spec in
// the same database; it declares and uses its own `Job` type and never
// touches `Task`, whose counts browse.spec asserts. Each test starts from an
// empty draft and opens the screens it needs; analyzer wording is never
// asserted, only the kind of a finding and where it is attached.
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
  // A context of its own, so a test can open a second tab beside the page.
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
const status = () => page.getByTestId("draft-status");
const count = () => page.getByTestId("draft-count");
const trayButton = (name: string) =>
  tray().getByRole("button", { name, exact: true });
const operation = (name: string) =>
  tray()
    .getByTestId("operation")
    .filter({ has: page.getByRole("button", { name, exact: true }) });

// An empty draft with no banner left over from an earlier test.
async function fresh() {
  const applied = page.getByTestId("applied");
  if ((await applied.count()) > 0) {
    await applied.getByRole("button", { name: "Dismiss" }).click();
  }
  if ((await count().textContent()) !== "0 operations") {
    await trayButton("Clear").click();
    await trayButton("Clear the whole draft?").click();
  }
  await expect(count()).toHaveText("0 operations");
}

async function openJobs() {
  await page
    .getByTestId("types")
    .getByRole("button", { name: "Job", exact: true })
    .click();
  await expect(main().getByRole("heading", { name: "Job" })).toBeVisible();
}

async function newJob() {
  await openJobs();
  await main().getByRole("button", { name: "New node of this type" }).click();
  await expect(main().getByLabel("type", { exact: true })).toHaveValue("Job");
  await expect(page.getByTestId("plan-id")).toContainText("lc1:");
}

// Makes a declared field present, if it is not yet, and types its value.
async function setField(field: string, value: string) {
  const present = main().getByLabel(`${field} present`);
  if (!(await present.isChecked())) {
    await present.check();
  }
  await main().getByLabel(field, { exact: true }).fill(value);
}

async function checkAccepted() {
  await trayButton("Check").click();
  await expect(status()).toHaveText("accepted");
  await expect(trayButton("Apply")).toBeEnabled();
}

// Another writer moves the graph past the draft's plan ID.
async function writeElsewhere(title: string) {
  await call("proposal.apply", {
    proposal: {
      entries: [
        { name: "sneaked", kind: "node", ty_name: "Job", payload: { title } },
      ],
    },
  });
}

test("a new Job entered through the form is checked, applied, and listed", async () => {
  await fresh();
  await openJobs();
  await expect(table().locator("table.entries tbody tr")).toHaveCount(3);
  await newJob();
  await expect(main().getByLabel("name", { exact: true })).toHaveValue("job_1");
  await expect(trayButton("Apply")).toBeDisabled();
  await setField("title", "Water the plants");
  await setField("priority", "3");
  await setField("due", "2026-11-01T09:00:00.000000001Z");
  // The form wrote canonical envelopes, the timestamp at full precision.
  await main().getByRole("button", { name: "Raw JSON" }).click();
  const raw = main().getByLabel("payload JSON");
  await expect(raw).toHaveValue(
    /"priority": \{\s*"kind": "integer",\s*"value": "3"\s*\}/,
  );
  await expect(raw).toHaveValue(
    /"due": \{\s*"kind": "timestamp",\s*"value": "2026-11-01T09:00:00.000000001Z"\s*\}/,
  );
  await main().getByRole("button", { name: "Form" }).click();
  await checkAccepted();
  await expect(operation("job_1").getByTestId("check-status")).toHaveText("ok");
  await expect(page.getByTestId("apply-note")).toContainText(
    "without an evaluation time",
  );
  await trayButton("Apply").click();
  const applied = page.getByTestId("applied");
  await expect(applied).toContainText("job_1 → @");
  await expect(applied.getByTestId("applied-findings")).toContainText(
    "0 inconsistencies",
  );
  await expect(count()).toHaveText("0 operations");
  // Apply reloaded every screen: the table lists the new row.
  await openJobs();
  await expect(table().locator("table.entries tbody tr")).toHaveCount(4);
  await expect(
    table()
      .locator("td.label-cell")
      .getByRole("button", { name: "Water the plants" }),
  ).toBeVisible();
  await applied.getByRole("button", { name: "Dismiss" }).click();
  await expect(applied).toHaveCount(0);
});

test("revising a Job keeps the chosen edge and leaves the old ones historical", async () => {
  await fresh();
  await openJobs();
  // The label cell: the neighbours' relationship cells name it too.
  await table()
    .locator("td.label-cell")
    .getByRole("button", { name: "Draft agenda" })
    .click();
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
  await expect(count()).toHaveText("3 operations");
  await items.filter({ hasText: "Book room" }).getByRole("checkbox").uncheck();
  await expect(count()).toHaveText("2 operations");
  await expect(page.getByTestId("reconnect-count")).toHaveText(
    "1 of 2 current edges kept",
  );
  // The kept edge sits under its revision, collapsed until asked for.
  const kept = `job_1_keep_${ids.invites_after_agenda?.position}`;
  const group = tray().getByTestId("kept-group");
  await expect(group).toContainText("1 kept edge");
  await expect(operation(kept)).toHaveCount(0);
  await group.getByRole("button", { name: /kept edge/ }).click();
  await expect(operation(kept)).toContainText("DependsOn");
  await setField("title", "Draft the agenda");
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
  const relationships = page
    .getByTestId("relationships")
    .getByTestId("neighbour");
  await expect(relationships).toHaveCount(1);
  await expect(relationships.nth(0)).toContainText("Send invites");
  await expect(relationships.nth(0)).not.toContainText("historical");
  // The superseded revision keeps both of its edges, now historical, and
  // can only be branched: no new edge can end at it either.
  await lineage.nth(0).getByRole("button", { name: "Draft agenda" }).click();
  await expect(
    main().getByRole("heading", { name: "Draft agenda" }),
  ).toBeVisible();
  await expect(relationships).toHaveCount(2);
  for (const row of await relationships.all()) {
    await expect(row).toContainText("historical");
  }
  await expect(
    main().getByRole("button", { name: "Revise", exact: true }),
  ).toBeDisabled();
  const connect = main().getByRole("button", { name: "Connect" });
  await expect(connect).toBeDisabled();
  await expect(connect).toHaveAttribute(
    "title",
    "only a current node can be an end of an edge",
  );
  await expect(
    main().getByRole("button", { name: "Branch", exact: true }),
  ).toBeEnabled();
  await applied.getByRole("button", { name: "Dismiss" }).click();
});

test("a stale base on Check keeps the draft until Replan", async () => {
  await fresh();
  await newJob();
  await setField("title", "Late job");
  await writeElsewhere("Sneaked in");
  const plan = await page.getByTestId("plan-id").textContent();
  await trayButton("Check").click();
  const stale = page.getByTestId("stale");
  await expect(stale).toBeVisible();
  await expect(status()).toHaveText("stale base");
  await expect(count()).toHaveText("1 operation");
  await expect(trayButton("Apply")).toBeDisabled();
  await stale.getByRole("button", { name: "Replan" }).click();
  await expect(stale).toHaveCount(0);
  await expect(page.getByTestId("plan-id")).not.toHaveText(plan ?? "");
  await checkAccepted();
});

test("a stale base on Apply keeps the draft until Replan and a new check", async () => {
  await fresh();
  await newJob();
  await setField("title", "Later job");
  await checkAccepted();
  await writeElsewhere("Sneaked in again");
  await trayButton("Apply").click();
  const stale = page.getByTestId("stale");
  await expect(stale).toBeVisible();
  await expect(count()).toHaveText("1 operation");
  await expect(trayButton("Apply")).toBeDisabled();
  await stale.getByRole("button", { name: "Replan" }).click();
  await expect(stale).toHaveCount(0);
  // Replan clears the check: Apply waits for a new one.
  await expect(trayButton("Apply")).toBeDisabled();
  await expect(status()).toHaveText("not checked");
  await checkAccepted();
  await trayButton("Apply").click();
  await expect(page.getByTestId("applied")).toContainText("job_1 → @");
  await expect(count()).toHaveText("0 operations");
});

test("a lost apply answer leaves the draft uncertain until the user decides", async () => {
  await fresh();
  await newJob();
  await setField("title", "Maybe applied");
  await checkAccepted();
  // The apply request never reaches the server; the page only sees a lost
  // connection.
  await page.route("**/rpc", async (route) => {
    const body = route.request().postData() ?? "";
    if (body.includes('"method":"proposal.apply"')) {
      await route.abort();
    } else {
      await route.continue();
    }
  });
  await trayButton("Apply").click();
  const uncertain = page.getByTestId("uncertain");
  await expect(uncertain).toContainText(
    "This draft may already have been applied",
  );
  await expect(trayButton("Apply")).toBeDisabled();
  await page.unroute("**/rpc");
  // The record is stored with the draft and survives a reload.
  await page.reload();
  await expect(uncertain).toBeVisible();
  await expect(count()).toHaveText("1 operation");
  await uncertain.getByRole("button", { name: "Look in the graph" }).click();
  await expect(uncertain.getByTestId("verdict")).toContainText(
    "was not applied",
  );
  await expect(
    uncertain.getByRole("button", { name: /Entries are present/ }),
  ).toHaveCount(0);
  await uncertain
    .getByRole("button", { name: "Verified not applied: keep editing" })
    .click();
  await expect(uncertain).toHaveCount(0);
  // The check was used up: Apply needs a new one.
  await expect(trayButton("Apply")).toBeDisabled();
  await checkAccepted();
  await trayButton("Apply").click();
  await expect(page.getByTestId("applied")).toContainText("job_1 → @");
});

test("another tab's apply freezes the draft, and that tab closing mid-apply leaves it uncertain", async () => {
  await fresh();
  await newJob();
  await setField("title", "Applied elsewhere");
  // A second tab of the same browser shares the stored draft and follows
  // this one's edits.
  const other = await page.context().newPage();
  const otherTray = other.getByTestId("tray");
  const otherButton = (name: string) =>
    otherTray.getByRole("button", { name, exact: true });
  // Until its database answers, that tab takes no edit: the draft it would
  // land in is not yet the stored one.
  let describe = () => {};
  const described = new Promise<void>((resolve) => {
    describe = resolve;
  });
  await other.route("**/rpc", async (route) => {
    const body = route.request().postData() ?? "";
    if (body.includes('"method":"database.describe"')) {
      await described;
    }
    await route.continue();
  });
  await other.goto(launched().page);
  await otherTray
    .getByRole("button", { name: "expand the draft tray" })
    .click();
  await expect(other.getByTestId("draft-status")).toHaveText(
    "waiting for the database",
  );
  await expect(otherButton("New edge")).toBeDisabled();
  await expect(otherButton("Import")).toBeDisabled();
  describe();
  await expect(otherTray.getByTestId("draft-count")).toHaveText("1 operation");
  await other.unrouteAll();
  await newJob();
  await setField("title", "Also applied elsewhere");
  await expect(otherTray.getByTestId("draft-count")).toHaveText("2 operations");
  // The other tab checks and sends an apply that never answers.
  await other.route("**/rpc", async (route) => {
    const body = route.request().postData() ?? "";
    if (!body.includes('"method":"proposal.apply"')) {
      await route.continue();
    }
  });
  await otherButton("Check").click();
  await expect(other.getByTestId("draft-status")).toHaveText("accepted");
  await otherButton("Apply").click();
  await expect(other.getByTestId("draft-status")).toHaveText("applying…");
  // This tab is frozen by that apply: nothing here can overwrite its marker.
  await expect(status()).toHaveText("applying in another tab");
  await expect(page.getByTestId("applying-elsewhere")).toBeVisible();
  await expect(trayButton("New edge")).toBeDisabled();
  await expect(trayButton("Clear")).toBeDisabled();
  await expect(trayButton("Apply")).toBeDisabled();
  await expect(main().getByLabel("title", { exact: true })).toBeDisabled();
  // The other tab goes away before the answer: a reload here finds the
  // apply of uncertain fate, whether that tab still saw its request fail
  // (transport) or left only its in-flight marker (interrupted).
  await other.close();
  await page.reload();
  const uncertain = page.getByTestId("uncertain");
  await expect(uncertain).toContainText(
    "This draft may already have been applied",
  );
  await expect(uncertain).toContainText(/interrupted|transport/);
  await expect(count()).toHaveText("2 operations");
  await uncertain.getByRole("button", { name: "Look in the graph" }).click();
  await expect(uncertain.getByTestId("verdict")).toContainText(
    "was not applied",
  );
  await uncertain
    .getByRole("button", { name: "Verified not applied: keep editing" })
    .click();
  await expect(uncertain).toHaveCount(0);
});

test("Keep none and Remove ask first, Remove goes Back, and an explicit time is hinted as typed", async () => {
  await fresh();
  await openJobs();
  await table()
    .locator("td.label-cell")
    .getByRole("button", { name: "Send invites" })
    .click();
  await expect(
    main().getByRole("heading", { name: "Send invites" }),
  ).toBeVisible();
  await main().getByRole("button", { name: "Revise", exact: true }).click();
  const reconnect = page.getByTestId("reconnect");
  await expect(reconnect.getByRole("checkbox", { checked: true })).toHaveCount(
    1,
  );
  await expect(count()).toHaveText("2 operations");
  await reconnect.getByRole("button", { name: "Keep none" }).click();
  // The first click only asks.
  await expect(count()).toHaveText("2 operations");
  await reconnect
    .getByRole("button", { name: "Remove the kept edge?" })
    .click();
  await expect(count()).toHaveText("1 operation");
  await expect(page.getByTestId("reconnect-count")).toHaveText(
    "0 of 1 current edge kept",
  );
  // The explicit evaluation time names its form until it looks like one.
  const choice = tray().getByRole("combobox", { name: /^evaluation time/ });
  const time = tray().getByLabel("evaluation timestamp");
  const hint = tray().getByTestId("time-hint");
  await choice.selectOption("at");
  await expect(hint).toHaveText(
    "enter an RFC 3339 instant such as 2026-10-15T08:30:00Z",
  );
  await expect(trayButton("Check")).toBeDisabled();
  await time.fill("next tuesday");
  await expect(hint).toContainText("expected an RFC 3339 instant");
  await expect(time).toHaveAttribute("aria-invalid", "true");
  await time.fill("2026-10-15T08:30:00Z");
  await expect(hint).toHaveCount(0);
  await expect(time).toHaveAttribute("aria-invalid", "false");
  await choice.selectOption("none");
  // Remove leaves for the step before, the inspector Revise was used in.
  await main().getByRole("button", { name: "Remove", exact: true }).click();
  await expect(count()).toHaveText("1 operation");
  await main().getByRole("button", { name: "Remove job_1?" }).click();
  await expect(
    main().getByRole("heading", { name: "Send invites" }),
  ).toBeVisible();
  await expect(count()).toHaveText("0 operations");
});

test("an imported bare integer is a decimal string and an undeclared type blocks Apply", async () => {
  await fresh();
  // An empty draft folds the tray to a strip; opening it offers Import.
  await tray().getByRole("button", { name: "expand the draft tray" }).click();
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
  // One inconsistency, attached to the copy and not to the original.
  const gizmo = operation("imported_2");
  await expect(gizmo.getByTestId("check-status")).toHaveText("1 inconsistency");
  const finding = gizmo.getByTestId("findings").getByRole("listitem");
  await expect(finding.locator(".badge.kind-inconsistency")).toHaveCount(1);
  await expect(operation("imported").getByTestId("check-status")).toHaveText(
    "ok",
  );
  await expect(operation("imported").getByTestId("findings")).toHaveCount(0);
  await expect(trayButton("Apply")).toBeDisabled();
});

test("under an as-of boundary only a branch's predecessor is picked", async () => {
  await fresh();
  await openJobs();
  await table()
    .locator("td.label-cell")
    .getByRole("button", { name: "Book room" })
    .click();
  const heading = main().getByRole("heading", { name: "Book room" });
  await expect(heading).toBeVisible();
  // Connect picks the edge's other end; this node is current, so it could
  // be that end too.
  await main().getByRole("button", { name: "Connect" }).click();
  await expect(main().getByTestId("operation-screen")).toBeVisible();
  await page.getByRole("button", { name: "← Back" }).click();
  await expect(heading).toBeVisible();
  const pick = page.getByTestId("pick-banner");
  const useAs = (role: string) =>
    pick.getByRole("button", { name: `Use as ${role}` });
  await expect(useAs("to")).toBeEnabled();
  // A bounded read does not say whether the node is current in the state
  // the draft applies to, which an edge's end and a revise's predecessor
  // must be.
  await main().getByRole("button", { name: "As of here" }).click();
  await expect(page.getByTestId("as-of")).toBeVisible();
  await expect(useAs("to")).toBeDisabled();
  await expect(useAs("to")).toHaveAttribute(
    "title",
    /^drafts apply to the current state; clear the as-of boundary/,
  );
  await expect(main().getByRole("button", { name: "Connect" })).toBeDisabled();
  await pick.getByRole("button", { name: "Cancel" }).click();
  await expect(pick).toHaveCount(0);
  // A branch may start from any revision.
  await main().getByRole("button", { name: "Branch", exact: true }).click();
  await expect(main().getByLabel("name", { exact: true })).toHaveValue("job_1");
  await main().getByRole("button", { name: "pick predecessor" }).click();
  await table()
    .locator("td.label-cell")
    .getByRole("button", { name: "Book room" })
    .click();
  await expect(useAs("predecessor")).toBeEnabled();
  // The same pick for a revise waits for the boundary to be cleared.
  await trayButton("job_1").click();
  await main().getByRole("radio", { name: "revise" }).check();
  await main().getByRole("button", { name: "pick predecessor" }).click();
  await table()
    .locator("td.label-cell")
    .getByRole("button", { name: "Book room" })
    .click();
  await expect(useAs("predecessor")).toBeDisabled();
  await page
    .getByRole("banner")
    .getByRole("button", { name: "current" })
    .click();
  await expect(page.getByTestId("as-of")).toHaveCount(0);
  await expect(useAs("predecessor")).toBeEnabled();
  await useAs("predecessor").click();
  await expect(main().getByLabel("name", { exact: true })).toHaveValue("job_1");
  await expect(main().getByRole("radio", { name: "revise" })).toBeChecked();
});
