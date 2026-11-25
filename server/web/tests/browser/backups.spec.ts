// SPDX-License-Identifier: MIT
// Synthetic UI evidence over the compiled MG HTTPS fixture. Real HTTP restore,
// authority and recovery durability are verified by the backend's separate suite.
import { writeFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import type { AccountStorage, CollectionBackup } from "../../src/api";
import { storageValue } from "../../src/storage";

test.use({ timezoneId: "UTC", locale: "en-US" });
const point = (id: string, date: string, kind: CollectionBackup["kind"], size_bytes: number): CollectionBackup =>
  ({ id, created_at: Date.parse(date) / 1000, kind, size_bytes });
// A synthetic account with a short history and an earlier restore. Calendar
// dates cross real day, week and month boundaries; the API supplies their order.
const demo: CollectionBackup[] = [
  point("demo-nov25", "2026-11-25T06:15:00Z", "daily", 184_600_000),
  point("demo-safety", "2026-11-24T17:40:00Z", "before_restore", 186_200_000),
  point("demo-nov24", "2026-11-24T06:15:00Z", "daily", 183_900_000),
  point("demo-nov23", "2026-11-23T06:15:00Z", "daily", 183_100_000),
  point("demo-nov16", "2026-11-16T06:15:00Z", "weekly", 178_400_000),
  point("demo-oct01", "2026-10-01T06:15:00Z", "monthly", 167_800_000),
  point("demo-sep01", "2026-09-01T06:15:00Z", "monthly", 159_300_000),
];
const currentStorage: AccountStorage = { collection_bytes: 12_600_000, media_bytes: 172_000_000, total_bytes: 184_600_000 };
const restoredStorage: AccountStorage = { collection_bytes: 11_200_000, media_bytes: 167_200_000, total_bytes: 178_400_000 };
type Failure = { status: number; code: string } | null;
function barrier() {
  let release!: () => void;
  const promise = new Promise<void>(resolve => { release = resolve; });
  return { promise, release };
}
async function fixture(page: Page, origin: string, holdIdentity = false, holdStorage = false) {
  const state = {
    history: [...demo], storage: currentStorage as AccountStorage | null, historyReads: 0, storageReads: 0, restores: [] as string[],
    historyError: null as Failure, storageError: null as Failure, restoreError: null as Failure, loseRestoreResponse: false,
    restoreResult: null as unknown,
    historyGate: null as Promise<void> | null, storageGate: null as Promise<void> | null, restoreGate: null as Promise<void> | null,
  };
  // Observe completion after application promise callbacks, including late
  // failures. This makes negative assertions independent of timing sleeps.
  await page.addInitScript(({ holdIdentity, holdStorage }) => {
    const original = window.fetch.bind(window);
    let identitySignal: AbortSignal | null | undefined;
    const replace = window.history.replaceState.bind(window.history);
    window.history.replaceState = (...args: Parameters<typeof window.history.replaceState>) => {
      if (holdIdentity && args[2] === "/login") {
        document.documentElement.dataset.identityAbortedAtSessionEnd = String(identitySignal?.aborted);
        window.dispatchEvent(new Event("release-identity-json"));
      }
      replace(...args);
    };
    window.fetch = async (...args) => {
      if (holdIdentity && args[0] === "/api/me" && args[1]?.signal) identitySignal = args[1].signal;
      const response = await original(...args), json = response.json.bind(response);
      response.json = async () => {
        try {
          const value = await json(), root = document.documentElement, path = new URL(response.url).pathname;
          if (holdIdentity && path === "/api/me" && args[1]?.signal) {
            root.dataset.identityJsonWaiting = "true";
            await new Promise<void>(resolve => window.addEventListener("release-identity-json", () => resolve(), { once: true }));
          }
          if ((holdStorage && path === "/api/me/storage") || root.dataset.holdBackupJson === path) {
            holdStorage = false;
            root.dataset.backupJsonWaiting = path;
            await new Promise<void>(resolve => window.addEventListener("release-backup-json", () => resolve(), { once: true }));
          }
          return value;
        }
        finally { setTimeout(() => {
          const root = document.documentElement, path = new URL(response.url).pathname;
          const consumed = JSON.parse(root.dataset.apiConsumed ?? "{}") as Record<string, number>;
          consumed[path] = (consumed[path] ?? 0) + 1; root.dataset.apiConsumed = JSON.stringify(consumed);
        }, 0); }
      };
      return response;
    };
  }, { holdIdentity, holdStorage });
  await page.context().addCookies([{ name: "__Host-meguri-session", value: "synthetic-session", url: origin, secure: true, httpOnly: true, sameSite: "Strict" }]);
  await page.route("**/api/**", async route => {
    const req = route.request(), path = new URL(req.url()).pathname;
    const data = (value: unknown) => route.fulfill({ contentType: "application/json", body: JSON.stringify({ data: value }) });
    const fail = (error: NonNullable<Failure>) => route.fulfill({ status: error.status, contentType: "application/json", body: JSON.stringify({ error: { code: error.code } }) });
    if (path === "/api/status") return data({ setup_required: false });
    if (path === "/api/me") return data({ id: "demo-account", username: "demo.study", role: "member", server_url: origin });
    if (path === "/api/logout") return data(null);
    if (path === "/api/me/mcp-grants") return data({ account_id: "demo-account", grants: [] });
    if (path === "/api/me/storage") {
      expect(req.method()).toBe("GET");
      state.storageReads++; const error = state.storageError, value = state.storage;
      await state.storageGate; return error ? fail(error) : data(value);
    }
    if (path === "/api/me/backups") {
      expect(req.method()).toBe("GET");
      state.historyReads++; const error = state.historyError, value = [...state.history];
      await state.historyGate; return error ? fail(error) : data(value);
    }
    const match = /^\/api\/me\/backups\/([^/]+)\/restore$/.exec(path);
    if (match) {
      expect(req.method()).toBe("POST"); expect(req.postDataJSON()).toEqual({});
      expect(req.headers()["content-type"]).toBe("application/json");
      state.restores.push(match[1]!); const error = state.restoreError;
      await state.restoreGate;
      if (error) return fail(error);
      state.history = [point("demo-new-safety", "2026-11-25T14:30:00Z", "before_restore", state.storage?.total_bytes ?? 0),
        ...state.history.filter(item => item.id !== "demo-new-safety")];
      state.storage = restoredStorage;
      if (state.loseRestoreResponse) return route.abort("connectionfailed");
      return data(state.restoreResult);
    }
    throw new Error(`Unexpected synthetic API ${path}`);
  });
  return state;
}
const history = (page: Page) => page.getByRole("region", { name: "Backup history", exact: true });
const restoreButton = (page: Page) => history(page).getByRole("button", { name: "Restore backup from Nov 16, 2026, 6:15 AM UTC", exact: true });
const confirmation = (page: Page) => page.getByRole("region", { name: "Restore backup from Nov 16, 2026, 6:15 AM UTC?", exact: true });
async function ready(page: Page) {
  await page.goto("/collection");
  await expect(history(page).getByRole("status").filter({ hasText: "7 backups available." })).toBeVisible();
  await expect(page.locator(".collection-storage")).toBeVisible();
}
async function audit(page: Page, name: string, screenshot = false) {
  const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
  writeFileSync(`/out/collection-backups-${name}-axe.json`, JSON.stringify(result, null, 2));
  expect(result.violations).toEqual([]);
  if (screenshot) {
    await page.evaluate(async () => { await document.fonts.ready; });
    if (name === "confirmation") await confirmation(page).screenshot({ path: `/out/collection-backups-${name}.png` });
    else {
      await page.evaluate(() => { window.scrollTo(0, 0); });
      await page.screenshot({ path: `/out/collection-backups-${name}.png`, fullPage: true });
    }
  }
}

test("populated calendar history and safe keyboard restore confirmation screenshots", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!);
  await ready(page);
  const rows = history(page).getByRole("list", { name: "Collection backups" }).getByRole("listitem");
  await expect(rows).toHaveCount(demo.length);
  expect(await rows.locator("time").evaluateAll(elements => elements.map(element => element.getAttribute("datetime"))))
    .toEqual(demo.map(item => new Date(item.created_at * 1000).toISOString()));
  for (const [index, item] of demo.entries()) {
    await expect(rows.nth(index)).toContainText(storageValue(item.size_bytes));
  }
  for (const kind of ["Daily", "Weekly", "Monthly", "Before restore"]) await expect(history(page).locator(".backup-kind").filter({ hasText: kind }).first()).toBeVisible();
  await audit(page, "populated", true);
  const trigger = restoreButton(page);
  await trigger.focus(); await page.keyboard.press("Enter");
  const confirm = confirmation(page), cancel = confirm.getByRole("button", { name: "Cancel", exact: true });
  await expect(cancel).toBeFocused();
  await expect(confirm).toContainText("This replaces your collection on the server, including its review history, scheduling and media.");
  await expect(confirm).toContainText("A safety copy");
  await expect(confirm).toContainText("on each device");
  await expect(trigger).toHaveAttribute("aria-expanded", "true");
  await audit(page, "confirmation", true);
  await page.keyboard.press("Enter"); // Default focused action is Cancel.
  await expect(confirm).toHaveCount(0); await expect(trigger).toBeFocused();
  expect(state.restores).toEqual([]);
  await trigger.press("Enter"); await expect(cancel).toBeFocused();
  await page.keyboard.press("Escape"); await expect(trigger).toBeFocused();
  expect(state.restores).toEqual([]);
});

