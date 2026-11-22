// SPDX-License-Identifier: MIT
// UI-only synthetic responses on the ordinary compiled HTTPS browser fixture.
// Backend OAuth binding, expiry and grant authority need separate real-server evidence.
import { writeFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import type { MCPAccess, MCPGrant } from "../../src/api";

const request = "r".repeat(43), password = " Synthetic consent password ";
const client = "Synthetic collection client", callbackHost = "callback.test";
const firstGrant: MCPGrant = { id: "synthetic-grant-one", client_name: client, access: "read", created_at: "2026-11-19T09:00:00Z" };
const secondGrant: MCPGrant = { id: "synthetic-grant-two", client_name: "Synthetic editing client", access: "read_write", created_at: "2026-11-19T10:00:00Z" };
const otherAccount = { id: "synthetic-other-account", username: "synthetic.other" };
const otherGrant: MCPGrant = { ...firstGrant, id: "synthetic-other-grant", client_name: "Other account client" };

function barrier() {
  let release!: () => void;
  const promise = new Promise<void>(resolve => { release = resolve; });
  return { promise, release };
}

async function synthetic(page: Page, origin: string, signedIn = true) {
  const state = {
    account: { id: "synthetic-account", username: "synthetic.member" }, identities: 0, lists: 0,
    session: signedIn, options: ["read", "read_write"] as MCPAccess[], grants: [firstGrant, secondGrant],
    readError: 0, decideError: 0, listError: 0, listAccount: null as string | null, revokeError: 0, loseDecisionResponse: false,
    previewError: 0, previewName: client as unknown, previews: [] as object[], previewResponses: 0, pausePreview: null as Promise<void> | null,
    reads: [] as object[], decisions: [] as { request: string; decision: string }[], revocations: [] as string[],
    pauseDecision: null as Promise<void> | null, pauseRevoke: null as Promise<void> | null,
    pauseRead: null as Promise<void> | null, pauseList: null as Promise<void> | null, pauseIdentity: null as Promise<void> | null,
    bootstrapSawFragment: false,
  };
  let boundAccount: string | null = null;
  // A completion marker after JSON consumption and the application's promise callbacks.
  // Held stale responses can then be checked without timing sleeps or an early negative assertion.
  await page.addInitScript(() => {
    const original = window.fetch.bind(window);
    window.fetch = async (...args) => {
      const response = await original(...args), json = response.json.bind(response);
      const path = new URL(response.url).pathname;
      response.json = async () => {
        try { return await json(); }
        finally {
          setTimeout(() => {
            const root = document.documentElement;
            const consumed = JSON.parse(root.dataset.apiConsumed ?? "{}") as Record<string, number>;
            consumed[path] = (consumed[path] ?? 0) + 1; root.dataset.apiConsumed = JSON.stringify(consumed);
          }, 0);
        }
      };
      return response;
    };
  });
  if (signedIn) await page.context().addCookies([{ name: "__Host-meguri-session", value: "synthetic-session", url: origin, secure: true, httpOnly: true, sameSite: "Strict" }]);
  // Reuse the real compiled document while the separately authored backend adds its route.
  await page.route("**/oauth/consent*", async route => {
    const response = await route.fetch({ url: `${origin}/login` });
    await route.fulfill({ response });
  });
  await page.route(`https://${callbackHost}/**`, route => route.fulfill({ contentType: "text/plain", body: "Synthetic client received the callback." }));
  await page.route("**/api/**", async route => {
    const req = route.request(), path = new URL(req.url()).pathname;
    const body = req.method() === "POST" ? req.postDataJSON() : undefined;
    const data = (value: unknown) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ data: value }) });
    const error = (status: number, code = status === 401 ? "authentication_failed" : status === 410 ? "capability_invalid" : "internal_error") => route.fulfill({ status, contentType: "application/json", body: JSON.stringify({ error: { code } }) });
    if (path === "/api/status") {
      state.bootstrapSawFragment ||= await page.evaluate(() => location.hash !== "");
      return data({ setup_required: false });
    }
    if (path === "/api/login") {
      expect(body).toEqual({ username: "synthetic.member", password });
      state.session = true;
      return route.fulfill({ status: 200, contentType: "application/json", headers: { "Set-Cookie": "__Host-meguri-session=synthetic-session; Path=/; Secure; HttpOnly; SameSite=Strict" }, body: JSON.stringify({ data: { ...state.account, role: "member" } }) });
    }
    if (path === "/api/mcp/authorization/preview") {
      state.previews.push(body); expect(body).toEqual({ request });
      const name = state.previewName, status = state.previewError;
      await state.pausePreview;
      try { await (status ? error(status) : data({ client_name: name })); }
      finally { state.previewResponses++; }
      return;
    }
    if (!state.session) return error(401);
    expect(req.headers().cookie).toContain("__Host-meguri-session=synthetic-session");
    if (path === "/api/me") {
      state.identities++; if (state.identities > 1) await state.pauseIdentity;
      return data({ ...state.account, role: "member", server_url: origin });
    }
    if (path === "/api/mcp/authorization/read") {
      state.reads.push(body); expect(body).toEqual({ request }); await state.pauseRead;
      if (state.readError) return error(state.readError);
      if (boundAccount && boundAccount !== state.account.id) return error(403, "capability_invalid");
      boundAccount = state.account.id;
      return data({ username: state.account.username, client_name: client, redirect_host: callbackHost, access_options: state.options });
    }
    if (path === "/api/mcp/authorization/decide") {
      state.decisions.push(body); await state.pauseDecision;
      if (state.loseDecisionResponse) return route.abort("connectionfailed");
      if (state.decideError) return error(state.decideError);
      if (boundAccount !== state.account.id) return error(403, "capability_invalid");
      return data({ redirect_to: `https://${callbackHost}/complete?${body.decision === "deny" ? "error=access_denied" : "code=synthetic-code"}&state=synthetic-state` });
    }
    if (path === "/api/me/mcp-grants") {
      state.lists++;
      const result = { account_id: state.listAccount ?? state.account.id, grants: [...state.grants] };
      await state.pauseList;
      if (state.listError) return error(state.listError);
      return data(result);
    }
    const match = /^\/api\/me\/mcp-grants\/([^/]+)\/revoke$/.exec(path);
    if (match) {
      const account = state.account.id;
      expect(body).toEqual({}); state.revocations.push(match[1]!); await state.pauseRevoke;
      if (state.revokeError) return error(state.revokeError);
      if (account === state.account.id) state.grants = state.grants.filter(grant => grant.id !== match[1]);
      return data({ account_id: account });
    }
    // Unlisted APIs never reach accounts or providers in the actual service.
    throw new Error(`Unexpected synthetic API: ${path}`);
  });
  return state;
}

