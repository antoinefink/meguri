// SPDX-License-Identifier: MIT
// Compiled UI states with intercepted synthetic APIs. browse-real.spec.ts and
// server_it browse::* separately cover real HTTP and native sync authority.
import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import type { BrowseCard, BrowseDetail, BrowseScope } from "../../src/api";
function barrier() { let release!: () => void; const promise = new Promise<void>(resolve => { release = resolve; }); return { promise, release }; }
const samples = [
  ["el viaje", "Quiero aprovechar el viaje para visitar a mi hermana."],
  ["aprovechar", "Quiero aprovechar el día."],
  ["la oportunidad", "Hay que aprovechar la oportunidad."],
];
async function fixture(page: Page, origin: string, many = false) {
  const notes = samples.map(([Spanish, Example], i) => ({ note_id: String(101 + i), note_type_id: "10", fields: { Spanish: Spanish!, English: "<b>keep this field</b>", Example: Example! }, tags: ["study"], revision: "r1" }));
  const state = { account: "synthetic-a", responseAccount: "synthetic-a", notes, pages: [] as BrowseScope[], writes: [] as { path: string; body: Record<string, unknown> }[],
    pageGate: null as Promise<void> | null, detailGate: null as Promise<void> | null, writeGate: null as Promise<void> | null,
    conflict: false, detailReads: 0, pageReads: 0, writesStarted: 0, create: false };
  const card = (i: number, scope: BrowseScope): BrowseCard => {
    const note = notes[i % notes.length]!;
    const field = scope.field || (scope.query && note.fields.Spanish.toLowerCase().includes(scope.query.toLowerCase()) ? "Spanish" : "Example");
    const text = note.fields[field as keyof typeof note.fields] ?? "";
    const at = text.toLowerCase().indexOf(scope.query.toLowerCase());
    return { card_id: String(201 + i), note_id: note.note_id, title: note.fields.Spanish, field, deck: "Spanish", template: "Forward",
      excerpt: !scope.query || at < 0 ? [{ text, matched: false }] : [{ text: text.slice(0, at), matched: false }, { text: text.slice(at, at + scope.query.length), matched: true }, { text: text.slice(at + scope.query.length), matched: false }] };
  };
  await page.context().addCookies([{ name: "__Host-meguri-session", value: "synthetic", url: origin, secure: true, httpOnly: true, sameSite: "Strict" }]);
  await page.route("**/api/**", async route => {
    const path = new URL(route.request().url()).pathname;
    const reply = (data: unknown) => route.fulfill({ contentType: "application/json", body: JSON.stringify({ data }) });
    if (path === "/api/status") return reply({ setup_required: false });
    if (path === "/api/me") return reply({ id: state.account, username: "synthetic.member", role: "member", server_url: origin });
    if (path === "/api/logout") return reply(null);
    if (route.request().headers()["x-meguri-expected-account"] !== state.account) return route.fulfill({ status: 409, contentType: "application/json", body: JSON.stringify({ error: { code: "stale_authority" } }) });
    expect(route.request().headers()["x-meguri-expected-account"]).toBe(state.account);
    const body = route.request().postDataJSON() as Record<string, unknown>;
    if (path.endsWith("/catalog")) return reply({ account_id: state.responseAccount, generation: "g1", decks: [{ deck_id: "1", name: "Spanish", filtered: false }, { deck_id: "2", name: "Filtered", filtered: true }], note_types: [{ note_type_id: "10", name: "Vocabulary", field_names: ["Spanish", "English", "Example"] }], next_cursor: null });
    if (path.endsWith("/page")) {
      state.pages.push(body as BrowseScope); state.pageReads++;
      const cards = Array.from({ length: many ? 50 : 3 }, (_, i) => card(i, body as BrowseScope)).filter(c => {
        const note = notes.find(n => n.note_id === c.note_id)!;
        return body.deck_id !== "2" && (!body.query || Object.entries(note.fields).some(([name, text]) => (!body.field || body.field === name) && text.toLowerCase().includes(String(body.query).toLowerCase())));
      });
      const result = { account_id: state.responseAccount, cards, next_cursor: body.query === "sparse" && !body.cursor ? "continue" : null };
      await state.pageGate; return reply(result);
    }
    if (path.endsWith("/detail")) {
      state.detailReads++;
      const c = card(Number(body.card_id) - 201, { query: "", field: null, deck_id: null });
      const note = notes.find(n => n.note_id === c.note_id)!;
      const result: BrowseDetail = { account_id: state.responseAccount, card: c, note: structuredClone(note), field_names: ["Spanish", "English", "Example"], sibling_ids: [c.card_id, "9007199254740993"], confirmation: "confirm-r1" };
      await state.detailGate; return reply(result);
    }
    state.writesStarted++; state.writes.push({ path, body }); await state.writeGate;
    if (state.conflict) return route.fulfill({ status: 409, contentType: "application/json", body: JSON.stringify({ error: { code: "browse_conflict" } }) });
    if (path.endsWith("/delete")) return reply({ account_id: state.responseAccount });
    const note = notes.find(n => n.note_id === body.note_id) ?? notes[0]!;
    if (path.endsWith("/edit")) { Object.assign(note.fields, body.fields); note.revision = "r2"; }
    if (path.endsWith("/create")) state.create = true;
    return reply({ account_id: state.responseAccount, note_id: note.note_id, revision: note.revision });
  });
  return state;
}
async function search(page: Page, query: string) { await page.getByLabel("Search note fields").fill(query); await page.getByRole("button", { name: "Search", exact: true }).click(); }
const rows = (page: Page) => page.getByRole("list", { name: "Cards", exact: true }).getByRole("button");
test("field examples, inert highlights, shared editing and unchanged markup", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3);
  await search(page, "aprovechar"); await expect(rows(page)).toHaveCount(3); await expect(rows(page).first().locator("mark")).toHaveText("aprovechar");
  await page.getByLabel("Field", { exact: true }).selectOption("Spanish"); await expect(rows(page)).toHaveCount(1); await expect(rows(page)).toContainText("aprovechar");
  await page.getByLabel("Field", { exact: true }).selectOption(""); await expect(rows(page)).toHaveCount(3);
  await rows(page).first().click(); await expect(page.getByLabel("Example", { exact: true })).toBeFocused();
  await expect(page.getByText("2 cards share this note.")).toBeVisible(); await page.getByText("Card info", { exact: true }).click();
  await expect(page.getByText("201", { exact: true })).toBeVisible(); await expect(page.getByText("101", { exact: true })).toBeVisible();
  await page.getByLabel("Example", { exact: true }).fill("Quiero aprovechar el viaje para ver a mi hermana."); await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Note 101 saved." })).toBeVisible();
  expect(state.writes[0]!.body.fields).toEqual({ Example: "Quiero aprovechar el viaje para ver a mi hermana." });
  await expect(page.getByLabel("Search note fields")).toHaveValue("aprovechar"); await expect(rows(page).first()).toContainText("para ver");
  await expect(page.getByLabel("English", { exact: true })).toHaveValue("<b>keep this field</b>");
  state.notes[0]!.fields.Spanish = "<img src=x onerror=alert(1)>"; await search(page, "aprovechar"); await expect(rows(page).first()).toContainText("<img src=x onerror=alert(1)>"); await expect(rows(page).first().locator("img, script")).toHaveCount(0);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});