test("loading, empty and independent history/storage errors with explicit refresh", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!);
  const gate = barrier(); state.history = []; state.historyGate = gate.promise;
  await page.goto("/collection");
  await expect(history(page).getByRole("status").filter({ hasText: "Loading backup history…" })).toBeVisible();
  await expect(page.locator(".collection-storage")).toBeVisible();
  await history(page).getByRole("button", { name: "Refresh history" }).evaluate((button: HTMLButtonElement) => button.click());
  expect(state.historyReads).toBe(1);
  gate.release();
  await expect(history(page).getByText(/No backups yet/)).toBeVisible();
  await expect(history(page).getByRole("button", { name: /^Restore/ })).toHaveCount(0);
  await audit(page, "empty");
  state.historyGate = null; state.historyError = { status: 503, code: "storage_unavailable" };
  await history(page).getByRole("button", { name: "Refresh history" }).click();
  await expect(history(page).getByRole("alert")).toBeFocused();
  await expect(page.locator(".collection-storage")).toBeVisible();
  await audit(page, "history-error");
  state.historyError = null; state.history = [...demo];
  await history(page).getByRole("button", { name: "Refresh history" }).click();
  await expect(history(page).getByRole("listitem")).toHaveCount(demo.length);
  await expect(history(page).getByRole("alert")).toHaveCount(0);
  state.storage = null;
  await page.reload();
  await expect(page.getByRole("status").filter({ hasText: "Storage unavailable" })).toBeVisible();
  await expect(restoreButton(page)).toHaveAttribute("aria-disabled", "false");
  state.storageError = { status: 503, code: "storage_unavailable" };
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("Storage is unavailable");
  await expect(history(page).getByRole("listitem")).toHaveCount(demo.length);
  state.historyError = null;
  await history(page).getByRole("button", { name: "Refresh history" }).click();
  await expect(history(page).getByRole("status").filter({ hasText: "7 backups available." })).toBeVisible();
});

