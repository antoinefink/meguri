// SPDX-License-Identifier: MIT
// UI-only responses over the production bundle. Backend reset authority and
// durability are exercised separately by server_it::reset_data.
import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import type { Account } from "../../src/api";
import { storageValue } from "../../src/storage";

function barrier() {
  let release!: () => void;
  const promise = new Promise<void>(resolve => { release = resolve; });
  return { promise, release };
}
async function fixture(page: Page, origin: string) {
  const admin: Account = { id: "synthetic-admin", username: "sole.admin", role: "admin", enabled: true, provisioning_state: "ready", storage: { collection_bytes: 524288, media_bytes: 16384, total_bytes: 540672 } };
  const member: Account = { ...admin, id: "synthetic-member", username: "member." + "long".repeat(14) + "x", role: "member" };
  const state = { accounts: [admin, member], lists: 0, listError: false, resets: [] as string[], resetGate: null as Promise<void> | null, listGate: null as Promise<void> | null, nullStorage: false };
  await page.context().addCookies([{ name: "__Host-meguri-session", value: "synthetic-session", url: origin, secure: true, httpOnly: true, sameSite: "Strict" }]);
  await page.route("**/api/**", async route => {
    const req = route.request(), path = new URL(req.url()).pathname;
    const data = (value: unknown) => route.fulfill({ contentType: "application/json", body: JSON.stringify({ data: value }) });
    if (path === "/api/status") return data({ setup_required: false });
    if (path === "/api/me") return data({ ...admin, server_url: origin });
    if (path === "/api/me/storage") return data(admin.storage);
    if (path === "/api/me/backups") return data([]);
    if (path === "/api/admin/accounts") {
      state.lists++; await state.listGate;
      if (state.listError) return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: { code: "storage_unavailable" } }) });
      return data({ accounts: state.accounts });
    }
    if (path.endsWith("/reset-data")) {
      expect(req.method()).toBe("POST"); expect(req.postDataJSON()).toEqual({});
      state.resets.push(path); await state.resetGate;
      const id = path.split("/").at(-2), account = state.accounts.find(item => item.id === id)!;
      const result: Account = { ...account, storage: state.nullStorage ? null : { collection_bytes: 102400, media_bytes: 0, total_bytes: 102400 } };
      state.accounts = state.accounts.map(item => item.id === id ? result : item);
      return data(result);
    }
    throw new Error(`Unexpected synthetic API ${path}`);
  });
  return { state, admin, member };
}

