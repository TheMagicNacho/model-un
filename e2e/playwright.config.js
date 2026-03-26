// @ts-check
const { defineConfig, devices } = require("@playwright/test");

module.exports = defineConfig({
  testDir: "./tests",

  // Allow up to 30 minutes for the full 12-player golden-path test.
  timeout: 30 * 60 * 1000,

  // Do not retry on CI – failures are deterministic.
  retries: 0,

  // Tests must run serially; they share a live server.
  workers: 1,

  reporter: [
    ["html", { open: "never", outputFolder: "playwright-report" }],
    ["json", { outputFile: "test-results/results.json" }],
    ["list"],
  ],

  use: {
    // Base URL is injected via the BASE_URL environment variable in CI.
    // Falls back to the default local-dev server address.
    baseURL: process.env.BASE_URL || "http://localhost:3000",

    // Capture traces and screenshots only on failure to keep artifacts small.
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