test("restore submits once, announces completion and refreshes both history and storage", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await ready(page);
  const gate = barrier(); state.restoreGate = gate.promise;
  await restoreButton(page).click();
  await confirmation(page).getByRole("button", { name: "Restore backup", exact: true }).evaluate((button: HTMLButtonElement) => { button.click(); button.click(); });
  const restoring = confirmation(page).getByRole("button", { name: "Restoring…", exact: true });
  await expect(restoring).toHaveAttribute("aria-disabled", "true");
  await expect(confirmation(page).getByRole("status")).toHaveText("Restoring the server collection and media…");
  await restoring.evaluate((button: HTMLButtonElement) => button.click());
  await confirmation(page).getByRole("button", { name: "Cancel" }).evaluate((button: HTMLButtonElement) => button.click());
  await page.keyboard.press("Escape");
  await history(page).getByRole("button", { name: "Refresh history" }).evaluate((button: HTMLButtonElement) => button.click());
  await restoreButton(page).evaluate((button: HTMLButtonElement) => button.click());
  expect(state.restores).toEqual(["demo-nov16"]); expect(state.historyReads).toBe(1); expect(state.storageReads).toBe(1);
  const refresh = barrier(); state.historyGate = refresh.promise; state.storageGate = refresh.promise;
  gate.release();
  await expect(confirmation(page)).toHaveCount(0);
  await expect(history(page).getByRole("heading", { name: "Backup history", exact: true })).toBeFocused();
  await expect(history(page).getByRole("status").filter({ hasText: "Restored the server collection and media" })).toContainText("On each device, download the restored server copy");
  await expect(restoreButton(page)).toHaveAttribute("aria-disabled", "true");
  await restoreButton(page).evaluate((button: HTMLButtonElement) => button.click());
  expect(state.restores).toHaveLength(1);
  refresh.release();
  await expect(history(page).getByRole("listitem")).toHaveCount(8);
  await expect(page.locator(".collection-storage")).toContainText(storageValue(restoredStorage.total_bytes));
  await expect(restoreButton(page)).toHaveAttribute("aria-disabled", "false");
  expect(state.historyReads).toBe(2); expect(state.storageReads).toBe(2);
});