test("Accounts disclosure, confirmations, direct reset results and narrow keyboard reflow", async ({ page, baseURL }) => {
  const { state, admin, member } = await fixture(page, baseURL!);
  const initial = barrier(); state.listGate = initial.promise;
  await page.goto("/accounts");
  await expect(page.getByRole("status").filter({ hasText: "Loading accounts…" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Refresh accounts", exact: true })).toHaveCount(0);
  initial.release();
  const heading = page.getByRole("heading", { name: "Server accounts", exact: true });
  await expect(heading).toBeVisible();
  const count = heading.getByRole("status").filter({ hasText: "2 accounts" });
  await expect(count).toBeVisible();
  const row = page.getByRole("listitem", { name: `Account ${member.username}`, exact: true });
  const actions = row.getByRole("button", { name: `Actions for ${member.username}`, exact: true });
  await expect(row.getByRole("button")).toHaveCount(1);
  await actions.focus(); await page.keyboard.press("Enter");
  await expect(actions).toHaveAttribute("aria-expanded", "true");
  await page.keyboard.press("ArrowDown");
  await expect(row.getByRole("button", { name: `Create reset link for ${member.username}` })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(actions).toBeFocused(); await expect(actions).toHaveAttribute("aria-expanded", "false");
  await actions.click(); await page.getByRole("heading", { name: "Accounts", exact: true }).click();
  await expect(actions).toHaveAttribute("aria-expanded", "false");
  await actions.click(); await page.keyboard.press("Tab");
  await expect(row.getByRole("button", { name: `Create reset link for ${member.username}` })).toBeFocused();
  await page.keyboard.press("Shift+Tab"); await page.keyboard.press("Shift+Tab");
  await expect(actions).toHaveAttribute("aria-expanded", "false");
  for (const width of [390, 320]) {
    await page.setViewportSize({ width, height: 844 });
    await actions.click();
    await page.evaluate(() => { document.documentElement.style.fontSize = "200%"; });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    const resetAction = row.getByRole("button", { name: `Reset data for ${member.username}`, exact: true });
    const bounds = (await resetAction.boundingBox())!;
    expect(bounds.x).toBeGreaterThanOrEqual(0); expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
    await resetAction.click();
    const confirmation = row.getByRole("region", { name: `Confirm resetting data for ${member.username}`, exact: true });
    await expect(confirmation.getByRole("button", { name: "Cancel", exact: true })).toBeFocused();
    await expect(confirmation).toContainText(`${member.username}’s collection and all media`);
    await expect(confirmation).toContainText("The account and device copies are kept.");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    if (width === 390) await page.keyboard.press("Escape");
    else await confirmation.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(actions).toBeFocused(); expect(state.resets).toEqual([]);
    await page.evaluate(() => { document.documentElement.style.removeProperty("font-size"); });
  }
  const held = barrier(); state.resetGate = held.promise;
  await actions.click(); await row.getByRole("button", { name: `Reset data for ${member.username}`, exact: true }).click();
  const confirm = row.locator('.confirmation button[data-variant="destructive"]');
  // Two synchronous activations exercise the ref guard before React rerenders.
  await confirm.evaluate((button: HTMLButtonElement) => { button.click(); button.click(); });
  await expect(confirm).toHaveAttribute("aria-disabled", "true");
  await expect.poll(() => state.resets.length).toBe(1);
  await page.keyboard.press("Escape"); await expect(confirm).toBeVisible();
  held.release();
  await expect(actions).toBeFocused();
  await expect(row.locator(".account-storage li").nth(0)).toContainText(storageValue(102400));
  await expect(row.locator(".account-storage li").nth(1)).toContainText("0 B");
  await expect(row.getByRole("status").filter({ hasText: `Reset server data for ${member.username}.` })).toBeVisible();
  await expect(count).toBeVisible(); expect(state.lists).toBe(1);
  // Sole administrator/self has the same data action and preserves the page.
  const self = page.getByRole("listitem", { name: `Account ${admin.username}`, exact: true });
  state.nullStorage = true;
  await self.getByRole("button", { name: `Actions for ${admin.username}`, exact: true }).click();
  await expect(self.getByRole("button", { name: `Delete ${admin.username}`, exact: true })).toHaveCount(0);
  await self.getByRole("button", { name: `Reset data for ${admin.username}`, exact: true }).click();
  await self.getByRole("button", { name: `Confirm reset data for ${admin.username}`, exact: true }).click();
  await expect(self.getByText("Storage unavailable", { exact: true })).toBeVisible();
  await expect(self.getByRole("button", { name: `Actions for ${admin.username}`, exact: true })).toBeFocused();
  await expect(page).toHaveURL(`${baseURL}/accounts`); expect(state.lists).toBe(1);
  const audit = await new AxeBuilder({ page }).analyze(); expect(audit.violations).toEqual([]);
});

test("Accounts initial failure is announced without a manual list refresh control", async ({ page, baseURL }) => {
  const { state } = await fixture(page, baseURL!); state.listError = true;
  await page.goto("/accounts");
  await expect(page.getByRole("alert")).toContainText("Storage is unavailable");
  await expect(page.getByRole("status").filter({ hasText: "Accounts could not be loaded. Reload the page to try again." })).toBeVisible();
  await expect(page.getByRole("button", { name: "Refresh accounts", exact: true })).toHaveCount(0);
  expect(state.lists).toBe(1);
});