async function entry(page: Page, fragment = `request=${request}`) {
  await page.goto(`/oauth/consent${fragment ? `#${fragment}` : ""}`);
  await expect(page.getByRole("heading", { name: /^(Connect a client|Sign in)$/ })).toBeVisible();
  expect(new URL(page.url()).hash).toBe("");
}
async function privateMemory(page: Page) {
  expect(await page.evaluate(() => ({ local: localStorage.length, session: sessionStorage.length, history: history.state, url: location.href })))
    .toEqual({ local: 0, session: 0, history: null, url: page.url() });
  expect(await page.locator("body").innerText()).not.toContain(request);
}
async function audit(page: Page, name: string) {
  const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
  writeFileSync(`/out/mcp-${name}-axe.json`, JSON.stringify(result, null, 2));
  expect(result.violations).toEqual([]);
  await page.screenshot({ path: `/out/mcp-${name}.png`, fullPage: true });
}
async function consumed(page: Page, path: string, count: number) {
  await expect.poll(() => page.evaluate(path => (JSON.parse(document.documentElement.dataset.apiConsumed ?? "{}") as Record<string, number>)[path] ?? 0, path)).toBe(count);
}

test("signed-out consent resumes after CredentialsForm login and cookie identity lookup", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!, false);
  await entry(page);
  await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  await expect(page.getByText(client, { exact: true })).toBeVisible();
  await expect(page.locator('[data-slot="card"]')).toHaveCount(1);
  expect(state.previews).toEqual([{ request }]);
  expect(state.reads).toEqual([]); expect(state.bootstrapSawFragment).toBe(false);
  await privateMemory(page);
  await audit(page, "consent-signed-out");
  await page.getByLabel("Username", { exact: true }).fill(" Synthetic.Member ");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("radio", { name: "Read collection", exact: true })).toBeChecked();
  await expect(page.getByRole("heading", { name: "Connect a client", exact: true })).toBeFocused();
  await expect(page.getByText("synthetic.member", { exact: true })).toBeVisible();
  await expect(page.getByText(callbackHost, { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toHaveCount(0);
  expect(state.reads).toEqual([{ request }]); expect(state.decisions).toEqual([]);
  await audit(page, "consent-ready");
});

for (const missing of [undefined, null, "", "   ", 42, "offline"] as const) {
  test(`signed-out client preview fallback (${String(missing)}) never uses browser text`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!, false);
    state.previewName = missing; if (missing === "offline") state.previewError = 500;
    await page.goto(`/oauth/consent?client_name=UnverifiedQueryName#request=${request}`, { referer: "https://unverified-referrer.test/" });
    await consumed(page, "/api/mcp/authorization/preview", 1);
    await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
    await page.getByLabel("Username", { exact: true }).fill("unverified.input");
    await expect(page.getByText("Sign in to continue connecting your client.", { exact: true })).toBeVisible();
    await expect(page.locator('[data-slot="card-description"]')).not.toContainText(/UnverifiedQueryName|unverified/);
    await expect(page.locator('[data-slot="card"]')).toHaveCount(1);
    expect(state.reads).toEqual([]); expect(state.decisions).toEqual([]);
    await privateMemory(page);
  });
}

