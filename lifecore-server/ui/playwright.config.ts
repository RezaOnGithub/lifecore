import { defineConfig, devices } from "@playwright/test";

// Firefox is the tested browser; others are backlogged (PLAN §6).
export default defineConfig({
  testDir: "e2e",
  globalSetup: "./e2e/global-setup.ts",
  forbidOnly: true,
  retries: 0,
  // One server, one database: files run one at a time, in name order.
  workers: 1,
  reporter: "list",
  use: { trace: "retain-on-failure" },
  projects: [{ name: "firefox", use: { ...devices["Desktop Firefox"] } }],
});