test("compact Back retains scope, scroll and row focus; dirty drafts require a decision", async ({ page, baseURL }) => {
  await page.setViewportSize({ width: 390, height: 844 }); await fixture(page, baseURL!, true); await page.goto("/browse"); await expect(rows(page)).toHaveCount(50);
  await search(page, "aprovechar"); await expect(rows(page)).toHaveCount(50);
  await page.locator(".browse-results").evaluate(el => { el.scrollTop = 600; });
  const selected = rows(page).nth(12); await selected.click();
  const before = await page.locator(".browse-results").evaluate(el => el.scrollTop); await expect(page.getByLabel("Example", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Search note fields")).toBeHidden(); await page.getByLabel("Example", { exact: true }).fill("draft");
  page.once("dialog", dialog => void dialog.dismiss()); await page.getByRole("button", { name: "Back to results" }).click(); await expect(page.getByLabel("Example", { exact: true })).toHaveValue("draft");
  page.once("dialog", dialog => void dialog.accept()); await page.getByRole("button", { name: "Back to results" }).click();
  await expect(page.getByLabel("Search note fields")).toHaveValue("aprovechar"); await expect(selected).toBeFocused();
  // Clicking can scroll the selected row into view first; Back restores that
  // position, which remains well below the top of the bounded list.
  expect(await page.locator(".browse-results").evaluate(el => el.scrollTop)).toBeGreaterThanOrEqual(before);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});
test("conflicts retain proposed fields and deletion requires the displayed sibling confirmation", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await rows(page).first().click(); await page.getByLabel("Example", { exact: true }).fill("my draft");
  state.conflict = true; await page.getByRole("button", { name: "Save changes" }).click(); await expect(page.getByRole("alert")).toContainText("Your draft is still here"); await expect(page.getByLabel("Example", { exact: true })).toHaveValue("my draft");
  page.once("dialog", dialog => void dialog.dismiss()); await rows(page).nth(1).click(); await expect(page.getByLabel("Example", { exact: true })).toHaveValue("my draft");
  state.conflict = false; page.once("dialog", dialog => void dialog.accept()); await page.getByRole("button", { name: "Load server copy" }).click();
  await expect(page.getByLabel("Example", { exact: true })).toHaveValue(samples[0]![1]!); await page.getByRole("button", { name: "Delete note", exact: true }).click();
  const confirmation = page.getByRole("region", { name: "Confirm note deletion" }); await expect(confirmation).toContainText("el viaje"); await expect(confirmation).toContainText("all 2 of its cards");
  await confirmation.getByRole("button", { name: "Cancel" }).click(); expect(state.writes).toHaveLength(1);
  await page.getByRole("button", { name: "Delete note", exact: true }).click(); await confirmation.getByRole("button", { name: "Confirm delete note" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Note and its cards deleted." })).toBeVisible();
  expect(state.writes[1]!.body).toEqual({ note_id: "101", revision: "r1", confirmation: "confirm-r1" });
});
test("superseded detail reads cannot overwrite another selection", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3);
  const old = barrier(); state.detailGate = old.promise; await rows(page).first().click(); await expect.poll(() => state.detailReads).toBe(1);
  state.detailGate = null; await rows(page).nth(1).click(); await expect(page.getByLabel("Spanish", { exact: true })).toHaveValue("aprovechar");
  old.release(); await page.getByLabel("Example", { exact: true }).fill("current draft"); await expect(page.getByLabel("Spanish", { exact: true })).toHaveValue("aprovechar");
});
test("sparse pages offer continuation and account mismatches clear all collection controls", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3);
  await search(page, "sparse"); await expect(page.getByRole("button", { name: "Load more cards" })).toBeVisible(); await page.getByRole("button", { name: "Load more cards" }).click();
  await expect(page.getByRole("button", { name: "Load more cards" })).toHaveCount(0);
  state.responseAccount = "synthetic-b"; await search(page, "aprovechar");
  await expect(page.getByRole("alert")).toContainText("account changed"); await expect(rows(page)).toHaveCount(0); await expect(page.getByRole("button", { name: "Add note", exact: true })).toBeDisabled();
});
test("a mutation response from a different account never announces success", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await rows(page).first().click(); await page.getByLabel("Example", { exact: true }).fill("draft");
  const held = barrier(); state.writeGate = held.promise; await page.getByRole("button", { name: "Save changes" }).click(); await expect.poll(() => state.writesStarted).toBe(1);
  state.responseAccount = "synthetic-b"; held.release(); await expect(page.getByRole("alert")).toContainText("account changed"); await expect(page.getByLabel("Example", { exact: true })).toHaveCount(0);
  await expect(page.getByText(/Note 101 saved/)).toHaveCount(0);
});
test("Add note uses only normal decks and sends the collection generation", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(page.getByRole("button", { name: "Add note", exact: true })).toBeEnabled(); await page.getByRole("button", { name: "Add note", exact: true }).click();
  await expect(page.locator("#new-deck option")).toHaveText(["Spanish"]); await page.getByLabel("Spanish", { exact: true }).fill("nuevo"); await page.getByLabel("Example", { exact: true }).fill("example");
  await page.getByRole("button", { name: "Create note" }).click(); await expect.poll(() => state.create).toBe(true);
  expect(state.writes[0]!.body).toEqual({ generation: "g1", deck_id: "1", note_type_id: "10", fields: { Spanish: "nuevo", Example: "example" }, tags: [] });
});