for (const status of [403, 410]) {
  test(`signed-out unknown or expired preview (${status}) requires restart`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!, false); state.previewError = status;
    await entry(page);
    await expect(page.getByRole("alert")).toContainText("Return to your client");
    await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toHaveCount(0);
    await expect(page.getByText(client, { exact: true })).toHaveCount(0);
    expect(state.reads).toEqual([]); expect(state.decisions).toEqual([]);
    if (status === 410) await audit(page, "consent-signed-out-expired");
  });
}

for (const status of [200, 410]) {
  test(`late anonymous preview (${status}) cannot replace authenticated consent`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!, false), preview = barrier();
    state.previewName = "Earlier preview client"; state.previewError = status; state.pausePreview = preview.promise;
    await entry(page); await expect.poll(() => state.previews.length).toBe(1);
    await page.getByLabel("Username", { exact: true }).fill("synthetic.member");
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(page.getByRole("radio", { name: "Read collection", exact: true })).toBeChecked();
    preview.release();
    await expect.poll(() => state.previewResponses).toBe(1);
    await expect(page.getByText(client, { exact: true })).toBeVisible();
    await expect(page.getByText("Earlier preview client", { exact: true })).toHaveCount(0);
    await expect(page.getByText("synthetic.member", { exact: true })).toBeVisible();
    await expect(page.getByText(callbackHost, { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Allow", exact: true })).toBeVisible();
    await expect(page.getByRole("alert")).toHaveCount(0);
  });
}

test("permission descriptions select the whole labeled row without replacing native radios", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!); await entry(page);
  await page.getByText("Read your collection, add and edit notes, and attach images and audio.", { exact: true }).click();
  await expect(page.getByRole("radio", { name: "Read and edit collection", exact: true })).toBeChecked();
  await page.getByText("Find notes and see decks and note types.", { exact: true }).click();
  await expect(page.getByRole("radio", { name: "Read collection", exact: true })).toBeChecked();
  for (const row of await page.locator(".consent-option").all()) {
    const bounds = await row.boundingBox(); expect(bounds!.height).toBeGreaterThanOrEqual(44);
  }
  expect(state.previews).toEqual([]); expect(state.decisions).toEqual([]);
});

