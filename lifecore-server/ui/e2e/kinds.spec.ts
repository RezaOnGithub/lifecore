import { expect, type Page, test } from "@playwright/test";
import type {
  Method,
  Params,
  ProposalEntry,
  Result,
  RpcResponse,
} from "../src/contract.ts";

// The value kinds of the core round (UI §5, "Editors for the value
// kinds"): a type using every one of them has a table and an inspector, its
// values show as the server spelled them, and the form authors each kind,
// in values and in declarations, through check and apply. Offset
// timestamps take the browser's offset at the instant typed, so the pages
// here pin their zone. This file declares and uses its own `Stance` and
// `Habit` types.
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
      at: {
        kind: "offset_timestamp",
        value: "2026-10-29t08:15:00.123456789+01:00",
      },
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
  // India has kept +05:30 all year since 1945: a nonzero offset that is
  // not whole hours.
  page = await (
    await browser.newContext({ timezoneId: "Asia/Kolkata" })
  ).newPage();
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
const field = (label: string) => main().getByLabel(label, { exact: true });

async function openType(name: string) {
  await page
    .getByTestId("types")
    .getByRole("button", { name, exact: true })
    .click();
  await expect(main().getByRole("heading", { name })).toBeVisible();
}

async function checkAndApply() {
  await trayButton("Check").click();
  await expect(page.getByTestId("draft-status")).toHaveText("accepted");
  await trayButton("Apply").click();
  await expect(page.getByTestId("applied")).toContainText("→ @");
  await page
    .getByTestId("applied")
    .getByRole("button", { name: "Dismiss" })
    .click();
}

async function stanceRow(title: string) {
  await openType("Stance");
  const row = table()
    .locator("table.entries tbody tr")
    .filter({ hasText: title });
  await expect(row).toHaveCount(1);
  return row;
}

test("the table and the inspector show each new kind as the server spelled it", async () => {
  const row = await stanceRow("Walk daily");
  await expect(row).toContainText("PT1H30M");
  await expect(row).toContainText("2026-11-06");
  await expect(row).toContainText("2026-10-29T08:15:00.123456789+01:00");
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

test("a revision edits each kind through its editor", async () => {
  await main().getByRole("button", { name: "Revise", exact: true }).click();
  await expect(main().getByText("(read-only)")).toHaveCount(0);
  // Each editor shows the stored value.
  await expect(field("cost")).toHaveValue("PT1H30M");
  await expect(field("deadline")).toHaveValue("2026-11-06");
  await expect(field("deadline")).toHaveAttribute("type", "date");
  // The picker shows the value's own wall clock to the second; the text
  // keeps its fraction and offset.
  await expect(field("at")).toHaveAttribute("type", "datetime-local");
  await expect(field("at")).toHaveValue("2026-10-29T08:15");
  await expect(field("at as text")).toHaveValue(
    "2026-10-29T08:15:00.123456789+01:00",
  );
  await expect(field("per")).toHaveValue("day");
  await expect(field("decision tag")).toHaveValue("committed");
  await expect(field("decision(committed).effort")).toHaveValue("PT20M");

  // Either duration spelling; the server keeps ISO 8601.
  await field("cost").fill("2h 15m");
  await field("deadline").fill("2026-12-01");
  // Typed here, at +05:30: the offset is this browser's, and the fraction
  // the picker does not show is kept.
  await field("at").fill("2026-10-30T09:45");
  await expect(field("at as text")).toHaveValue(
    "2026-10-30T09:45:00.123456789+05:30",
  );
  await field("per").selectOption("week");
  await field("decision(committed).effort").fill("45m");
  // Switching tags shows the other tag's fields and keeps the body left.
  await field("decision tag").selectOption("resumed");
  await expect(field("decision(committed).effort")).toHaveCount(0);
  await expect(
    main().getByText("new field in decision(resumed)"),
  ).toBeVisible();
  await field("decision tag").selectOption("committed");
  await expect(field("decision(committed).effort")).toHaveValue("45m");
  await checkAndApply();

  const row = await stanceRow("Walk daily");
  await expect(row).toContainText("PT2H15M");
  await expect(row).toContainText("2026-12-01");
  await expect(row).toContainText("2026-10-30T09:45:00.123456789+05:30");
  await expect(row).toContainText("week");
  await expect(row).toContainText("committed {effort: PT45M}");
});

test("a new node gets each kind through the form", async () => {
  await openType("Stance");
  await main().getByRole("button", { name: "New node of this type" }).click();
  await field("title present").check();
  await field("title").fill("Read");
  // Nothing is chosen for an enum or a variant until the user does.
  await expect(field("per present")).not.toBeChecked();
  await field("per present").check();
  await expect(field("per")).toHaveValue("");
  await field("per").selectOption("day");
  await field("decision present").check();
  await field("decision tag").selectOption("committed");
  await field("decision(committed).effort present").check();
  await field("decision(committed).effort").fill("PT30M");
  await field("cost present").check();
  await field("cost").fill("1h");
  await field("deadline present").check();
  await field("deadline").fill("2027-01-15");
  // `now` captures the instant with this browser's offset, never `Z`.
  await field("at present").check();
  await main().getByRole("button", { name: "now for at" }).click();
  await expect(field("at as text")).toHaveValue(
    /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,3})?\+05:30$/,
  );
  const now = await field("at as text").inputValue();
  await checkAndApply();

  const row = await stanceRow("Read");
  await expect(row).toContainText("PT1H");
  await expect(row).toContainText("2027-01-15");
  await expect(row).toContainText(now);
  await expect(row).toContainText("day");
  await expect(row).toContainText("committed {effort: PT30M}");
});

