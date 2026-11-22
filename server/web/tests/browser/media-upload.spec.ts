// SPDX-License-Identifier: MIT
// Compiled browser UI with synthetic responses; real authority is covered by HTTP tests.
import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
const uploadId = "u".repeat(43);
async function fixture(page: Page, origin: string, signedIn = true) {
  const state = { session: signedIn, username: "generated.member", accountId: "generated-account", logouts: 0, calls: 0, status: 200, pause: null as Promise<void> | null, sawFragment: false };
  await page.route("**/mcp/upload*", async route => {
    const response = await route.fetch({ url: `${origin}/login` }); await route.fulfill({ response });
  });
  await page.route("**/api/**", async route => {
    const request = route.request(), path = new URL(request.url()).pathname;
    const data = (value: unknown) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ data: value }) });
    const error = (status: number, code: string) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify({ error: { code } }) });
    if (path === "/api/status") { state.sawFragment ||= await page.evaluate(() => Boolean(location.hash)); return data({ setup_required: false }); }
    if (path === "/api/me") return state.session ? data({ id: state.accountId, username: state.username, role: "member", server_url: origin }) : error(401, "authentication_failed");
    if (path === "/api/logout") { state.session = false; state.logouts++; return data({ logged_out: true }); }
    if (path === "/api/login") { state.session = true; return data({ id: state.accountId, username: state.username, role: "member" }); }
    if (path === "/api/mcp/media-upload") {
      state.calls++;
      expect(request.headers()["x-meguri-upload"]).toBe(uploadId);
      expect(request.headers()["content-type"]).toBe("application/octet-stream");
      expect(request.headers()["authorization"]).toBeUndefined();
      expect(request.postDataBuffer()).toEqual(Buffer.from("synthetic file payload"));
      if (state.pause) await state.pause;
      return state.status === 200 ? data({ uploaded: true }) : error(state.status, state.status === 400 ? "attachment_invalid" : "capability_invalid");
    }
    throw new Error(`Unexpected synthetic API path: ${path}`);
  });
  return state;
}
async function privateMemory(page: Page) {
  expect(await page.evaluate(() => location.hash)).toBe("");
  expect(await page.evaluate(() => history.state)).toBeNull();
  expect(await page.evaluate(() => JSON.stringify({ ...localStorage, ...sessionStorage }))).not.toContain(uploadId);
}
async function choose(page: Page, bytes = Buffer.from("synthetic file payload")) {
  await page.getByLabel("File", { exact: true }).setInputFiles({ name: "synthetic.png", mimeType: "image/png", buffer: bytes });
}
test("a refused signed-in account can sign out and resume the same unconsumed link with the right account", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!);
  state.accountId = "generated-wrong-account"; state.username = "generated.other"; state.status = 403;
  await page.goto(`/mcp/upload#upload=${uploadId}`);
  await expect(page.getByRole("group", { name: "Signed-in account" })).toContainText("Signed in as generated.other.");
  await choose(page); await page.getByRole("button", { name: "Upload file", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Sign out and sign in");
  await expect(page.getByLabel("File", { exact: true })).toHaveCount(0);
  expect(state.calls).toBe(1); await privateMemory(page);
  await page.getByRole("button", { name: "Sign out generated.other", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  expect(state.logouts).toBe(1); expect(state.session).toBe(false);
  expect(new URL(page.url()).pathname).toBe("/mcp/upload"); await privateMemory(page);
  state.accountId = "generated-account"; state.username = "generated.member"; state.status = 200;
  await page.getByLabel("Username", { exact: true }).fill(state.username);
  await page.getByLabel("Password", { exact: true }).fill("Synthetic upload password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("group", { name: "Signed-in account" })).toContainText("Signed in as generated.member.");
  await choose(page); await page.getByRole("button", { name: "Upload file", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "File uploaded. Return to your chat" })).toBeVisible();
  expect(state.calls).toBe(2); await privateMemory(page);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});
test("signed-out upload resumes sign-in, clears fragment before bootstrap, and returns to chat once", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!, false);
  await page.goto(`/mcp/upload#upload=${uploadId}`);
  await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  expect(state.sawFragment).toBe(false); await privateMemory(page);
  await page.getByLabel("Username", { exact: true }).fill("generated.member");
  await page.getByLabel("Password", { exact: true }).fill("Synthetic upload password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Attach a file", exact: true })).toBeVisible();
  await expect(page.getByRole("navigation")).toHaveCount(0);
  await choose(page);
  let resume!: () => void; state.pause = new Promise<void>(resolve => { resume = resolve; });
  const submit = page.getByRole("button", { name: "Upload file", exact: true }); await submit.focus(); await page.keyboard.press("Enter");
  await expect(page.getByRole("status").filter({ hasText: "Uploading file…" })).toBeVisible();
  await expect.poll(() => state.calls).toBe(1);
  await page.getByRole("button", { name: "Uploading…", exact: true }).evaluate(button => { (button as HTMLButtonElement).click(); });
  expect(state.calls).toBe(1); resume();
  await expect(page.getByRole("status").filter({ hasText: "File uploaded. Return to your chat" })).toBeVisible();
  await expect(page.getByLabel("File", { exact: true })).toHaveCount(0); await privateMemory(page);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("Return to your chat"); expect(state.calls).toBe(1);
});
for (const fragment of ["", "upload=bad", `upload=${uploadId}&upload=${uploadId}`]) {
  test(`unavailable upload fragment (${fragment.slice(0, 12) || "bare"}) has no picker`, async ({ page, baseURL }) => {
    const state = await fixture(page, baseURL!); await page.goto(`/mcp/upload${fragment ? `#${fragment}` : ""}`);
    await expect(page.getByRole("alert")).toContainText("Return to your chat");
    await expect(page.getByRole("alert")).toBeFocused(); await expect(page.getByLabel("File", { exact: true })).toHaveCount(0);
    expect(state.calls).toBe(0); await privateMemory(page);
  });
}
for (const status of [400, 401, 403, 410, 500]) {
  test(`upload refusal (${status}) clears the capability without reissue`, async ({ page, baseURL }) => {
    const state = await fixture(page, baseURL!); state.status = status;
    await page.setViewportSize({ width: 320, height: 844 }); await page.goto(`/mcp/upload#upload=${uploadId}`);
    await choose(page); await page.getByRole("button", { name: "Upload file", exact: true }).click();
    await expect(page.getByRole("alert")).toBeFocused();
    await expect(page.getByRole("alert")).toContainText(status === 400 ? "not accepted" : "Return to your chat");
    await expect(page.getByLabel("File", { exact: true })).toHaveCount(0); expect(state.calls).toBe(1); await privateMemory(page);
    await page.evaluate(() => { document.documentElement.style.fontSize = "200%"; });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  });
}
test("oversized selections stay local, and the narrow picker remains accessible", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.setViewportSize({ width: 320, height: 844 });
  await page.goto(`/mcp/upload#upload=${uploadId}`);
  await choose(page, Buffer.alloc(8 * 1024 * 1024 + 1));
  await page.getByRole("button", { name: "Upload file", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("up to 8 MiB"); expect(state.calls).toBe(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

for (const [name, mimeType] of [["fixture.gif", "image/gif"], ["fixture.wav", "audio/wav"], ["fixture.ogg", "audio/ogg"], ["fixture.pdf", "application/pdf"]]) {
  test(`expanded picker sends ${name} through the existing binary upload`, async ({ page, baseURL }) => {
    const state = await fixture(page, baseURL!); await page.goto(`/mcp/upload#upload=${uploadId}`);
    const picker = page.getByLabel("File", { exact: true });
    await expect(page.getByText("Choose one image, audio file or PDF up to 8 MiB.", { exact: true })).toBeVisible();
    for (const extension of [".gif", ".webp", ".avif", ".heic", ".heif", ".wav", ".flac", ".aac", ".m4a", ".aiff", ".aifc", ".caf", ".ogg", ".oga", ".opus", ".pdf"]) {
      expect((await picker.getAttribute("accept"))!.split(",")).toContain(extension);
    }
    await picker.setInputFiles({ name: name!, mimeType: mimeType!, buffer: Buffer.from("synthetic file payload") });
    await page.getByRole("button", { name: "Upload file", exact: true }).click();
    await expect(page.getByRole("status").filter({ hasText: "File uploaded. Return to your chat" })).toBeVisible();
    expect(state.calls).toBe(1); await privateMemory(page);
  });
}