test("consent identifies only the session bound by the authorized read, never cached Me", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!), read = barrier(); state.pauseRead = read.promise;
  await entry(page);
  await expect.poll(() => state.reads.length).toBe(1);
  await expect(page.getByText("synthetic.member", { exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Allow", exact: true })).toHaveCount(0);
  // The cached bootstrap identity is A; the authorization read binds the new session B.
  state.account = otherAccount; read.release();
  await expect(page.getByRole("radio", { name: "Read collection", exact: true })).toBeChecked();
  await expect(page.getByText(otherAccount.username, { exact: true })).toBeVisible();
  await expect(page.getByText("synthetic.member", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Allow", exact: true }).click();
  await expect(page).toHaveURL(`https://${callbackHost}/complete?code=synthetic-code&state=synthetic-state`);
  expect(state.decisions).toEqual([{ request, decision: "read" }]);
});

test("a consent read without its bound username cannot offer approval", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!);
  await page.route("**/api/mcp/authorization/read", route => route.fulfill({ contentType: "application/json", body: JSON.stringify({ data: { client_name: client, redirect_host: callbackHost, access_options: ["read"] } }) }));
  await entry(page);
  await expect(page.getByRole("alert")).toContainText("No decision was sent");
  await expect(page.getByRole("button", { name: "Allow", exact: true })).toHaveCount(0);
  await expect(page.getByText("synthetic.member", { exact: true })).toHaveCount(0);
  expect(state.decisions).toEqual([]);
});

for (const decision of ["read", "deny"] as const) {
  test(`session switch before ${decision} response clears bound consent`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!), response = barrier(); state.pauseDecision = response.promise;
    await entry(page);
    await expect(page.getByText("synthetic.member", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: decision === "deny" ? "Deny" : "Allow", exact: true }).click();
    await expect.poll(() => state.decisions.length).toBe(1);
    state.account = otherAccount; response.release();
    await consumed(page, "/api/mcp/authorization/decide", 1);
    await expect(page.getByRole("alert")).toContainText("Return to your client");
    await expect(page.getByRole("button", { name: "Allow", exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Deny", exact: true })).toHaveCount(0);
    await expect(page.getByText("synthetic.member", { exact: true })).toHaveCount(0);
    await expect(page.getByText(otherAccount.username, { exact: true })).toHaveCount(0);
    await expect(page).toHaveURL(/\/oauth\/consent$/);
    await privateMemory(page); await page.reload();
    await expect(page.getByRole("alert")).toContainText("Reloading this page cannot restore it");
    expect(state.reads).toHaveLength(1); expect(state.decisions).toEqual([{ request, decision }]);
  });
}

for (const decision of ["read", "read_write", "deny"] as const) {
  test(`signed-in consent ${decision} sends one exact decision and follows server callback`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!);
    let resume!: () => void;
    state.pauseDecision = new Promise<void>(resolve => { resume = resolve; });
    await entry(page);
    await expect(page.getByRole("radio", { name: "Read collection", exact: true })).toBeChecked();
    expect(state.decisions).toEqual([]); expect(state.bootstrapSawFragment).toBe(false);
    if (decision === "read_write") {
      await page.getByRole("radio", { name: "Read collection", exact: true }).focus();
      await page.keyboard.press("ArrowDown");
      await expect(page.getByRole("radio", { name: "Read and edit collection", exact: true })).toBeChecked();
      await audit(page, "consent-read-write");
    }
    const action = page.getByRole("button", { name: decision === "deny" ? "Deny" : "Allow", exact: true });
    await action.focus(); await page.keyboard.press("Enter");
    await expect(page.getByRole("status").filter({ hasText: "Submitting decision…" })).toBeVisible();
    await expect.poll(() => state.decisions.length).toBe(1);
    await page.getByRole("button", { name: "Deny", exact: true }).evaluate(button => { (button as HTMLButtonElement).click(); });
    expect(state.decisions).toEqual([{ request, decision }]);
    resume();
    await expect(page).toHaveURL(`https://${callbackHost}/complete?${decision === "deny" ? "error=access_denied" : "code=synthetic-code"}&state=synthetic-state`);
    expect(state.decisions).toHaveLength(1);
  });
}