test("an offset timestamp takes the browser's offset at the instant typed", async ({
  browser,
}) => {
  const typed: [string, string, string, RegExp | null][] = [
    // New York changes offset twice a year: each date gets its own.
    ["America/New_York", "2026-07-01T09:30", "2026-07-01T09:30:00-04:00", null],
    ["America/New_York", "2026-12-01T09:30", "2026-12-01T09:30:00-05:00", null],
    // 02:30 is skipped on 2026-03-08, and 01:30 occurs twice on
    // 2026-11-01.
    [
      "America/New_York",
      "2026-03-08T02:30",
      "2026-03-08T03:30:00-04:00",
      /02:30 does not occur on 2026-03-08/,
    ],
    [
      "America/New_York",
      "2026-11-01T01:30",
      "2026-11-01T01:30:00-04:00",
      /01:30 occurs twice on 2026-11-01/,
    ],
    // A zero offset is +00:00, a known one, not Z.
    ["UTC", "2026-07-01T09:30:15", "2026-07-01T09:30:15+00:00", null],
  ];
  for (const zone of ["America/New_York", "UTC"]) {
    const context = await browser.newContext({ timezoneId: zone });
    const other = await context.newPage();
    await other.goto(launched().page);
    const inMain = (label: string) =>
      other.getByRole("main").getByLabel(label, { exact: true });
    await other
      .getByTestId("types")
      .getByRole("button", { name: "Stance", exact: true })
      .click();
    await other
      .getByRole("main")
      .getByRole("button", { name: "New node of this type" })
      .click();
    await inMain("title present").check();
    await inMain("title").fill(`Typed in ${zone}`);
    await inMain("per present").check();
    await inMain("per").selectOption("week");
    await inMain("at present").check();
    for (const [, wall, value, note] of typed.filter(([z]) => z === zone)) {
      await inMain("at").fill(wall);
      await expect(inMain("at as text")).toHaveValue(value);
      if (note !== null) {
        await expect(other.getByRole("main").getByText(note)).toBeVisible();
      }
    }
    // The gap's note names the wall clock written, which the picker shows.
    if (zone === "America/New_York") {
      await inMain("at").fill("2026-03-08T02:30");
      await expect(inMain("at")).toHaveValue("2026-03-08T03:30");
    }
    const last = await inMain("at as text").inputValue();
    const otherTray = other.getByTestId("tray");
    await otherTray.getByRole("button", { name: "Check", exact: true }).click();
    await expect(other.getByTestId("draft-status")).toHaveText("accepted");
    await otherTray.getByRole("button", { name: "Apply", exact: true }).click();
    await expect(other.getByTestId("applied")).toContainText("→ @");
    await context.close();
    // Written by another page: this one reads it after a refresh.
    await page.getByRole("button", { name: "Refresh", exact: true }).click();
    const row = await stanceRow(`Typed in ${zone}`);
    await expect(row).toContainText(last);
  }
});

