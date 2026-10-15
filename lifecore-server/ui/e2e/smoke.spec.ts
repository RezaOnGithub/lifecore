import { expect, test } from "@playwright/test";

function launched(): { url: string; database: string } {
  const url = process.env.LIFECORE_URL;
  const database = process.env.LIFECORE_DATABASE;
  if (!url || !database) {
    throw new Error("the global setup did not start lifecore serve");
  }
  return { url, database };
}

test("the printed URL opens the client and shows the database description", async ({
  page,
}) => {
  const { url, database } = launched();
  await page.goto(url);
  await expect(page.getByRole("heading", { name: "LifeCore" })).toBeVisible();
  await expect(page.getByTestId("database")).toHaveText(database);
  await expect(page.getByTestId("entries")).toHaveText("0");
  await expect(page.getByText("Type_Check")).toBeVisible();
  // The token left the address bar but stays in session storage.
  expect(new URL(page.url()).hash).toBe("");
  await page.reload();
  await expect(page.getByTestId("database")).toHaveText(database);
});

test("without the launch token the page says how to open it", async ({
  page,
}) => {
  const { url } = launched();
  await page.goto(url.slice(0, url.indexOf("#")));
  const alert = page.getByRole("alert");
  await expect(alert).toContainText("Not authorized (HTTP 401, token)");
  await expect(alert).toContainText("lifecore serve");
  await expect(page.getByTestId("database")).toHaveCount(0);
});