test("read-only validated requests expose no edit choice; narrow consent reflows", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!); state.options = ["read"];
  await page.setViewportSize({ width: 320, height: 844 }); await entry(page);
  await expect(page.getByRole("radio", { name: "Read collection", exact: true })).toBeChecked();
  await expect(page.getByRole("radio", { name: "Read and edit collection", exact: true })).toHaveCount(0);
  await audit(page, "consent-narrow-default");
  await page.evaluate(() => { document.documentElement.style.fontSize = "200%"; });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await audit(page, "consent-narrow");
});

for (const fragment of ["", "request=bad", `request=${request}&request=${request}`]) {
  test(`missing or malformed consent fragment (${fragment ? fragment.slice(0, 16) : "bare"}) cannot submit`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!); await entry(page, fragment);
    await expect(page.getByRole("alert")).toContainText("Return to your client");
    await expect(page.getByRole("button", { name: "Allow", exact: true })).toHaveCount(0);
    await expect(page.getByRole("alert")).toBeFocused();
    expect(state.reads).toEqual([]); expect(state.decisions).toEqual([]); await privateMemory(page);
    if (!fragment) await audit(page, "consent-invalid");
  });
}

test("refresh discards the handle and cannot issue another read or decision", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!); await entry(page);
  await expect(page.getByRole("button", { name: "Allow", exact: true })).toBeVisible();
  await privateMemory(page); await page.reload();
  await expect(page.getByRole("alert")).toContainText("Reloading this page cannot restore it");
  expect(state.reads).toHaveLength(1); expect(state.decisions).toEqual([]);
});

for (const status of [401, 410]) {
  test(`unavailable consent read (${status}) requires returning to client`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!); state.readError = status; await entry(page);
    await expect(page.getByRole("alert")).toContainText("Return to your client");
    await expect(page.getByRole("button", { name: "Allow", exact: true })).toHaveCount(0);
    await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toHaveCount(0);
    expect(state.decisions).toEqual([]);
    if (status === 410) await audit(page, "consent-expired");
  });
}

test("failed read has an explicit reload action and no automatic decision", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!); state.readError = 500; await entry(page);
  await expect(page.getByRole("alert")).toContainText("No decision was sent");
  expect(state.reads).toHaveLength(1); state.readError = 0;
  await page.getByRole("button", { name: "Try loading request again", exact: true }).click();
  await expect(page.getByRole("button", { name: "Allow", exact: true })).toBeVisible();
  expect(state.reads).toHaveLength(2); expect(state.decisions).toEqual([]);
});