test("the declaration form builds each new word", async () => {
  await openType("Type");
  await main().getByRole("button", { name: "New node of this type" }).click();
  await field("declared name").fill("Habit");
  const add = async (
    name: string,
    presence: string,
    shape: string,
    first?: string,
    prefix = "",
  ) => {
    const what = `new field declaration${prefix === "" ? "" : ` in ${prefix}`}`;
    await field(what).fill(name);
    await field(`${what} presence`).selectOption(presence);
    await field(`${what} shape`).selectOption(shape);
    if (first !== undefined) {
      const word = shape === "enum" ? "member" : "tag";
      // The declaration needs it, so Add waits for it.
      await expect(
        main().getByRole("button", {
          name:
            prefix === ""
              ? "Add field declaration"
              : `Add field declaration to ${prefix}`,
          exact: true,
        }),
      ).toBeDisabled();
      await field(`${what} first ${word}`).fill(first);
    }
    await field(what).press("Enter");
    await expect(
      field(`${prefix === "" ? "" : `${prefix}.`}${name} shape`),
    ).toHaveValue(shape);
  };
  await add("effort", "optional", "duration");
  await add("due", "optional", "date");
  await add("at", "mandatory", "offset_timestamp");
  await add("per", "mandatory", "enum", "week");
  await field("new member of per").fill("day");
  await field("new member of per").press("Enter");
  // Members come in code point order, as the server's set holds them.
  await expect(field("per member 1")).toHaveValue("day");
  await expect(field("per member 2")).toHaveValue("week");
  await field("new member of per").fill("week");
  await expect(main().getByText("another member is week")).toBeVisible();
  await field("new member of per").fill("windows");
  await field("new member of per").press("Enter");
  await field("per member 3").fill("window");
  await expect(
    main().getByRole("button", { name: "remove day from per" }),
  ).toBeEnabled();
  await add("decision", "optional", "variant", "committed");
  await add("cost", "mandatory", "duration", undefined, "decision(committed)");
  await field("new tag of decision").fill("resumed");
  await main().getByRole("button", { name: "add tag to decision" }).click();
  await expect(field("decision tag 2")).toHaveValue("resumed");
  // A shape changed to an enum waits for its first member.
  await add("label", "optional", "text");
  await field("label shape").selectOption("enum");
  await field("label first member").fill("a");
  await main().getByRole("button", { name: "make enum of label" }).click();
  await expect(field("label member 1")).toHaveValue("a");
  // The last member cannot go: an enum needs one.
  await expect(
    main().getByRole("button", { name: "remove a from label" }),
  ).toBeDisabled();
  await checkAndApply();

  const listed = await call("types.list", {});
  const habit = listed.types.find((type) => type.name === "Habit");
  expect(habit?.resolution.kind).toBe("declared");
  expect(
    habit?.resolution.kind === "declared" ? habit.resolution.schema : null,
  ).toEqual({
    name: "Habit",
    fields: {
      at: { status: "mandatory", shape: { kind: "offset_timestamp" } },
      decision: {
        status: "optional",
        shape: {
          kind: "variant",
          tags: {
            committed: {
              cost: { status: "mandatory", shape: { kind: "duration" } },
            },
            resumed: {},
          },
        },
      },
      due: { status: "optional", shape: { kind: "date" } },
      effort: { status: "optional", shape: { kind: "duration" } },
      label: { status: "optional", shape: { kind: "enum", members: ["a"] } },
      per: {
        status: "mandatory",
        shape: { kind: "enum", members: ["day", "week", "window"] },
      },
    },
    outgoing: [],
    incoming: [],
    successors: [],
  });
});

test("a revised declaration shows its new words in the builder", async () => {
  await openType("Type");
  await table()
    .locator("td.label-cell")
    .getByRole("button", { name: "Stance" })
    .click();
  await main().getByRole("button", { name: "Revise", exact: true }).click();
  await expect(main().getByText("(read-only)")).toHaveCount(0);
  await expect(field("per shape")).toHaveValue("enum");
  await expect(field("per member 1")).toHaveValue("day");
  await expect(field("cost shape")).toHaveValue("duration");
  await expect(field("decision shape")).toHaveValue("variant");
  await expect(field("decision tag 1")).toHaveValue("committed");
  await expect(field("decision(committed).effort shape")).toHaveValue(
    "duration",
  );
  // Choosing enum over another shape and cancelling keeps the shape.
  await field("title shape").selectOption("variant");
  await main()
    .getByRole("button", { name: "cancel variant for title" })
    .click();
  await expect(field("title shape")).toHaveValue("text");
  await trayButton("Clear").click();
  await trayButton("Clear the whole draft?").click();
  await expect(page.getByTestId("draft-count")).toHaveText("0 operations");
});
