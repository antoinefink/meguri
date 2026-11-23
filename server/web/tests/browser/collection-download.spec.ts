// SPDX-License-Identifier: MIT
// Synthetic browser fixture: native attachment save; backend authority uses HTTP tests.
import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import { readFile } from "node:fs/promises";
import AxeBuilder from "@axe-core/playwright";
const ticket = "d".repeat(43);
const payload = Buffer.from("SQLite format 3\0generated browser attachment");
async function fixture(page: Page, signedIn = true) {
  const state = { signedIn, calls: 0, sawFragment: false, refuse: false, username: "generated.member" };
  await page.context().route("**/api/**", async route => {
    const request = route.request(), url = new URL(request.url());
    const data = (value: unknown) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ data: value }) });
    if (url.pathname === "/api/status") {
      state.sawFragment ||= await page.evaluate(() => Boolean(location.hash)); return data({ setup_required: false });
    }
    if (url.pathname === "/api/me") return state.signedIn ? data({ id: state.username, username: state.username, role: "member" })
      : route.fulfill({ status: 401, contentType: "application/json", body: '{"error":{"code":"authentication_failed"}}' });
    if (url.pathname === "/api/login") { state.signedIn = true; return data({ id: state.username, username: state.username, role: "member" }); }
    if (url.pathname === "/api/logout") { state.signedIn = false; return data({}); }
    if (url.pathname === "/api/mcp/collection-download") {
      state.calls++;
      expect(request.method()).toBe("POST"); expect(url.search).toBe(""); expect(url.hash).toBe("");
      expect(request.headers()["content-type"]).toBe("application/x-www-form-urlencoded");
      expect(request.headers()["authorization"]).toBeUndefined(); expect(request.postData()).toBe(`ticket=${ticket}`);
      expect(request.headers()["origin"]).toBe(url.origin);
      expect(request.headers()["referer"]).toBe(`${url.origin}/mcp/download`);
      return state.refuse ? route.fulfill({ status: 403, contentType: "application/json", body: '{"error":{"code":"capability_invalid"}}' })
        : route.fulfill({ status: 200, headers: { "content-type": "application/vnd.sqlite3", "content-disposition": 'attachment; filename="collection.anki2"', "cache-control": "no-store" }, body: payload });
    }
    throw new Error(`Unexpected generated path: ${url.pathname}`);
  });
  return state;
}
async function privatePage(page: Page) {
  expect(await page.evaluate(() => location.hash)).toBe("");
  expect(await page.evaluate(() => history.state)).toBeNull();
  expect(await page.evaluate(() => JSON.stringify({ ...localStorage, ...sessionStorage }))).not.toContain(ticket);
}
async function login(page: Page) {
  await page.getByLabel("Username", { exact: true }).fill("generated.member");
  await page.getByLabel("Password", { exact: true }).fill("Synthetic download password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
}
test("fragment clears before sign-in; deliberate form saves a real attachment", async ({ page }, info) => {
  const state = await fixture(page, false);
  await page.goto(`/mcp/download#download=${ticket}`);
  await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  expect(state.sawFragment).toBe(false); await privatePage(page); expect(state.calls).toBe(0);
  await login(page);
  await expect(page.getByRole("heading", { name: "Download collection", exact: true })).toBeVisible();
  await expect(page.getByRole("navigation")).toHaveCount(0);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  // target=_blank gives the attachment navigation its own page; a download can
  // be emitted by either page depending on the browser's attachment handling.
  const saved = new Promise<import("@playwright/test").Download>(resolve => {
    page.once("download", resolve);
    page.context().once("page", popup => popup.once("download", resolve));
  });
  await page.getByRole("button", { name: "Download collection", exact: true }).click();
  const download = await saved;
  expect(download.suggestedFilename()).toBe("collection.anki2");
  const path = info.outputPath("collection.anki2"); await download.saveAs(path);
  expect(await readFile(path)).toEqual(payload);
  expect(state.calls).toBe(1); await privatePage(page);
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("request a new download link");
  await expect(page.getByRole("button", { name: "Download collection", exact: true })).toHaveCount(0);
});
test("a refused account can switch accounts without losing the unconsumed ticket", async ({ page }) => {
  const state = await fixture(page); state.refuse = true;
  await page.goto(`/mcp/download#download=${ticket}`);
  const popup = page.context().waitForEvent("page");
  await page.getByRole("button", { name: "Download collection", exact: true }).click();
  const refusal = await popup; await expect(refusal.locator("body")).toContainText("capability_invalid"); await refusal.close();
  await page.getByRole("button", { name: "Try again", exact: true }).click();
  await expect(page.getByRole("button", { name: "Download collection", exact: true })).toBeEnabled();
  expect(state.calls).toBe(1);
  await page.getByRole("button", { name: "Sign out generated.member", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  state.refuse = false; await login(page);
  await expect(page.locator('input[name="ticket"]')).toHaveValue(ticket);
  await expect(page.getByRole("button", { name: "Download collection", exact: true })).toBeEnabled();
  await privatePage(page); expect(state.calls).toBe(1);
});
for (const fragment of ["", "download=bad", `download=${ticket}&download=${ticket}`]) {
  test(`invalid download fragment ${fragment.slice(0, 16) || "absent"}`, async ({ page }) => {
    const state = await fixture(page); await page.goto(`/mcp/download${fragment ? `#${fragment}` : ""}`);
    await expect(page.getByRole("alert")).toContainText("request a new download link");
    await expect(page.getByRole("alert")).toBeFocused();
    await expect(page.getByRole("button", { name: "Download collection", exact: true })).toHaveCount(0);
    expect(state.calls).toBe(0); await privatePage(page);
  });
}
