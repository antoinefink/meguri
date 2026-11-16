// SPDX-License-Identifier: MIT
import { defineConfig } from "@playwright/test";
const origin = process.env.MEGURI_WEB_BASE_URL;
if (!origin || new URL(origin).protocol !== "https:" || new URL(origin).origin !== origin) throw new Error("Provide the exact private HTTPS fixture origin");
export default defineConfig({
  testDir: "tests/browser", retries: 0, workers: 1, fullyParallel: false,
  timeout: 120_000, expect: { timeout: 8_000 },
  outputDir: "/out/browser-artifacts", reporter: [["list"], ["json", { outputFile: "/out/browser-results.json" }]],
  use: { baseURL: origin, browserName: "chromium", headless: true, ignoreHTTPSErrors: true,
    viewport: { width: 1280, height: 900 }, trace: "off", video: "off", screenshot: "off" },
});