test("slow storage permits recovery and its stale answer cannot replace the post-restore read", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!, false, true);
  await page.goto("/collection");
  await expect(page.locator("html")).toHaveAttribute("data-backup-json-waiting", "/api/me/storage");
  await expect(history(page).getByRole("listitem")).toHaveCount(7);
  await expect(restoreButton(page)).toHaveAttribute("aria-disabled", "false");
  const fresh = barrier(); state.storageGate = fresh.promise;
  await restoreButton(page).click();
  await confirmation(page).getByRole("button", { name: "Restore backup", exact: true }).click();
  await expect(history(page).getByRole("listitem")).toHaveCount(8);
  await expect(restoreButton(page)).toHaveAttribute("aria-disabled", "false");
  await expect(page.getByRole("status").filter({ hasText: "Loading storage…" })).toBeVisible();
  fresh.release();
  await expect(page.locator(".collection-storage")).toContainText(storageValue(restoredStorage.total_bytes));
  await page.evaluate(() => window.dispatchEvent(new Event("release-backup-json")));
  await expect.poll(() => page.evaluate(() => (JSON.parse(document.documentElement.dataset.apiConsumed ?? "{}") as Record<string, number>)["/api/me/storage"] ?? 0)).toBe(2);
  await expect(page.locator(".collection-storage")).toContainText(storageValue(restoredStorage.total_bytes));
  expect(state.storageReads).toBe(2); expect(state.restores).toEqual(["demo-nov16"]);
});

test("server failure requires explicit history review before another restore", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await ready(page);
  state.restoreError = { status: 503, code: "storage_unavailable" };
  await restoreButton(page).click();
  await confirmation(page).getByRole("button", { name: "Restore backup", exact: true }).click();
  await expect(history(page).getByRole("alert")).toBeFocused();
  await expect(history(page).getByRole("alert")).toContainText("The restore result is unknown");
  expect(state.restores).toHaveLength(1); expect(state.historyReads).toBe(1); expect(state.storageReads).toBe(1);
  await audit(page, "restore-error");
  await page.keyboard.press("Escape");
  await expect(history(page).getByRole("alert")).toHaveCount(0);
  await expect(history(page).getByRole("heading", { name: "Backup history", exact: true })).toBeFocused();
  await expect(restoreButton(page)).toHaveAttribute("aria-disabled", "true");
  await restoreButton(page).evaluate((button: HTMLButtonElement) => button.click());
  expect(state.restores).toHaveLength(1);
  await history(page).getByRole("button", { name: "Refresh history" }).click();
  await expect(restoreButton(page)).toHaveAttribute("aria-disabled", "false");
});