for (const failure of ["lost-session", "expired", "lost-response"] as const) {
  test(`decision ${failure} clears consent without a second submission`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!); await entry(page);
    await expect(page.getByRole("button", { name: "Allow", exact: true })).toBeVisible();
    if (failure === "lost-response") state.loseDecisionResponse = true;
    else state.decideError = failure === "lost-session" ? 401 : 410;
    await page.getByRole("button", { name: "Allow", exact: true }).click();
    await expect(page.getByRole("alert")).toBeFocused();
    await expect(page.getByRole("alert")).toContainText(failure === "lost-session" ? "Return to your client" : "Check your client");
    await expect(page.getByRole("button", { name: "Allow", exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Deny", exact: true })).toHaveCount(0);
    expect(state.decisions).toHaveLength(1); await privateMemory(page);
  });
}

test("Connection lists and copies MCP details, confirms or cancels revocation, and restores focus", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!); await page.goto("/connection");
  const section = page.getByRole("region", { name: "MCP connections", exact: true });
  await expect(section.getByLabel("MCP URL", { exact: true })).toHaveValue(`${baseURL}/mcp`);
  await expect(section.getByRole("button", { name: "Refresh connections", exact: true })).toHaveCount(0);
  await section.getByRole("button", { name: "Copy MCP URL", exact: true }).click();
  await expect(section.getByRole("status").filter({ hasText: /Copied\.|text is selected/ })).toBeVisible();
  const row = section.getByRole("listitem", { name: `Connection ${client}`, exact: true });
  await expect(row.getByText("Read collection", { exact: true })).toBeVisible();
  await expect(row.locator("time")).toHaveAttribute("datetime", firstGrant.created_at);
  await audit(page, "connection-desktop");
  const revoke = row.getByRole("button", { name: `Revoke ${client}`, exact: true });
  await revoke.focus(); await page.keyboard.press("Enter");
  const confirmation = row.getByRole("region", { name: `Confirm revoking ${client}`, exact: true });
  await expect(confirmation).toContainText("read collection permission");
  await expect(confirmation.getByRole("button", { name: "Cancel", exact: true })).toBeFocused();
  await audit(page, "revoke-confirmation");
  await page.keyboard.press("Escape"); await expect(revoke).toBeFocused(); expect(state.revocations).toEqual([]);
  await revoke.click(); await confirmation.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(revoke).toBeFocused(); expect(state.revocations).toEqual([]);
  await revoke.click(); await confirmation.getByRole("button", { name: `Confirm revoke ${client}`, exact: true }).click();
  await expect(row).toHaveCount(0);
  await expect(section.getByRole("button", { name: `Revoke ${secondGrant.client_name}`, exact: true })).toBeFocused();
  await expect(section.getByRole("status").filter({ hasText: `Revoked ${client}.` })).toBeVisible();
  await audit(page, "connection-revoked");
  expect(state.revocations).toEqual([firstGrant.id]);
  await page.setViewportSize({ width: 320, height: 844 });
  await section.getByRole("button", { name: `Revoke ${secondGrant.client_name}`, exact: true }).click();
  await expect(section.getByRole("region", { name: `Confirm revoking ${secondGrant.client_name}`, exact: true })).toContainText("read and edit collection permission");
  await page.evaluate(() => { document.documentElement.style.fontSize = "200%"; });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await audit(page, "revoke-narrow");
  await section.getByRole("button", { name: `Confirm revoke ${secondGrant.client_name}`, exact: true }).click();
  await consumed(page, `/api/me/mcp-grants/${secondGrant.id}/revoke`, 1);
  await expect(section.getByRole("status").filter({ hasText: `Revoked ${secondGrant.client_name}.` })).toBeVisible();
  await expect(section.getByRole("listitem")).toHaveCount(0);
  await expect(section.getByRole("list")).toHaveCount(0);
  await expect(section).toBeFocused();
  await page.reload(); await consumed(page, "/api/me/mcp-grants", 1);
  await expect(section.getByText("Loading MCP connections…", { exact: true })).toHaveCount(0);
  await expect(section.getByText("No MCP connections", { exact: true })).toHaveCount(0);
  await expect(section.getByRole("listitem")).toHaveCount(0);
  await expect(section.getByRole("list")).toHaveCount(0);
});

for (const width of [320, 390]) {
  test(`Connection grant actions fit beside details at ${width}px and wrap with larger text`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!);
    await page.setViewportSize({ width, height: 844 }); await page.goto("/connection");
    const row = page.getByRole("listitem", { name: `Connection ${client}`, exact: true });
    const action = row.getByRole("button", { name: `Revoke ${client}`, exact: true });
    await expect(action).toBeVisible();
    const details = (await row.locator(".grant-info").boundingBox())!, bounds = (await action.boundingBox())!;
    expect(bounds.x).toBeGreaterThanOrEqual(details.x + details.width);
    expect(bounds.y).toBeLessThan(details.y + details.height);
    expect(bounds.width).toBeGreaterThanOrEqual(44); expect(bounds.height).toBeGreaterThanOrEqual(44);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await audit(page, `connection-${width}`);
    state.grants = [{ ...firstGrant, client_name: "Synthetic client with a much longer name for collection research and editing" }];
    await page.reload();
    await expect(page.getByRole("listitem", { name: `Connection ${state.grants[0]!.client_name}`, exact: true })).toBeVisible();
    await page.evaluate(() => { document.documentElement.style.fontSize = "200%"; });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await audit(page, `connection-${width}-large-text`);
  });
}

