// SPDX-License-Identifier: MIT
// Real service only. Parent provides a fresh synthetic Rust root behind HTTPS.
import { readFileSync, writeFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import type { Page, Response } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { storageValue } from "../../src/storage";
const fixture: { setup_token: string } = JSON.parse(readFileSync("/fixture/fixture.json", "utf8"));
const initialPassword = " Synthetic initial password ";
const replacedPassword = "é".repeat(6);
const resetPassword = "Synthetic reset password 2026";
const memberUsername = "member." + "long".repeat(14) + "x"; // Maximum 64-character username.
const fontResponses = new WeakMap<Page, Response[]>();
async function loadedFont(page: Page) {
  const evidence = await page.evaluate(async () => {
    const text = "Meguri 0123456789 ilMW";
    const faces = await document.fonts.load('normal 650 16px "IBM Plex Sans"', text);
    await document.fonts.ready;
    return {
      faces: faces.map(face => ({ family: face.family.replaceAll('"', '').replaceAll("'", ''),
        status: face.status, style: face.style, weight: face.weight, display: face.display })),
      checked: document.fonts.check('normal 650 16px "IBM Plex Sans"', text),
      families: Array.from(document.querySelectorAll("body, h1, h2, h3, p, a, label, button, input"))
        .filter(element => element.getClientRects().length)
        .map(element => getComputedStyle(element).fontFamily.split(",")[0]!.trim().replaceAll('"', '').replaceAll("'", '')),
      resources: performance.getEntriesByType("resource").map(entry => entry.name)
        .filter(url => new URL(url).pathname.endsWith(".woff2")),
    };
  });
  expect(evidence.faces).toEqual([{ family: "IBM Plex Sans", status: "loaded", style: "normal", weight: "100 700", display: "swap" }]);
  expect(evidence.checked).toBe(true);
  expect(evidence.families.length).toBeGreaterThan(0);
  expect(new Set(evidence.families)).toEqual(new Set(["IBM Plex Sans"]));
  expect(evidence.resources).toHaveLength(1);
  const url = new URL(evidence.resources[0]!);
  expect(url.origin).toBe(new URL(page.url()).origin);
  expect(url.pathname).toMatch(/^\/assets\/IBMPlexSans-Variable-[A-Za-z0-9_-]{8,}\.woff2$/);
  expect(url.search + url.hash).toBe("");
  // Use the browser's actual font response, including when served from cache.
  const response = fontResponses.get(page)?.filter(item => item.url() === url.href).at(-1);
  expect(response).toBeDefined();
  expect(response!.request().resourceType()).toBe("font");
  expect(response!.status()).toBe(200);
  expect(response!.headers()["content-type"]).toBe("font/woff2");
  expect(await response!.body()).toEqual(readFileSync("src/IBMPlexSans-Variable.woff2"));
}
async function brand(page: Page) {
  const lockup = page.locator(".brand");
  await expect(page).toHaveTitle("Meguri");
  await expect(lockup).toHaveCount(1);
  await expect(lockup.getByText("Meguri", { exact: true })).toBeVisible();
  const brandLink = page.getByRole("link", { name: "Meguri", exact: true });
  if (await lockup.evaluate(element => element.classList.contains("auth-brand"))) {
    await expect(brandLink).toHaveCount(0);
    expect(await lockup.evaluate(element => element.tagName)).toBe("P");
  } else {
    await expect(brandLink).toBeVisible();
    await expect(brandLink).toHaveAttribute("href", "/connection");
    await expect(brandLink).toHaveCSS("text-decoration-line", "none");
    expect(await brandLink.evaluate(element => getComputedStyle(element).color === getComputedStyle(element.parentElement!).color)).toBe(true);
  }
  const image = lockup.locator('img[alt=""]');
  await expect(image).toBeVisible();
  await expect(image).toHaveAttribute("width", "28");
  await expect(image).toHaveAttribute("height", "28");
  await expect(image).toHaveJSProperty("complete", true);
  await expect(image).toHaveJSProperty("naturalWidth", 64);
  await expect(image).toHaveJSProperty("naturalHeight", 64);
  const src = await image.evaluate(element => (element as HTMLImageElement).currentSrc);
  const icon = page.locator('link[rel="icon"][sizes="64x64"][type="image/png"]');
  await expect(icon).toHaveCount(1);
  const href = await icon.getAttribute("href");
  expect(href).toBeTruthy();
  expect(src).toBe(new URL(href!, page.url()).href);
  expect(new URL(src).origin).toBe(new URL(page.url()).origin);
  expect(new URL(src).pathname).toMatch(/^\/assets\/favicon-64-[^/]+\.png$/);
  const response = await page.request.get(src);
  expect(response.ok()).toBe(true);
  expect(await response.body()).toEqual(readFileSync("src/favicon-64.png"));
}
async function accountShell(page: Page, username: string, admin: boolean) {
  await brand(page);
  const mobile = page.viewportSize()!.width <= 768;
  const navigation = page.getByRole("navigation", { name: "Account", exact: true });
  const names = admin ? ["Collection", "Connection", "Password", "Accounts"] : ["Collection", "Connection", "Password"];
  await expect(navigation.getByRole("link")).toHaveCount(names.length);
  await expect(navigation.getByRole("link")).toHaveText(names);
  for (const name of names) {
    const link = navigation.getByRole("link", { name, exact: true });
    await expect(link).toBeVisible();
    await expect(link.locator('svg[aria-hidden="true"][focusable="false"]')).toHaveCount(1);
    if (mobile) await expect(link.locator("svg")).toBeHidden();
    else await expect(link.locator("svg")).toBeVisible();
    const target = await link.boundingBox();
    expect(target?.width).toBeGreaterThanOrEqual(44);
    expect(target?.height).toBeGreaterThanOrEqual(44);
    await link.focus();
    await page.keyboard.press("Shift+Tab");
    await page.keyboard.press("Tab");
    await expect(link).toBeFocused();
    await expect(link).toHaveCSS("outline-style", "solid");
  }
  if (mobile) {
    const brandTarget = await page.getByRole("link", { name: "Meguri", exact: true }).boundingBox();
    expect(brandTarget?.height).toBeGreaterThanOrEqual(44);
    const links = await navigation.getByRole("link").evaluateAll(elements => elements.map(element => {
      const bounds = element.getBoundingClientRect();
      const menu = element.closest("ul")!.getBoundingClientRect();
      const label = element.querySelector("span")!.getBoundingClientRect();
      const text = document.createRange();
      text.selectNodeContents(element.querySelector("span")!);
      return { left: bounds.left, right: bounds.right, top: bounds.top, width: bounds.width,
        labelLines: text.getClientRects().length,
        menuLeft: menu.left, menuRight: menu.right,
        groupOffset: (label.left + label.right - bounds.left - bounds.right) / 2,
        fits: label.left >= bounds.left && label.right <= bounds.right
          && label.bottom <= bounds.bottom && element.scrollWidth <= element.clientWidth };
    }));
    expect(links[0]!.left).toBeCloseTo(links[0]!.menuLeft, 0);
    expect(links.at(-1)!.right).toBeCloseTo(links[0]!.menuRight, 0);
    for (const [index, link] of links.entries()) {
      expect(link.width).toBeCloseTo(links[0]!.width, 0);
      expect(link.top).toBeCloseTo(links[0]!.top, 0);
      expect(Math.abs(link.groupOffset)).toBeLessThanOrEqual(1);
      expect(link.fits).toBe(true);
      expect(link.labelLines).toBe(1);
      expect(link.width).toBeGreaterThanOrEqual(44);
      if (index) expect(link.left - links[index - 1]!.right).toBeGreaterThanOrEqual(0);
    }
    for (const link of await navigation.getByRole("link").all()) {
      await expect(link).toHaveCSS("border-bottom-width", "2px");
      if (await link.getAttribute("aria-current") === "page") {
        await expect(link).toHaveCSS("border-bottom-color", "rgb(38, 38, 38)");
        await expect(link).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
        await expect(link).toHaveCSS("font-weight", "650");
      } else {
        await expect(link).toHaveCSS("border-bottom-color", "rgba(0, 0, 0, 0)");
      }
    }
  }
  const identity = page.getByRole("group", { name: "Signed-in account", exact: true });
  await expect(identity).toHaveCount(1);
  await expect(identity.getByText(username, { exact: true })).toBeVisible();
  await expect(identity.getByText(username, { exact: true })).toHaveAttribute("title", username);
  expect(await identity.ariaSnapshot()).toContain(username);
  if (mobile || username === "root.admin") {
    expect(await identity.getByText(username, { exact: true }).evaluate(element => {
      const range = document.createRange();
      range.selectNodeContents(element);
      return new Set(Array.from(range.getClientRects(), rect => rect.top)).size;
    })).toBe(1);
  }
  if (mobile) {
    const name = identity.locator(".identity");
    await expect(name).toHaveCSS("white-space", "nowrap");
    await expect(name).toHaveCSS("overflow", "hidden");
    await expect(name).toHaveCSS("text-overflow", "ellipsis");
    if (username.length === 64) expect(await name.evaluate(element => element.scrollWidth > element.clientWidth)).toBe(true);
  } else {
    await expect(identity.locator(".identity")).toHaveCSS("white-space", "normal");
    if (username.length === 64) expect(await identity.locator(".identity").evaluate(element => {
      const text = document.createRange();
      text.selectNodeContents(element);
      return text.getClientRects().length;
    })).toBeGreaterThan(1);
  }
  await expect(identity.getByText(/^(Administrator|Member)$/)).toHaveCount(0);
  const silhouette = identity.locator(':scope > svg[aria-hidden="true"][focusable="false"]');
  if (mobile) await expect(silhouette).toBeHidden();
  else await expect(silhouette).toBeVisible();
  const signOut = identity.getByRole("button", { name: "Sign out", exact: true });
  await expect(signOut).toBeVisible();
  await expect(signOut).toBeEnabled();
  await expect(signOut).toHaveAttribute("title", "Sign out");
  await expect(signOut.locator('svg[aria-hidden="true"][focusable="false"]')).toBeVisible();
  const target = await signOut.boundingBox();
  expect(target?.width).toBeGreaterThanOrEqual(44);
  expect(target?.height).toBeGreaterThanOrEqual(44);
  await signOut.focus();
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Tab");
  await expect(signOut).toBeFocused();
  await expect(signOut).toHaveCSS("outline-style", "solid");
  // The hidden account presentation must never add a keyboard stop.
  await page.getByRole("link", { name: "Skip to content", exact: true }).focus();
  const brandLink = page.getByRole("link", { name: "Meguri", exact: true });
  await page.keyboard.press("Tab");
  await expect(brandLink).toBeFocused();
  await expect(brandLink).toHaveCSS("outline-style", "solid");
  expect(await brandLink.evaluate(element => parseFloat(getComputedStyle(element).outlineWidth))).toBeGreaterThan(0);
  const rowLinks = await navigation.getByRole("link").all();
  for (const control of mobile ? [signOut, ...rowLinks] : [...rowLinks, signOut]) {
    await page.keyboard.press("Tab");
    await expect(control).toBeFocused();
  }
  const firstMainControl = page.getByRole("main").locator('a[href], button:not(:disabled), input:not(:disabled)').first();
  if (await firstMainControl.count()) {
    await page.keyboard.press("Tab");
    await expect(firstMainControl).toBeFocused();
  }
  await page.getByRole("main").focus();
  expect(await identity.evaluate(element => {
    const sidebar = element.closest("aside")!.getBoundingClientRect();
    return [element, ...element.querySelectorAll<HTMLElement>("p, button, .footer-identity")].every(child => {
      const bounds = child.getBoundingClientRect();
      return bounds.left >= sidebar.left && bounds.right <= sidebar.right
        && (child.scrollWidth <= child.clientWidth || (child.matches(".mobile-account .identity")
          && getComputedStyle(child).textOverflow === "ellipsis" && getComputedStyle(child).overflow === "hidden"))
        && child.scrollHeight <= child.clientHeight;
    });
  })).toBe(true);
  if (mobile) {
    await expect(identity.locator(".footer-identity")).toHaveCSS("text-align", "right");
    const alignment = await identity.evaluate(element => {
      const header = element.closest('[data-slot="sidebar-header"]')!.getBoundingClientRect();
      const brand = element.parentElement!.querySelector(".brand")!.getBoundingClientRect();
      const account = element.getBoundingClientRect();
      const button = element.querySelector("button")!.getBoundingClientRect();
      const nav = element.closest("aside")!.querySelector("nav")!.getBoundingClientRect();
      return { brandLeft: brand.left - header.left, accountRight: header.right - account.right,
        buttonRight: header.right - button.right, centers: (brand.top + brand.bottom - account.top - account.bottom) / 2,
        rowGap: nav.top - header.bottom };
    });
    expect(alignment.brandLeft).toBeCloseTo(0, 0);
    expect(alignment.accountRight).toBeCloseTo(0, 0);
    expect(alignment.buttonRight).toBeCloseTo(0, 0);
    expect(Math.abs(alignment.centers)).toBeLessThanOrEqual(1);
    expect(alignment.rowGap).toBeGreaterThanOrEqual(0);
    expect(alignment.rowGap).toBeLessThanOrEqual(8);
  } else {
    const footer = await identity.boundingBox();
    const sidebar = await page.locator('[data-slot="sidebar"]').boundingBox();
    const bottom = sidebar!.y + sidebar!.height;
    expect(bottom).toBe(page.viewportSize()!.height);
    expect(bottom - footer!.y - footer!.height).toBeCloseTo(8, 0);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
}

async function connectionLayout(page: Page, username: string, admin: boolean) {
  const viewport = page.viewportSize()!;
  try {
    for (const width of [390, 320]) {
      await page.setViewportSize({ width, height: 844 });
      await accountShell(page, username, admin);
      await expect(page.getByRole("status").filter({ hasText: /^Signed in\.$/ })).toBeVisible();
      await expect(page.locator(".outcome").filter({ hasText: /^Signed in\.$/ })).toHaveCSS("background-color", "rgb(245, 247, 245)");
      await audit(page, `connection-${admin ? "admin" : "member"}-${width}`);
    }
    const main = page.getByRole("main");
    await expect(main.locator('[data-slot="card"]')).toHaveCount(1);
    await expect(main.getByRole("heading", { name: "Connection details", exact: true })).toBeVisible();
    await expect(main.getByLabel("Server URL", { exact: true })).not.toHaveValue("");
    await expect(main.getByLabel("Username", { exact: true })).toHaveValue(username);
    await expect(main.getByText("Sign in to a compatible client with your username and account password.", { exact: true })).toBeVisible();
    await expect(main.getByText("Replacing your password revokes existing client sign-ins. Sign in again on each device afterward.", { exact: true })).toBeVisible();
    await expect(main).not.toContainText(/Client guidance|Desktop 26\.09\.4|AnkiMobile 26\.10|Exact server fields|Review the client’s explicit confirmation/);
    await page.setViewportSize({ width: 1280, height: 900 });
    await accountShell(page, username, admin);
    await page.setViewportSize({ width: 320, height: 844 });
    // Text zoom wraps tab labels; only the mobile username may ellipsize.
    await page.evaluate(() => { document.documentElement.style.fontSize = "200%"; });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect(await page.locator('[data-slot="sidebar-menu-button"], [data-slot="sidebar-menu-button"] span, .mobile-account, .mobile-account p').evaluateAll(elements =>
      elements.every(element => (element.scrollWidth <= element.clientWidth || (element.matches(".mobile-account .identity")
        && getComputedStyle(element).textOverflow === "ellipsis" && getComputedStyle(element).overflow === "hidden"))
        && element.scrollHeight <= element.clientHeight))).toBe(true);
    const identity = page.locator(".mobile-account .identity");
    await expect(identity).toHaveText(username);
    await expect(identity).toHaveAttribute("title", username);
    expect(await page.locator(".mobile-account").ariaSnapshot()).toContain(username);
    expect(await page.getByRole("link", { name: "Connection", exact: true }).locator("span").evaluate(element => {
      const text = document.createRange();
      text.selectNodeContents(element);
      return text.getClientRects().length;
    })).toBeGreaterThan(1);
    for (const link of await page.getByRole("navigation", { name: "Account", exact: true }).getByRole("link").all()) {
      const target = await link.boundingBox();
      expect(target!.width).toBeGreaterThanOrEqual(44);
      expect(target!.height).toBeGreaterThanOrEqual(44);
    }
  } finally {
    await page.evaluate(() => { document.documentElement.style.removeProperty("font-size"); });
    await page.setViewportSize(viewport);
  }
  try {
    for (const width of [1280, 390, 320]) {
      await page.setViewportSize({ width, height: 900 });
      const documentTime = await page.evaluate(() => performance.timeOrigin);
      await page.getByRole("link", { name: "Collection", exact: true }).click();
      await expect(page.getByRole("heading", { name: "Collection", exact: true })).toBeVisible();
      await expect(page).toHaveURL(/\/collection$/);
      const logo = page.getByRole("link", { name: "Meguri", exact: true });
      if (width === 1280) await logo.locator("img").click();
      else { await logo.focus(); await page.keyboard.press("Enter"); }
      await expect(page).toHaveURL(/\/connection$/);
      await expect(page.getByRole("heading", { name: "Connect your devices", exact: true })).toBeVisible();
      await expect(page.getByRole("link", { name: "Connection", exact: true })).toHaveAttribute("aria-current", "page");
      await expect(page.getByRole("main")).toBeFocused();
      expect(await page.evaluate(() => performance.timeOrigin)).toBe(documentTime);
    }
  } finally { await page.setViewportSize(viewport); }
}

async function openCapability(page: Page, url: string) {
  try { await page.goto(url); } catch { throw new Error("Could not open the synthetic capability page"); }
}
async function fillPassword(page: Page, value: string) {
  await page.getByLabel("New password", { exact: true }).fill(value);
  await page.getByLabel("Confirm new password", { exact: true }).fill(value);
}
async function signIn(page: Page, username: string, password: string) {
  await page.goto("/login");
  await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  await expect(page.getByText("Use your Meguri username and password.", { exact: true })).toBeVisible();
  await expect(page.getByRole("main")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByLabel("Username", { exact: true })).toBeFocused();
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.keyboard.press("Tab");
  await expect(page.getByLabel("Password", { exact: true })).toBeFocused();
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "Sign in", exact: true })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "Connect your devices" })).toBeVisible();
}
async function audit(page: Page, name?: string) {
  await loadedFont(page);
  await brand(page);
  const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
  expect(result.violations.map(item => ({ id: item.id, impact: item.impact }))).toEqual([]);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  for (const outcome of await page.locator(".outcome:not(:empty)").all()) {
    await expect(outcome).toHaveCSS("border-top-style", "solid");
    await expect(outcome).toHaveCSS("border-top-width", "1px");
    await expect(outcome).toHaveCSS("background-color", await outcome.evaluate(element => element.classList.contains("error") ? "rgb(255, 242, 243)" : "rgb(245, 247, 245)"));
    expect(await outcome.evaluate(element => getComputedStyle(element, "::before").content)).toBe("none");
  }
  const borders = await page.locator('[data-slot="input"]').evaluateAll(inputs => {
    function rgb(color: string) { return (color.match(/[\d.]+/g) ?? []).map(Number); }
    function luminance(color: string) {
      const [r = 0, g = 0, b = 0] = rgb(color).map(value => {
        const channel = value / 255;
        return channel <= .04045 ? channel / 12.92 : ((channel + .055) / 1.055) ** 2.4;
      });
      return .2126 * r + .7152 * g + .0722 * b;
    }
    return inputs.filter(input => input.getClientRects().length).map(input => {
      let parent: Element | null = input, background = "rgb(255, 255, 255)";
      while (parent) {
        const color = getComputedStyle(parent).backgroundColor;
        if (rgb(color)[3] !== 0) { background = color; break; }
        parent = parent.parentElement;
      }
      const border = getComputedStyle(input).borderTopColor;
      const a = luminance(border), b = luminance(background);
      return { border, background, contrast: (Math.max(a, b) + .05) / (Math.min(a, b) + .05) };
    });
  });
  for (const border of borders) expect(border.contrast, JSON.stringify(border)).toBeGreaterThanOrEqual(3);
  const fontEvidence = await page.locator("h1").evaluate(heading => {
    const canvas = document.createElement("canvas").getContext("2d");
    const computedStack = getComputedStyle(heading).fontFamily;
    const canvasWidths = [computedStack, "system-ui", "ui-sans-serif", "sans-serif", "monospace"].map(family => {
      if (!canvas) return { family, width: null };
      canvas.font = `16px ${family}`;
      return { family, width: canvas.measureText("Meguri 0123456789 ilMW").width };
    });
    return { computedStack, canvasWidths };
  });
  let platformFonts: unknown = "CDP unavailable";
  try {
    const session = await page.context().newCDPSession(page);
    try {
      await session.send("DOM.enable"); await session.send("CSS.enable");
      const { root } = await session.send("DOM.getDocument");
      const { nodeId } = await session.send("DOM.querySelector", { nodeId: root.nodeId, selector: "h1" });
      platformFonts = (await session.send("CSS.getPlatformFontsForNode", { nodeId })).fonts;
    } finally { await session.detach(); }
  } catch { /* Canvas metrics above remain available on engines without CDP. */ }
  if (name) writeFileSync(`/out/browser-artifacts/${name}-fonts.json`, JSON.stringify({ ...fontEvidence, platformFonts, borders }, null, 2) + "\n");
  await page.evaluate(() => {
    if (document.activeElement?.matches(".skip-link")) document.querySelector<HTMLElement>("main")?.focus({ preventScroll: true });
    window.scrollTo({ top: 0, left: 0, behavior: "instant" });
  });
  if (name) await page.screenshot({ path: `/out/browser-artifacts/${name}.png`, fullPage: true });
}
test("setup, invitation, member settings, reset and admin actions over private HTTPS", async ({ page, browser, baseURL }) => {
  if (!baseURL) throw new Error("Missing HTTPS fixture origin");
  const desktopViewport = page.viewportSize();
  if (!desktopViewport) throw new Error("Missing desktop viewport");
  const external: string[] = [];
  let bootstrapSawFragment = false;
  function observe(target: Page) {
    const fonts: Response[] = [];
    fontResponses.set(target, fonts);
    target.on("response", response => {
      if (response.request().resourceType() === "font") fonts.push(response);
    });
    target.on("request", request => {
      const url = new URL(request.url());
      if (url.origin !== baseURL) external.push(url.origin);
      if (url.pathname.startsWith("/api/") && target.url().includes("#")) bootstrapSawFragment = true;
    });
  }
  observe(page);
  await openCapability(page, `/setup#token=${fixture.setup_token}`);
  await expect(page.getByRole("heading", { name: "Set up Meguri" })).toBeVisible();
  await expect(page.getByLabel("One-time setup code", { exact: true })).toHaveAttribute("autocomplete", "off");
  await expect(page.getByText("Find the code in the server’s first-start console or Docker logs, or in the protected state/setup-token file.", { exact: true })).toBeVisible();
  await expect(page.getByText("Use a long password or passphrase. Spaces and letter case are preserved.", { exact: true })).toBeVisible();
  expect(new URL(page.url()).hash).toBe("");
  await audit(page, "setup-wide");
  await page.getByLabel("Username", { exact: true }).fill("Root.Admin");
  await fillPassword(page, initialPassword);
  await page.getByLabel("Confirm new password", { exact: true }).press("Tab");
  await expect(page.getByRole("button", { name: "Set up Meguri", exact: true })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  await audit(page);
  await page.goto("/login");
  await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  await expect(page.getByLabel("Username", { exact: true })).toHaveValue("");
  await expect(page.getByLabel("Password", { exact: true })).toHaveValue("");
  await audit(page, "anonymous-login-wide");
  // Hold the actual authenticated Rust response across same-document Back navigation.
  await page.goto("/reset");
  await expect(page.getByRole("heading", { name: "Reset your password", exact: true })).toBeVisible();
  await page.getByRole("link", { name: "Go to sign in", exact: true }).click();
  let releaseIdentity!: () => void, identityReady!: () => void, identityFailed!: (error: unknown) => void;
  const release = new Promise<void>(resolve => { releaseIdentity = resolve; });
  const held = new Promise<void>((resolve, reject) => { identityReady = resolve; identityFailed = reject; });
  await page.route("**/api/me", async route => {
    try {
      const response = await route.fetch({ timeout: 8_000, maxRetries: 0, maxRedirects: 0 });
      expect(response.status()).toBe(200);
      expect((await response.json()).data.username).toBe("root.admin");
      identityReady();
      await release;
      await route.fulfill({ response });
    } catch (error) { identityFailed(error); throw error; }
  });
  await page.getByLabel("Username", { exact: true }).fill("root.admin");
  await page.getByLabel("Password", { exact: true }).fill(initialPassword);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  try {
    await held;
    await page.goBack();
    await expect(page.getByRole("heading", { name: "Reset your password", exact: true })).toBeVisible();
    const completed = page.waitForResponse(response => new URL(response.url()).pathname === "/api/me", { timeout: 8_000 });
    releaseIdentity();
    expect(await (await completed).finished()).toBeNull();
    // Flush rendering after the real fetch body has completed, with no timer delay.
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    await expect(page).toHaveURL(`${baseURL}/reset`);
    await expect(page.getByRole("heading", { name: "Reset your password", exact: true })).toBeVisible();
  } finally { releaseIdentity(); await page.unroute("**/api/me"); }
  const logout = await page.context().request.post(`${baseURL}/api/logout`, { data: {}, headers: { Origin: baseURL } });
  expect(logout.ok()).toBe(true);
  await signIn(page, "root.admin", initialPassword);
  await accountShell(page, "root.admin", true);
  await connectionLayout(page, "root.admin", true);
  await audit(page, "connection-wide");
  await page.getByRole("link", { name: "Accounts", exact: true }).click();
  await expect(page.getByRole("heading", { name: "root.admin (you)" })).toBeVisible();
  await accountShell(page, "root.admin", true);
  await audit(page, "accounts-wide");
  await page.setViewportSize({ width: 390, height: 844 });
  await accountShell(page, "root.admin", true);
  await audit(page, "accounts-narrow");
  await page.setViewportSize(desktopViewport);
  await page.getByRole("button", { name: "Create invitation", exact: true }).click();
  const invite = await page.getByLabel("One-time link", { exact: true }).inputValue();
  expect(new URL(invite).pathname).toBe("/invite");
  await page.getByRole("button", { name: "Copy One-time link", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: /Copied\.|text is selected/ })).toBeVisible();
  const cookie = (await page.context().cookies()).find(item => item.name === "__Host-meguri-session");
  expect(cookie?.secure && cookie.httpOnly && cookie.sameSite === "Strict").toBe(true);
  const context = await browser.newContext({ baseURL, ignoreHTTPSErrors: true, viewport: { width: 390, height: 844 } });
  const member = await context.newPage(); observe(member);
  await openCapability(member, invite);
  await expect(member.getByRole("heading", { name: "Create your account" })).toBeVisible();
  expect(new URL(member.url()).hash).toBe("");
  await audit(member, "invite-narrow");
  await member.getByLabel("Username", { exact: true }).fill("root.admin");
  await fillPassword(member, initialPassword);
  await member.getByLabel("Confirm new password", { exact: true }).fill("different password");
  await member.getByRole("button", { name: "Create your account", exact: true }).click();
  await expect(member.getByRole("alert")).toBeFocused();
  expect(await member.getByLabel("New password", { exact: true }).inputValue() === initialPassword).toBe(true);
  await fillPassword(member, initialPassword);
  await member.getByRole("button", { name: "Create your account", exact: true }).click();
  await expect(member.getByRole("alert")).toContainText("username is unavailable");
  expect(await member.getByLabel("New password", { exact: true }).inputValue() === initialPassword).toBe(true);
  await audit(member, "invite-conflict-narrow");
  await member.getByLabel("Username", { exact: true }).fill(memberUsername);
  await member.getByRole("button", { name: "Create your account", exact: true }).click();
  await expect(member.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  await member.getByLabel("Username", { exact: true }).fill(memberUsername);
  await member.getByLabel("Password", { exact: true }).fill("incorrect");
  await member.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(member.getByRole("alert")).toBeFocused();
  await expect(member.getByLabel("Username", { exact: true })).toHaveValue(memberUsername);
  await expect(member.getByLabel("Password", { exact: true })).toHaveValue("");
  await member.getByLabel("Password", { exact: true }).fill(initialPassword);
  await member.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(member.getByRole("heading", { name: "Connect your devices" })).toBeVisible();
  await accountShell(member, memberUsername, false);
  await connectionLayout(member, memberUsername, false);
  await expect(member.getByRole("main").getByText("Collection", { exact: true })).toHaveCount(0);
  await expect(member.getByText("Total", { exact: true })).toHaveCount(0);
  await expect(member.getByRole("link", { name: "Accounts", exact: true })).toHaveCount(0);
  const ownResponse = await context.request.get(`${baseURL}/api/me/storage`);
  expect(ownResponse.status()).toBe(200);
  const ownStorage = (await ownResponse.json()).data;
  expect(Object.keys(ownStorage).sort()).toEqual(["collection_bytes", "media_bytes", "total_bytes"]);
  expect(ownStorage.collection_bytes).toBeGreaterThan(0);
  expect(ownStorage.media_bytes).toBe(0);
  expect(ownStorage.total_bytes).toBe(ownStorage.collection_bytes + ownStorage.media_bytes);
  await member.getByRole("link", { name: "Collection", exact: true }).click();
  await expect(member.getByRole("heading", { name: "Collection", exact: true })).toBeVisible();
  await expect(member.getByRole("heading", { name: "Storage on server", exact: true })).toBeVisible();
  for (const [label, bytes] of [["Collection database", ownStorage.collection_bytes], ["Media", ownStorage.media_bytes], ["Total", ownStorage.total_bytes]] as [string, number][]) {
    const row = member.locator(".collection-storage > div").filter({ has: member.getByRole("term").filter({ hasText: label }) });
    await expect(row.getByRole("term")).toHaveText(label);
    await expect(row.getByRole("definition")).toHaveText(storageValue(bytes));
  }
  for (const width of [390, 320]) {
    await member.setViewportSize({ width, height: 844 });
    await accountShell(member, memberUsername, false);
    await member.evaluate(() => { document.documentElement.style.fontSize = "200%"; });
    try {
      expect(await member.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      expect(await member.locator('[data-slot="sidebar-menu-button"], .collection-storage dt, .collection-storage dd').evaluateAll(elements => elements.every(element => element.scrollWidth <= element.clientWidth))).toBe(true);
    } finally { await member.evaluate(() => { document.documentElement.style.removeProperty("font-size"); }); }
    await expect(member.getByRole("link", { name: "Collection", exact: true })).toHaveAttribute("aria-current", "page");
    await audit(member, `collection-member-${width}`);
  }
  await member.setViewportSize({ width: 390, height: 844 });
  const denial = await context.request.get(`${baseURL}/api/admin/accounts`);
  expect(denial.status()).toBe(403); expect((await denial.json()).error.code).toBe("permission_denied");
  await member.goto("/accounts");
  await expect(member.getByRole("heading", { name: "Administrator access required" })).toBeVisible();
  await expect(member.getByRole("button", { name: "Create invitation", exact: true })).toHaveCount(0);
  await member.getByRole("link", { name: "Password", exact: true }).click();
  await expect(member.getByRole("heading", { name: "Change password", exact: true })).toBeVisible();
  await audit(member, "password-settings-narrow");
  await member.getByLabel("Current password", { exact: true }).fill(initialPassword);
  await fillPassword(member, replacedPassword);
  await member.getByRole("button", { name: "Replace password and sign out" }).click();
  await expect(member.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  await signIn(member, memberUsername, replacedPassword);
  await member.setViewportSize({ width: 1280, height: 900 });
  await accountShell(member, memberUsername, false);
  expect(await member.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await member.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Refresh accounts", exact: true }).click();
  const memberRow = page.getByRole("listitem", { name: `Account ${memberUsername}`, exact: true });
  await expect(memberRow).toBeVisible();
  const storage = memberRow.getByRole("list", { name: "Storage on server", exact: true });
  for (const label of ["Collection", "Media", "Total"]) await expect(storage.getByText(label, { exact: true })).toBeVisible();
  const amounts = storage.getByRole("listitem").locator("span:last-child");
  await expect(amounts).toHaveCount(3);
  await expect(amounts.nth(0)).toHaveText(/^[1-9][\d,.]* (B|kB|MB|GB)$/);
  await expect(amounts.nth(1)).toHaveText("0 B");
  await expect(amounts.nth(2)).toHaveText(await amounts.nth(0).innerText());
  await memberRow.getByRole("button", { name: `Create reset link for ${memberUsername}`, exact: true }).click();
  const firstReset = await memberRow.getByLabel("One-time link", { exact: true }).inputValue();
  await memberRow.getByRole("button", { name: "Copy One-time link", exact: true }).click();
  await expect(memberRow.getByRole("status").filter({ hasText: /Copied\.|text is selected/ })).toBeVisible();
  await memberRow.getByRole("button", { name: `Create reset link for ${memberUsername}`, exact: true }).click();
  await expect(memberRow.getByLabel("One-time link", { exact: true })).not.toHaveValue(firstReset);
  await expect(memberRow.getByRole("status").filter({ hasText: /Copied\.|text is selected/ })).toHaveCount(0);
  const reset = await memberRow.getByLabel("One-time link", { exact: true }).inputValue();
  expect(reset).not.toBe(firstReset);
  await openCapability(member, reset);
  await expect(member.getByRole("heading", { name: "Check your account" })).toBeVisible();
  await expect(member.getByText(`You are signed in as ${memberUsername} (member).`, { exact: true })).toBeVisible();
  await member.getByRole("button", { name: `Sign out ${memberUsername}`, exact: true }).click();
  await expect(member.getByRole("heading", { name: "Reset your password" })).toBeVisible();
  await expect(member.getByLabel("Username", { exact: true })).toHaveCount(0);
  await member.setViewportSize({ width: 1280, height: 900 });
  await audit(member, "reset-wide");
  await member.setViewportSize({ width: 390, height: 844 });
  await fillPassword(member, resetPassword);
  await member.getByRole("button", { name: "Reset your password", exact: true }).click();
  await expect(member.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  await openCapability(member, reset);
  await fillPassword(member, resetPassword);
  await member.getByRole("button", { name: "Reset your password", exact: true }).click();
  await expect(member.getByRole("alert")).toContainText("unavailable");
  await expect(member.getByLabel("New password", { exact: true })).toHaveValue("");
  const adminRow = page.getByRole("listitem", { name: "Account root.admin", exact: true });
  await adminRow.getByRole("button", { name: "Disable root.admin", exact: true }).click();
  await adminRow.getByRole("button", { name: "Confirm disable root.admin", exact: true }).click();
  await expect(adminRow.getByRole("alert")).toContainText("sole enabled, ready administrator");
  await adminRow.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(adminRow.getByRole("button", { name: "Disable root.admin", exact: true })).toBeFocused();
  await signIn(member, memberUsername, resetPassword);
  await context.request.post(`${baseURL}/api/logout`, { data: {}, headers: { Origin: baseURL! } });
  await member.getByRole("link", { name: "Password", exact: true }).click();
  await expect(member.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  await expect(member.getByLabel("Username", { exact: true })).toHaveValue("");
  await memberRow.getByRole("button", { name: `Disable ${memberUsername}`, exact: true }).click();
  await expect(memberRow.getByRole("button", { name: "Cancel", exact: true })).toBeFocused();
  await memberRow.getByRole("button", { name: `Confirm disable ${memberUsername}`, exact: true }).click();
  await expect(memberRow.getByText(/Member · Disabled · Ready/)).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/accounts");
  await expect(memberRow).toBeVisible();
  await accountShell(page, "root.admin", true);
  await expect(memberRow.getByText(memberUsername, { exact: true })).toBeVisible();
  await expect(amounts.nth(1)).toHaveText("0 B");
  await audit(page);
  await page.setViewportSize({ width: 1280, height: 900 });
  await audit(page);
  await openCapability(page, `/setup#token=${fixture.setup_token}`);
  await expect(page.getByRole("heading", { name: "Signed in as root.admin" })).toBeVisible();
  expect(new URL(page.url()).hash).toBe("");
  expect(await page.evaluate(() => localStorage.length + sessionStorage.length)).toBe(0);
  expect(await member.evaluate(() => localStorage.length + sessionStorage.length)).toBe(0);
  const missing = await context.request.get(`${baseURL}/assets/missing-abcdefgh.js`); expect(missing.status()).toBe(404);
  const missingFont = await context.request.get(`${baseURL}/assets/missing-abcdefgh.woff2`); expect(missingFont.status()).toBe(404);
  const apiMissing = await context.request.get(`${baseURL}/api/unknown`); expect(apiMissing.status()).toBe(404);
  const unknown = await context.request.get(`${baseURL}/unknown`); expect(unknown.status()).toBe(404);
  expect(bootstrapSawFragment).toBe(false); expect(external).toEqual([]);
  await context.close();
});
