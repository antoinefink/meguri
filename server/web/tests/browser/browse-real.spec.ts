// SPDX-License-Identifier: MIT
// Real compiled website, browser sessions, generated SQLite and normal sync.
import { test, expect } from "@playwright/test";
import type { APIRequestContext } from "@playwright/test";
import { DatabaseSync } from "node:sqlite";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { zstdCompressSync, zstdDecompressSync } from "node:zlib";
import AxeBuilder from "@axe-core/playwright";
const rootPassword = " Synthetic initial password ";
const memberPassword = "Synthetic browse password 2026";
type WireNote = [number, string, number, number, number, string, string, string, string, number, string];
type Chunk = { done: boolean; cards: unknown[][]; notes: WireNote[]; revlog: unknown[][] };
function collection(): Buffer {
  const directory = mkdtempSync(join(tmpdir(), "meguri-web-browse-")), path = join(directory, "synthetic.anki2");
  try {
    const db = new DatabaseSync(path);
    db.exec(readFileSync("tests/browser/fixtures/browse.sql", "utf8"));
    db.exec("INSERT INTO col VALUES(1,123,100,100,18,0,0,0,'','','','','')");
    db.prepare("INSERT INTO decks VALUES(1,'Spanish',0,0,?,?)").run(Buffer.alloc(0), Buffer.from([10, 2, 8, 1]));
    db.prepare("INSERT INTO deck_config VALUES(1,'Ordered',0,0,?)").run(Buffer.from([160, 1, 1]));
    db.prepare("INSERT INTO config VALUES('nextPos',0,0,?)").run(Buffer.from("1"));
    for (const [id, name, count] of [[10, "Vocabulary", 1], [20, "Two directions", 2]] as const) {
      db.prepare("INSERT INTO notetypes VALUES(?,?,0,0,?)").run(id, name, Buffer.alloc(0));
      for (const [ord, name] of ["Spanish", "English", "Example"].entries()) db.prepare("INSERT INTO fields VALUES(?,?,?,?)").run(id, ord, name, Buffer.alloc(0));
      for (let ord = 0; ord < count; ord++) {
        const front = Buffer.from(ord === 0 ? "{{Spanish}}" : "{{English}}");
        // P6.3 field 1: UTF-8 question template. No third-party runtime/assets.
        const config = Buffer.concat([Buffer.from([10, front.length]), front]);
        db.prepare("INSERT INTO templates VALUES(?,?,?,0,0,?)").run(id, ord, ord === 0 ? "Forward" : "Reverse", config);
      }
    }
    db.close(); return readFileSync(path);
  } finally { rmSync(directory, { recursive: true, force: true }); }
}
async function wire(request: APIRequestContext, origin: string, key: string, method: string, body: object | Buffer): Promise<Buffer> {
  const response = await request.post(`${origin}/sync/${method}`, { headers: { "anki-sync": JSON.stringify({ v: 11, k: key, c: "synthetic-browser-browse", s: "browse-web" }), "Content-Type": "application/octet-stream" }, data: zstdCompressSync(Buffer.isBuffer(body) ? body : Buffer.from(JSON.stringify(body))) });
  expect(response.status()).toBe(200); return zstdDecompressSync(await response.body());
}
async function native<T>(request: APIRequestContext, origin: string, key: string, method: string, body: object): Promise<T> { return JSON.parse((await wire(request, origin, key, method, body)).toString()) as T; }
async function round(request: APIRequestContext, origin: string, key: string, min: number, notes: number, cards: number, incoming?: WireNote[]) {
  const meta = await native<{ usn: number }>(request, origin, key, "meta", { v: 11, cv: "synthetic" });
  const graves = await native<{ notes: number[]; cards: number[] }>(request, origin, key, "start", { minUsn: min, lnewer: false, graves: null });
  await native(request, origin, key, "applyChanges", { changes: { models: [], decks: [[], []], tags: [] } });
  const chunk = await native<Chunk>(request, origin, key, "chunk", {}); expect(chunk.done).toBe(true);
  await native(request, origin, key, "applyChunk", { chunk: { done: true, ...(incoming ? { notes: incoming } : {}) } });
  const counts = [[0, 0, 0], cards, notes, 0, 0, 2, 1, 1];
  expect(await native(request, origin, key, "sanityCheck2", { client: counts })).toEqual({ status: "ok", c: counts, s: counts });
  expect(await native<number>(request, origin, key, "finish", {})).toBeGreaterThan(0);
  return { min: meta.usn, graves, chunk };
}
test("real Browse field examples, compact editing, siblings, conflict and native incremental sync", async ({ page, browser, baseURL }, testInfo) => {
  const admin = await browser.newContext({ ignoreHTTPSErrors: true });
  try {
    const origin = baseURL!, headers = { Origin: origin };
    const secrets = [rootPassword, memberPassword];
    const status = await (await admin.request.get(`${origin}/api/status`)).json() as { data: { setup_required: boolean } };
    if (status.data.setup_required) {
      const fixture = JSON.parse(readFileSync("/fixture/fixture.json", "utf8")) as { setup_token: string };
      secrets.push(fixture.setup_token);
      expect((await admin.request.post(`${origin}/api/setup`, { headers, data: { token: fixture.setup_token, username: "root.admin", password: rootPassword } })).status()).toBe(201);
    }
    expect((await admin.request.post(`${origin}/api/login`, { headers, data: { username: "root.admin", password: rootPassword } })).status()).toBe(200);
    const invitation = await (await admin.request.post(`${origin}/api/admin/invitations`, { headers, data: {} })).json() as { data: { url: string } };
    secrets.push(invitation.data.url, new URL(invitation.data.url).hash.slice(7));
    const username = `browse.${randomUUID().slice(0, 12)}`;
    expect((await admin.request.post(`${origin}/api/invitations/claim`, { headers, data: { token: new URL(invitation.data.url).hash.slice(7), username, password: memberPassword } })).status()).toBe(201);
    const host = await native<{ key: string }>(admin.request, origin, "", "hostKey", { u: username, p: memberPassword });
    secrets.push(host.key);
    expect((await wire(admin.request, origin, host.key, "upload", collection())).toString()).toBe("OK");
    async function capture(name: string) {
      await test.step(`Capture ${name}`, async () => {
        await expect(page).toHaveURL(/\/browse$/);
        await expect(page.locator('input[type="password"]')).toHaveCount(0);
        const visible = await page.evaluate(() => [
          document.body.innerText,
          ...Array.from(document.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>("input, textarea, select"), control => control.value),
        ].join("\n"));
        for (const secret of secrets) expect(visible.includes(secret), "capture must exclude credentials and capability URLs").toBe(false);
        const path = `/out/browser-artifacts/${name}.png`;
        await page.screenshot({ path, fullPage: true });
        await testInfo.attach(name, { path, contentType: "image/png" });
      });
    }
    await page.goto("/browse"); await expect(page).toHaveURL(/\/login$/);
    await page.getByLabel("Username", { exact: true }).fill(username); await page.getByLabel("Password", { exact: true }).fill(memberPassword);
    await page.getByRole("button", { name: "Sign in", exact: true }).click(); await expect(page).toHaveURL(/\/collection$/);
    await page.getByRole("link", { name: "Browse", exact: true }).click();
    const rows = () => page.getByRole("list", { name: "Cards", exact: true }).getByRole("button");
    const samples = [["el viaje", "trip", "Quiero aprovechar el viaje para visitar a mi hermana."], ["aprovechar", "make use of", "Quiero aprovechar el día."], ["la oportunidad", "opportunity", "Hay que aprovechar la oportunidad."]];
    for (const [Spanish, English, Example] of samples) {
      await page.getByRole("button", { name: "Add note", exact: true }).click(); await page.getByLabel("Spanish", { exact: true }).fill(Spanish!);
      await page.getByLabel("English", { exact: true }).fill(`<b>${English!}</b>`); await page.getByLabel("Example", { exact: true }).fill(Example!);
      await page.getByRole("button", { name: "Create note" }).click(); await expect(page.getByRole("status").filter({ hasText: /^Note \d+ saved\.$/ })).toBeVisible();
    }
    await expect(rows()).toHaveCount(3);
    await page.getByLabel("Search note fields").fill("aprovechar"); await page.getByRole("button", { name: "Search", exact: true }).click(); await expect(rows()).toHaveCount(3);
    await expect(rows().first().locator("mark")).toHaveText("aprovechar");
    await page.getByLabel("Field", { exact: true }).selectOption("Spanish"); await expect(rows()).toHaveCount(1); await expect(rows().first().locator("strong")).toHaveText("aprovechar");
    await page.getByLabel("Field", { exact: true }).selectOption(""); await expect(rows()).toHaveCount(3);
    const first = await round(admin.request, origin, host.key, 0, 3, 3); expect(first.chunk.notes).toHaveLength(3); expect(first.chunk.cards).toHaveLength(3);
    await rows().first().click(); await expect(page.getByLabel("Example", { exact: true })).toHaveValue(samples[0]![2]!);
    await page.getByText("Card info", { exact: true }).click(); await expect(page.getByText("Forward", { exact: true })).toBeVisible();
    await capture("browse-real-desktop");
    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByLabel("Example", { exact: true }).fill("Quiero aprovechar el viaje para ver a mi hermana."); await page.getByRole("button", { name: "Save changes" }).click();
    await expect(page.getByRole("status").filter({ hasText: /^Note \d+ saved\.$/ })).toBeVisible();
    await page.getByRole("button", { name: "Back to results" }).click(); await expect(page.getByLabel("Search note fields")).toHaveValue("aprovechar"); await expect(rows().first()).toContainText("para ver");
    const edited = await round(admin.request, origin, host.key, first.min, 3, 3); expect(edited.chunk.notes).toHaveLength(1);
    expect(edited.chunk.notes[0]![6]).toBe("el viaje\x1f<b>trip</b>\x1fQuiero aprovechar el viaje para ver a mi hermana.");
    // AE4: retain the proposed web values when a device correction commits.
    await rows().first().click(); await page.getByLabel("Example", { exact: true }).fill("My older browser draft");
    const device: WireNote = [...edited.chunk.notes[0]!]; device[3] = Math.floor(Date.now() / 1000) + 60; device[4] = -1; device[6] = "el viaje\x1f<b>trip</b>\x1fDevice correction aprovechar";
    const corrected = await round(admin.request, origin, host.key, edited.min, 3, 3, [device]);
    await page.getByRole("button", { name: "Save changes" }).click(); await expect(page.getByRole("alert")).toContainText("Your draft is still here"); await expect(page.getByLabel("Example", { exact: true })).toHaveValue("My older browser draft");
    page.once("dialog", dialog => void dialog.accept()); await page.getByRole("button", { name: "Load server copy" }).click(); await expect(page.getByLabel("Example", { exact: true })).toHaveValue("Device correction aprovechar");
    await page.getByRole("button", { name: "Back to results" }).click();
    // AE3: distinct generated siblings, cancellation, exact deletion and graves.
    await page.getByRole("button", { name: "Add note", exact: true }).click(); await page.getByLabel("Note type", { exact: true }).selectOption("20");
    await page.getByLabel("Spanish", { exact: true }).fill("hermanos"); await page.getByLabel("English", { exact: true }).fill("siblings"); await page.getByLabel("Example", { exact: true }).fill("Two cards");
    await page.getByRole("button", { name: "Create note" }).click(); await expect(page.getByRole("status").filter({ hasText: /^Note \d+ saved\.$/ })).toBeVisible();
    await page.getByLabel("Search note fields").fill("hermanos"); await page.getByRole("button", { name: "Search", exact: true }).click(); await expect(rows()).toHaveCount(2);
    const added = await round(admin.request, origin, host.key, corrected.min, 4, 5); const siblings = added.chunk.cards.filter(c => c[1] === added.chunk.notes.find(n => n[6].startsWith("hermanos"))?.[0]); expect(siblings).toHaveLength(2); expect(siblings[0]![0]).not.toBe(siblings[1]![0]);
    await rows().first().click(); await expect(page.getByText("2 cards share this note.")).toBeVisible(); await page.getByRole("button", { name: "Delete note", exact: true }).click();
    const confirmation = page.getByRole("region", { name: "Confirm note deletion" }); await expect(confirmation).toContainText("hermanos"); await expect(confirmation).toContainText("all 2 of its cards");
    await confirmation.getByRole("button", { name: "Cancel" }).click(); await page.getByRole("button", { name: "Back to results" }).click(); await expect(rows()).toHaveCount(2);
    await rows().last().click(); await page.getByRole("button", { name: "Delete note", exact: true }).click(); await capture("browse-real-compact");
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    await confirmation.getByRole("button", { name: "Confirm delete note" }).click(); await expect(page.getByRole("status").filter({ hasText: "Note and its cards deleted." })).toBeVisible(); await expect(rows()).toHaveCount(0);
    const deleted = await round(admin.request, origin, host.key, added.min, 3, 3); expect(deleted.graves.cards.sort()).toEqual(siblings.map(c => c[0]).sort()); expect(deleted.graves.notes).toHaveLength(1);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect(testInfo.attachments.filter(attachment => attachment.name.startsWith("browse-real-")).map(attachment => attachment.name))
      .toEqual(["browse-real-desktop", "browse-real-compact"]);
  } finally { await admin.close(); }
});