for (const result of ["lost", "malformed"] as const) {
  test(`${result} response after commit preserves the first safety point until history review`, async ({ page, baseURL }) => {
    const state = await fixture(page, baseURL!, false, result === "lost");
    if (result === "lost") {
      await page.goto("/collection");
      await expect(history(page).getByRole("listitem")).toHaveCount(7);
      await expect(page.locator("html")).toHaveAttribute("data-backup-json-waiting", "/api/me/storage");
    } else await ready(page);
    state.loseRestoreResponse = result === "lost"; state.restoreResult = {};
    await restoreButton(page).click();
    await confirmation(page).getByRole("button", { name: "Restore backup", exact: true }).click();
    await expect(history(page).getByRole("alert")).toBeFocused();
    await expect(history(page).getByRole("alert")).toContainText("The restore result is unknown");
    await expect(history(page).getByRole("alert")).toContainText("previous pre-restore safety copy");
    await expect(confirmation(page)).toHaveCount(0);
    await expect(page.getByRole("status").filter({ hasText: "Storage unavailable" })).toBeVisible();
    if (result === "lost") {
      await page.evaluate(() => window.dispatchEvent(new Event("release-backup-json")));
      await expect.poll(() => page.evaluate(() => (JSON.parse(document.documentElement.dataset.apiConsumed ?? "{}") as Record<string, number>)["/api/me/storage"] ?? 0)).toBe(1);
    }
    await expect(page.locator(".collection-storage")).toHaveCount(0);
    await expect(history(page).getByRole("status").filter({ hasText: "Restored the server collection" })).toHaveCount(0);
    const firstSafety = state.history[0];
    expect(firstSafety?.id).toBe("demo-new-safety"); expect(state.storage).toEqual(restoredStorage);
    await history(page).getByRole("button", { name: "Dismiss", exact: true }).click();
    await restoreButton(page).evaluate((button: HTMLButtonElement) => { button.click(); button.click(); });
    expect(state.restores).toEqual(["demo-nov16"]); expect(state.history[0]).toBe(firstSafety);
    expect(state.historyReads).toBe(1); expect(state.storageReads).toBe(1);
    await history(page).getByRole("button", { name: "Refresh history" }).click();
    await expect(history(page).getByRole("listitem")).toHaveCount(8);
    await expect(restoreButton(page)).toHaveAttribute("aria-disabled", "false");
    for (const value of Object.values(restoredStorage)) await expect(page.locator(".collection-storage")).toContainText(storageValue(value));
    expect(state.storageReads).toBe(2);
    expect(state.restores).toHaveLength(1); expect(state.history[0]).toBe(firstSafety);
  });
}

test("malformed history never renders restore choices and a missing backend route focuses its alert", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!);
  state.history = [{ ...demo[0]!, created_at: 8640000000001 }];
  await page.goto("/collection");
  await expect(history(page).getByRole("alert")).toContainText("The server response could not be confirmed");
  await expect(history(page).getByRole("listitem")).toHaveCount(0);
  state.historyError = { status: 404, code: "route_not_found" };
  await history(page).getByRole("button", { name: "Refresh history" }).click();
  await expect(history(page).getByRole("alert")).toContainText("This operation is unavailable");
  await expect(history(page).getByRole("alert")).toBeFocused();
  await expect(page.getByRole("main")).not.toBeFocused();
});

test("a completed restore stays announced when the follow-up reads fail", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await ready(page);
  const gate = barrier(); state.restoreGate = gate.promise;
  await restoreButton(page).click();
  await confirmation(page).getByRole("button", { name: "Restore backup", exact: true }).click();
  await expect.poll(() => state.restores.length).toBe(1);
  state.historyError = { status: 503, code: "storage_unavailable" }; state.storageError = state.historyError;
  gate.release();
  await expect(history(page).getByRole("status").filter({ hasText: "Restored the server collection and media" })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(2);
  await expect(restoreButton(page)).toHaveAttribute("aria-disabled", "true");
  await expect(page.locator(".collection-storage")).toHaveCount(0);
  expect(state.restores).toHaveLength(1); expect(state.historyReads).toBe(2); expect(state.storageReads).toBe(2);
  state.historyError = null;
  await history(page).getByRole("button", { name: "Refresh history" }).click();
  await expect(history(page).getByRole("alert")).toHaveCount(0);
  await expect(history(page).getByRole("listitem")).toHaveCount(8);
});

for (const failure of [{ status: 401, code: "authentication_failed" }, { status: 409, code: "stale_authority" }]) {
  for (const operation of ["history", "restore"] as const) {
    test(`${operation} ${failure.code} returns to sign-in`, async ({ page, baseURL }) => {
      const state = await fixture(page, baseURL!);
      if (operation === "history") { state.historyError = failure; await page.goto("/collection"); }
      else {
        await ready(page); state.restoreError = failure;
        await restoreButton(page).click();
        await confirmation(page).getByRole("button", { name: "Restore backup", exact: true }).click();
      }
      await expect(page).toHaveURL(/\/login$/);
      await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
      await expect(page.getByText("Your session is no longer available. Sign in again.", { exact: true })).toBeVisible();
      await expect(history(page)).toHaveCount(0);
    });
  }
}