test("Connection load failure can recover on reload; failed and pending revokes retain controls and never duplicate", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!); state.listError = 500; await page.goto("/connection");
  const section = page.getByRole("region", { name: "MCP connections", exact: true });
  await expect(section.getByRole("alert")).toBeFocused(); state.listError = 0;
  await page.reload();
  await section.getByRole("button", { name: `Revoke ${client}`, exact: true }).click();
  const confirm = section.getByRole("button", { name: `Confirm revoke ${client}`, exact: true });
  state.revokeError = 500;
  let resume!: () => void; state.pauseRevoke = new Promise<void>(resolve => { resume = resolve; });
  await confirm.click();
  await expect(section.getByRole("status").filter({ hasText: `Revoking ${client}…` })).toBeVisible();
  await expect.poll(() => state.revocations.length).toBe(1);
  await section.getByRole("button", { name: "Revoking…", exact: true }).evaluate(button => { (button as HTMLButtonElement).click(); });
  resume();
  await expect(section.getByRole("alert")).toContainText("Revocation is not confirmed");
  await expect(section.getByRole("alert")).toBeFocused();
  await expect(confirm).toBeVisible(); expect(state.revocations).toEqual([firstGrant.id]);
  await section.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(section.getByRole("button", { name: `Revoke ${client}`, exact: true })).toBeFocused();
});

test("a grant list for another session stays hidden and refreshes screen identity", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!), identity = barrier();
  state.pauseIdentity = identity.promise;
  state.listAccount = otherAccount.id; state.grants = [otherGrant];
  await page.goto("/connection");
  const section = page.getByRole("region", { name: "MCP connections", exact: true });
  await expect.poll(() => state.identities).toBe(3);
  await consumed(page, "/api/me/mcp-grants", 1);
  await expect(page.getByLabel("Username", { exact: true })).toHaveValue("synthetic.member");
  await expect(section.getByRole("listitem")).toHaveCount(0);
  await expect(section).not.toContainText(otherGrant.client_name);
  await expect(section.getByRole("alert")).toContainText("Your account changed");
  state.account = otherAccount; state.listAccount = null; identity.release();
  await expect(page.getByLabel("Username", { exact: true })).toHaveValue(otherAccount.username);
  await expect(section.getByRole("listitem", { name: `Connection ${otherGrant.client_name}`, exact: true })).toBeVisible();
  await expect(section.getByRole("listitem", { name: `Connection ${client}`, exact: true })).toHaveCount(0);
  expect(state.lists).toBe(2);
});

test("an absent-target revoke by a different caller clears confirmation without announcing success", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!); await page.goto("/connection");
  const section = page.getByRole("region", { name: "MCP connections", exact: true });
  await section.getByRole("button", { name: `Revoke ${client}`, exact: true }).click();
  await consumed(page, "/api/me", 2);
  const identity = barrier(); state.pauseIdentity = identity.promise;
  state.account = otherAccount; state.grants = [otherGrant];
  await section.getByRole("button", { name: `Confirm revoke ${client}`, exact: true }).click();
  await expect.poll(() => state.identities).toBe(3);
  await consumed(page, `/api/me/mcp-grants/${firstGrant.id}/revoke`, 1);
  await expect(page.getByLabel("Username", { exact: true })).toHaveValue("synthetic.member");
  await expect(section.getByRole("listitem")).toHaveCount(0);
  await expect(section.getByRole("region", { name: `Confirm revoking ${client}`, exact: true })).toHaveCount(0);
  await expect(section).not.toContainText(`Revoked ${client}.`);
  await expect(section).not.toContainText(`Revoking ${client}…`);
  await expect(section.getByRole("alert")).toContainText("Your account changed");
  identity.release();
  await expect(page.getByLabel("Username", { exact: true })).toHaveValue(otherAccount.username);
  await expect(section.getByRole("button", { name: `Revoke ${otherGrant.client_name}`, exact: true })).toBeVisible();
  expect(state.grants).toEqual([otherGrant]); expect(state.revocations).toEqual([firstGrant.id]);
  await expect(section).not.toContainText(`Revoked ${client}.`);
});