test("a list read begun before a save cannot restore old excerpts", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3); await rows(page).first().click();
  const held = barrier(); state.pageGate = held.promise; const before = state.pageReads;
  await search(page, "aprovechar"); await expect.poll(() => state.pageReads).toBe(before + 1);
  await page.getByLabel("Example", { exact: true }).fill("aprovechar the new sentence"); state.pageGate = null;
  await page.getByRole("button", { name: "Save changes" }).click(); await expect(rows(page).first()).toContainText("the new sentence");
  held.release(); await expect(page.getByLabel("Example", { exact: true })).toHaveValue("aprovechar the new sentence"); await expect(rows(page).first()).toContainText("the new sentence");
});
test("identity changes unmount drafts and disregard a pending old-account read", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3); await rows(page).first().click();
  await page.getByLabel("Example", { exact: true }).fill("old-account draft");
  const held = barrier(); state.pageGate = held.promise;
  const before = state.pageReads; await search(page, "aprovechar"); await expect.poll(() => state.pageReads).toBe(before + 1);
  // A normal identity refresh is triggered by an explicit account navigation.
  // The open draft is deliberately discarded; the pending read stays bound A.
  page.once("dialog", dialog => void dialog.accept()); state.account = "synthetic-b"; state.responseAccount = "synthetic-b";
  state.pageGate = null; state.notes.forEach(note => { note.fields.Spanish = "New account only"; note.fields.Example = "new-account field"; });
  await page.getByRole("link", { name: "Browse", exact: true }).click(); await expect(rows(page).first()).toContainText("New account only");
  held.release(); await expect(page.getByLabel("Example", { exact: true })).toHaveCount(0); await expect(page.getByText("old-account draft", { exact: true })).toHaveCount(0);
  await expect(rows(page).first()).toContainText("New account only");
});