for (const end of ["session loss", "sign out"] as const) {
  test(`${end} synchronously retires a sibling identity response before navigation`, async ({ page, baseURL }) => {
    const state = await fixture(page, baseURL!, true);
    const gate = barrier(); state.historyGate = gate.promise;
    if (end === "session loss") state.historyError = { status: 401, code: "authentication_failed" };
    await page.goto("/collection");
    await expect(page.locator("html")).toHaveAttribute("data-identity-json-waiting", "true");
    if (end === "session loss") gate.release();
    else await page.getByRole("button", { name: "Sign out", exact: true }).click();
    // replaceState releases the stale parsed identity in the same turn that
    // ends the session, before React's navigation-effect cleanup can run.
    await expect(page.locator("html")).toHaveAttribute("data-identity-aborted-at-session-end", "true");
    await expect.poll(() => page.evaluate(() => (JSON.parse(document.documentElement.dataset.apiConsumed ?? "{}") as Record<string, number>)["/api/me"] ?? 0)).toBe(2);
    await expect(page).toHaveURL(/\/login$/);
    await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Signed in as demo.study", exact: true })).toHaveCount(0);
    gate.release();
  });
}

for (const operation of ["history", "restore"] as const) {
  test(`late ${operation} failure after navigation cannot change the new screen`, async ({ page, baseURL }) => {
    const state = await fixture(page, baseURL!); await ready(page);
    const failure = { status: 409, code: "stale_authority" };
    const path = operation === "history" ? "/api/me/backups" : "/api/me/backups/demo-nov16/restore";
    await page.evaluate(path => { document.documentElement.dataset.holdBackupJson = path; }, path);
    if (operation === "history") {
      state.historyError = failure;
      await history(page).getByRole("button", { name: "Refresh history" }).click();
    } else {
      state.restoreError = failure;
      await restoreButton(page).click();
      await confirmation(page).getByRole("button", { name: "Restore backup", exact: true }).click();
    }
    // Delay JSON consumption rather than the network: the old component's
    // AbortController cannot mask the late error this regression exercises.
    await expect(page.locator("html")).toHaveAttribute("data-backup-json-waiting", path);
    await page.getByRole("link", { name: "Password", exact: true }).click();
    await expect(page.getByRole("heading", { name: "Change password", exact: true })).toBeVisible();
    await page.evaluate(() => window.dispatchEvent(new Event("release-backup-json")));
    await expect.poll(() => page.evaluate(path => (JSON.parse(document.documentElement.dataset.apiConsumed ?? "{}") as Record<string, number>)[path] ?? 0, path)).toBe(operation === "history" ? 2 : 1);
    await expect(page).toHaveURL(/\/settings$/);
    await expect(page.getByRole("alert")).toHaveCount(0);
  });
}

test("narrow layouts, large text and keyboard confirmation retain complete content", async ({ page, baseURL }) => {
  await fixture(page, baseURL!); await ready(page);
  for (const width of [390, 320]) {
    await page.setViewportSize({ width, height: 844 });
    await page.evaluate(() => { document.documentElement.style.fontSize = "200%"; });
    await restoreButton(page).focus(); await page.keyboard.press("Enter");
    await expect(confirmation(page).getByRole("button", { name: "Cancel" })).toBeFocused();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect(await history(page).locator("button, time, p, h2, h3").evaluateAll(elements => elements.every(element => element.scrollWidth <= element.clientWidth))).toBe(true);
    const buttons = await history(page).getByRole("button").evaluateAll(elements => elements.map(element => element.getBoundingClientRect()).map(rect => ({ width: rect.width, height: rect.height })));
    for (const button of buttons) { expect(button.width).toBeGreaterThanOrEqual(44); expect(button.height).toBeGreaterThanOrEqual(44); }
    await audit(page, `narrow-${width}`);
    await page.keyboard.press("Escape"); await expect(restoreButton(page)).toBeFocused();
    await page.evaluate(() => { document.documentElement.style.removeProperty("font-size"); });
  }
});