for (const status of [200, 401]) {
  test(`identity change fences a late grant-list completion (${status})`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!), identity = barrier(), list = barrier();
    // Only this test's initial grant fetch ignores transport cancellation, so its
    // late success/401 reaches the unchanged screen's account and abort fences.
    await page.addInitScript(() => {
      const original = window.fetch.bind(window);
      let initialList = true;
      window.fetch = (input, init) => {
        if (initialList && input === "/api/me/mcp-grants") {
          initialList = false;
          return original(input, { ...init, signal: null });
        }
        return original(input, init);
      };
    });
    state.pauseIdentity = identity.promise; state.pauseList = list.promise; await page.goto("/connection");
    const section = page.getByRole("region", { name: "MCP connections", exact: true });
    await expect(section.getByText("Loading MCP connections…", { exact: true })).toBeVisible();
    await expect.poll(() => state.identities).toBe(2);
    await expect.poll(() => state.lists).toBe(1);
    state.account = otherAccount; state.grants = [otherGrant]; state.pauseList = null; identity.release();
    await expect(page.getByLabel("Username", { exact: true })).toHaveValue(otherAccount.username);
    await expect(section.getByRole("button", { name: `Revoke ${otherGrant.client_name}`, exact: true })).toBeVisible();
    await consumed(page, "/api/me/mcp-grants", 1);
    state.listError = status === 200 ? 0 : status; list.release();
    await consumed(page, "/api/me/mcp-grants", 2);
    await expect(page.getByLabel("Username", { exact: true })).toHaveValue(otherAccount.username);
    await expect(section.getByRole("listitem")).toHaveCount(1);
    await expect(section.getByRole("button", { name: `Revoke ${otherGrant.client_name}`, exact: true })).toBeVisible();
    await expect(section).not.toContainText(client);
    await expect(section.getByRole("alert")).toHaveCount(0);
  });

  test(`identity change clears pending confirmation and fences a late revoke (${status})`, async ({ page, baseURL }) => {
    const state = await synthetic(page, baseURL!), identity = barrier(), revoke = barrier();
    state.pauseIdentity = identity.promise; await page.goto("/connection");
    const section = page.getByRole("region", { name: "MCP connections", exact: true });
    await section.getByRole("button", { name: `Revoke ${client}`, exact: true }).click();
    await expect.poll(() => state.identities).toBe(2);
    state.pauseRevoke = revoke.promise;
    await section.getByRole("button", { name: `Confirm revoke ${client}`, exact: true }).click();
    await expect.poll(() => state.revocations.length).toBe(1);
    // Reusing a grant ID also catches retained row confirmation state.
    state.account = otherAccount; state.grants = [{ ...otherGrant, id: firstGrant.id }]; identity.release();
    await expect(page.getByLabel("Username", { exact: true })).toHaveValue(otherAccount.username);
    await expect(section.getByRole("button", { name: `Revoke ${otherGrant.client_name}`, exact: true })).toBeVisible();
    await expect(section.getByRole("region")).toHaveCount(0);
    await expect(section.getByRole("button", { name: `Revoke ${otherGrant.client_name}`, exact: true })).toHaveAttribute("aria-disabled", "false");
    state.revokeError = status === 200 ? 0 : status; revoke.release();
    await consumed(page, `/api/me/mcp-grants/${firstGrant.id}/revoke`, 1);
    await expect(page.getByLabel("Username", { exact: true })).toHaveValue(otherAccount.username);
    await expect(section.getByRole("button", { name: `Revoke ${otherGrant.client_name}`, exact: true })).toBeVisible();
    await expect(section.getByRole("region")).toHaveCount(0);
    await expect(section).not.toContainText(`Revoked ${client}.`);
    await expect(section.getByRole("alert")).toHaveCount(0);
  });
}

test("lost Connection session returns to sign in without keeping grants", async ({ page, baseURL }) => {
  const state = await synthetic(page, baseURL!); await page.goto("/connection");
  await page.getByRole("button", { name: `Revoke ${client}`, exact: true }).click();
  state.revokeError = 401;
  await page.getByRole("button", { name: `Confirm revoke ${client}`, exact: true }).click();
  await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  await expect(page.getByRole("status").filter({ hasText: "Your session is no longer available" })).toBeVisible();
  await expect(page.getByRole("region", { name: "MCP connections", exact: true })).toHaveCount(0);
});
