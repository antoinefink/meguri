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
  // Mark each individual read only after fetch/JSON has completed or aborted
  // and the application's promise callbacks have drained. Route fulfillment
  // alone does not fence a late response in the browser.
  await page.addInitScript(() => {
    const original = window.fetch.bind(window);
    const reads: Record<string, number> = {};
    window.fetch = async (...args) => {
      const kind = String(args[0]).match(/\/browse\/(page|detail)$/)?.[1];
      if (!kind) return original(...args);
      reads[kind] = (reads[kind] ?? 0) + 1;
      const id = `${kind}:${reads[kind]}`;
      const settled = () => setTimeout(() => {
        const root = document.documentElement;
        const completions = JSON.parse(root.dataset.browseReadsSettled ?? "{}") as Record<string, string>;
        completions[id] = args[1]?.signal?.aborted ? "aborted" : "completed";
        root.dataset.browseReadsSettled = JSON.stringify(completions);
      }, 0);
      try {
        const response = await original(...args), json = response.json.bind(response);
        response.json = async () => {
          try { return await json(); }
          finally { settled(); }
        };
        return response;
      } catch (error) { settled(); throw error; }
    };
  });
  const notes = samples.map(([Spanish, Example], i) => ({ note_id: String(101 + i), note_type_id: "10", fields: { Spanish: Spanish!, English: "<b>keep this field</b>", Example: Example! }, tags: ["study"], revision: "r1" }));
  const state = { account: "synthetic-a", responseAccount: "synthetic-a", notes, pages: [] as BrowseScope[], writes: [] as { path: string; body: Record<string, unknown> }[],
    pageGate: null as Promise<void> | null, detailGate: null as Promise<void> | null, writeGate: null as Promise<void> | null,
    pageGateAt: null as number | null, releasedPages: 0, releasedDetails: 0, conflict: false, catalogError: false, detailError: false, pageError: false, detailUnavailable: false, sparse: false, detailReads: 0, pageReads: 0, writesStarted: 0, create: false };
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
    if (path.endsWith("/catalog")) {
      if (state.catalogError) return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: { code: "storage_unavailable" } }) });
      return reply({ account_id: state.responseAccount, generation: "g1", decks: [{ deck_id: "1", name: "Spanish", filtered: false }, { deck_id: "2", name: "Filtered", filtered: true }], note_types: [{ note_type_id: "10", name: "Vocabulary", field_names: ["Spanish", "English", "Example"] }], next_cursor: null });
    }
    if (path.endsWith("/page")) {
      state.pages.push(body as BrowseScope); state.pageReads++;
      if (state.pageError) return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: { code: "storage_unavailable" } }) });
      const position = state.sparse ? Number(body.cursor ?? 0) + 200 : 0;
      const indexes = state.sparse ? position === 600 ? [399] : position === 1200 ? [800, 801] : [] : Array.from({ length: many ? 50 : 3 }, (_, i) => i);
      const cards = indexes.map(i => card(i, body as BrowseScope)).filter(c => {
        const note = notes.find(n => n.note_id === c.note_id)!;
        return body.deck_id !== "2" && (!body.query || Object.entries(note.fields).some(([name, text]) => (!body.field || body.field === name) && text.toLowerCase().includes(String(body.query).toLowerCase())));
      });
      const result = { account_id: state.responseAccount, cards, next_cursor: state.sparse && position < 1200 ? String(position) : null, examined_through: state.sparse ? String(position) : cards.at(-1)?.card_id ?? null };
      const gate = state.pageGateAt === null || state.pageReads === state.pageGateAt ? state.pageGate : null;
      await gate;
      await reply(result);
      if (gate) state.releasedPages++;
      return;
    }
    if (path.endsWith("/detail")) {
      state.detailReads++;
      if (state.detailError) return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: { code: "storage_unavailable" } }) });
      const c = card(Number(body.card_id) - 201, { query: "", field: null, deck_id: null });
      const note = notes.find(n => n.note_id === c.note_id)!;
      if (state.detailUnavailable) return reply({ account_id: state.responseAccount, card: { ...c, unavailable: `Card ${c.card_id} · Note ${c.note_id}. Unavailable for browser editing.` } });
      const result: BrowseDetail = { account_id: state.responseAccount, card: c, note: structuredClone(note), field_names: ["Spanish", "English", "Example"], sibling_ids: [c.card_id, "9007199254740993"], confirmation: "confirm-r1" };
      const gate = state.detailGate;
      await gate;
      await reply(result);
      if (gate) state.releasedDetails++;
      return;
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
async function search(page: Page, query: string) {
  await page.getByLabel("Search note fields").fill(query);
  const requested = page.waitForRequest(request => request.url().endsWith("/browse/page") && request.method() === "POST"
    && request.postDataJSON().query === query && !request.postDataJSON().cursor);
  await page.getByRole("button", { name: "Search", exact: true }).click(); await requested;
}
const rows = (page: Page) => page.getByRole("list", { name: "Cards", exact: true }).getByRole("button");
async function readSettled(page: Page, kind: "page" | "detail", ordinal: number, result: "aborted" | "completed") {
  await expect.poll(() => page.evaluate(id => (JSON.parse(document.documentElement.dataset.browseReadsSettled ?? "{}") as Record<string, string>)[id], `${kind}:${ordinal}`)).toBe(result);
}
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
  const selected = rows(page).nth(12);
  await selected.focus(); await selected.scrollIntoViewIfNeeded();
  // Observe the live scrolling element before compact detail hides the list.
  const before = await page.locator(".browse-results").evaluate(el => el.scrollTop);
  expect(before).toBeGreaterThan(0);
  await selected.press("Enter"); await expect(page.getByLabel("Example", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Search note fields")).toBeHidden(); await page.getByLabel("Example", { exact: true }).fill("draft");
  page.once("dialog", dialog => void dialog.dismiss()); await page.getByRole("button", { name: "Back to results" }).click(); await expect(page.getByLabel("Example", { exact: true })).toHaveValue("draft");
  page.once("dialog", dialog => void dialog.accept()); await page.getByRole("button", { name: "Back to results" }).click();
  await expect(page.getByLabel("Search note fields")).toHaveValue("aprovechar"); await expect(selected).toBeFocused();
  await expect.poll(() => page.locator(".browse-results").evaluate(el => el.scrollTop)).toBeCloseTo(before, 0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

for (const entry of ["history", "hash"] as const) test(`declined ${entry} navigation clears a synthetic credential before guarding the draft`, async ({ page, baseURL }) => {
  await fixture(page, baseURL!);
  await page.addInitScript(() => {
    const confirm = window.confirm.bind(window);
    window.confirm = message => {
      document.documentElement.dataset.fragmentClearedAtGuard = String(location.hash === "");
      return confirm(message);
    };
  });
  await page.goto("/browse?draft=synthetic"); await rows(page).first().click();
  await page.getByLabel("Example", { exact: true }).fill("retained draft");
  const decision = page.waitForEvent("dialog").then(dialog => dialog.dismiss());
  await page.evaluate(entry => {
    const fragment = `token=${"S".repeat(43)}`; // Synthetic placeholder only.
    if (entry === "hash") location.hash = fragment;
    else {
      history.pushState(null, "", `/reset?source=synthetic#${fragment}`);
      window.dispatchEvent(new PopStateEvent("popstate"));
    }
  }, entry);
  await decision;
  await expect(page.locator("html")).toHaveAttribute("data-fragment-cleared-at-guard", "true");
  await expect(page).toHaveURL(`${baseURL}/browse?draft=synthetic`);
  await expect(page.getByLabel("Example", { exact: true })).toHaveValue("retained draft");
  // Revisit the entry we refused. Its fragment must already be gone, and must
  // not be offered to the application again when the user later leaves.
  page.once("dialog", dialog => void dialog.accept()); await page.goBack();
  if (entry === "history") {
    await expect(page).toHaveURL(`${baseURL}/reset?source=synthetic`);
    await page.getByRole("button", { name: "Sign out synthetic.member", exact: true }).click();
    await expect(page.getByText("Open the original invitation or reset link to continue.", { exact: true })).toBeVisible();
  } else {
    await expect(page).toHaveURL(`${baseURL}/browse?draft=synthetic`);
    await expect(page.getByLabel("Example", { exact: true })).toHaveCount(0);
    await rows(page).first().click();
    await expect(page.getByLabel("Example", { exact: true })).toHaveValue(samples[0]![1]!);
  }
});

test("accepted history navigation still delivers a cleared synthetic credential", async ({ page, baseURL }) => {
  await fixture(page, baseURL!); await page.goto("/browse"); await rows(page).first().click();
  await page.getByLabel("Example", { exact: true }).fill("discarded draft");
  const decision = page.waitForEvent("dialog").then(dialog => dialog.accept());
  await page.evaluate(() => {
    history.pushState(null, "", `/reset#token=${"S".repeat(43)}`); // Synthetic placeholder only.
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  await decision;
  await expect(page).toHaveURL(`${baseURL}/reset`);
  await page.getByRole("button", { name: "Sign out synthetic.member", exact: true }).click();
  await expect(page.getByText("Secure link received. It has been removed from the address bar.", { exact: true })).toBeVisible();
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
  old.release(); await expect.poll(() => state.releasedDetails).toBe(1); await readSettled(page, "detail", 1, "aborted");
  await page.getByLabel("Example", { exact: true }).fill("current draft"); await expect(page.getByLabel("Spanish", { exact: true })).toHaveValue("aprovechar");
});
test("sparse pages automatically advance and account mismatches clear collection controls", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3);
  state.sparse = true;
  const before = state.pageReads;
  await search(page, "aprovechar"); await expect(rows(page)).toHaveCount(1);
  expect(state.pageReads - before).toBe(3); await expect(rows(page).first()).toHaveAttribute("data-card", "600");
  await page.getByRole("button", { name: "Load more cards" }).click(); await expect(rows(page)).toHaveCount(3);
  expect(state.pageReads - before).toBe(6);
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

test("an existing note saves with its detail revision when the initial catalog fails", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); state.catalogError = true;
  await page.goto("/browse"); await expect(page.getByRole("alert")).toContainText("Storage is unavailable");
  await expect(rows(page)).toHaveCount(3);
  await expect(page.getByRole("button", { name: "Add note", exact: true })).toBeDisabled();
  await rows(page).first().click(); await page.getByLabel("Example", { exact: true }).fill("saved without catalog");
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Note 101 saved." })).toBeVisible();
  expect(state.writes).toEqual([{ path: "/api/me/browse/edit", body: { note_id: "101", revision: "r1", fields: { Example: "saved without catalog" }, add_tags: [], remove_tags: [] } }]);
  await expect(page.getByLabel("Example", { exact: true })).toHaveValue("saved without catalog");
  await expect(page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Add note", exact: true })).toBeDisabled();
  expect(state.create).toBe(false);
});

test("a list read begun before a save cannot restore old excerpts", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3); await rows(page).first().click();
  const held = barrier(); state.pageGate = held.promise; const before = state.pageReads;
  await search(page, "aprovechar"); await expect.poll(() => state.pageReads).toBe(before + 1);
  await page.getByLabel("Example", { exact: true }).fill("aprovechar the new sentence"); state.pageGate = null;
  await page.getByRole("button", { name: "Save changes" }).click(); await expect(rows(page).first()).toContainText("the new sentence");
  held.release(); await expect.poll(() => state.releasedPages).toBe(1); await readSettled(page, "page", before + 1, "aborted");
  await expect(page.getByLabel("Example", { exact: true })).toHaveValue("aprovechar the new sentence"); await expect(rows(page).first()).toContainText("the new sentence");
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
  held.release(); await expect.poll(() => state.releasedPages).toBe(1); await readSettled(page, "page", before + 1, "aborted");
  await expect(page.getByLabel("Example", { exact: true })).toHaveCount(0); await expect(page.getByText("old-account draft", { exact: true })).toHaveCount(0);
  await expect(rows(page).first()).toContainText("New account only");
});

test("selection handover fences typing in the previous editor during a delayed response", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await rows(page).first().click();
  const previous = page.getByLabel("Example", { exact: true });
  await expect(previous).toHaveValue(samples[0]![1]!);
  const held = barrier(); state.detailGate = held.promise;
  await rows(page).nth(1).focus(); await rows(page).nth(1).press("Enter");
  await expect.poll(() => state.detailReads).toBe(2);
  await expect(previous).toBeDisabled(); await expect(page.getByLabel("Tags", { exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  // Keyboard input during the handover cannot create a draft that the response
  // would silently replace. The selected row still owns keyboard focus.
  await page.keyboard.type("typing while selection is pending");
  await expect(previous).toHaveValue(samples[0]![1]!);
  held.release(); await expect(previous).toBeEnabled(); await expect(previous).toHaveValue(samples[1]![1]!);
  await previous.fill("new selection draft"); await expect(previous).toHaveValue("new selection draft");
  expect(state.writes).toHaveLength(0);
});

test("saving a later sparse result replays the examined range and keeps nearby matches", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3);
  state.sparse = true; await search(page, "aprovechar"); await expect(rows(page)).toHaveCount(1);
  await page.getByRole("button", { name: "Load more cards" }).click(); await expect(rows(page)).toHaveCount(3);
  await rows(page).nth(1).click(); await expect(page.getByLabel("Spanish", { exact: true })).toHaveValue("la oportunidad");
  const before = state.pageReads;
  await page.getByLabel("Example", { exact: true }).fill("This row no longer matches.");
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  await expect(rows(page)).toHaveCount(2);
  expect(state.pageReads - before).toBe(6);
  await expect(rows(page).first()).toHaveAttribute("data-card", "600");
  await expect(rows(page).last()).toHaveAttribute("data-card", "1002");
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Back to results" }).click();
  await expect(page.getByLabel("Search note fields")).toHaveValue("aprovechar"); await expect(rows(page).last()).toBeFocused();
  expect(state.writes[0]!.body.revision).toBe("r1");
});

test("changed search and selection cancel automatic sparse continuation", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3);
  const old = barrier(); state.sparse = true; state.pageGate = old.promise;
  const before = state.pageReads; state.pageGateAt = before + 2;
  await search(page, "aprovechar"); await expect.poll(() => state.pageReads).toBe(before + 2);
  await expect(page.getByRole("status").filter({ hasText: "Loading cards" })).toContainText("1 pages checked");
  await expect(page.getByLabel("Search note fields")).toBeEnabled();
  await expect(page.getByRole("button", { name: "Search", exact: true })).toBeEnabled();
  state.sparse = false; state.pageGate = null; state.pageGateAt = null;
  await search(page, "oportunidad"); await expect(rows(page)).toHaveCount(1);
  expect(state.pages.at(-1)?.query).toBe("oportunidad");
  old.release(); await expect.poll(() => state.releasedPages).toBe(1);
  await readSettled(page, "page", before + 2, "aborted");
  await expect(rows(page)).toHaveCount(1); await expect(rows(page).first()).toContainText("la oportunidad");
  await expect(page.getByRole("status").filter({ hasText: "cards shown" })).toContainText("1 cards shown");
  state.sparse = true; await search(page, "aprovechar");
  await expect(rows(page).first()).toHaveAttribute("data-card", "600");
  await expect(page.getByRole("status").filter({ hasText: "cards shown" })).toContainText("1 cards shown");
  const pending = barrier(); state.pageGate = pending.promise;
  const reads = state.pageReads; state.pageGateAt = reads + 2;
  await page.getByRole("button", { name: "Load more cards" }).click(); await expect.poll(() => state.pageReads).toBe(reads + 2);
  await expect(rows(page).first()).toBeEnabled();
  await rows(page).first().press("Enter"); await expect(page.getByLabel("Spanish", { exact: true })).toHaveValue("el viaje");
  pending.release(); await expect.poll(() => state.releasedPages).toBe(2);
  await readSettled(page, "page", reads + 2, "aborted");
  await expect(page.getByLabel("Example", { exact: true })).toBeEnabled();
  await expect(rows(page)).toHaveCount(1);
  expect(state.pageReads).toBe(reads + 2);
});

test("a committed edit adopts its revision even if detail refresh fails", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await rows(page).first().click();
  state.detailError = true;
  await page.getByLabel("Example", { exact: true }).fill("first committed edit"); await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByRole("alert")).toContainText("Note 101 saved. Could not refresh");
  await expect(page.getByLabel("Example", { exact: true })).toHaveValue("first committed edit");
  await expect(page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Delete note", exact: true })).toBeDisabled();
  await page.getByLabel("Example", { exact: true }).fill("second edit with committed revision");
  await page.getByRole("button", { name: "Save changes" }).click(); await expect.poll(() => state.writes.length).toBe(2);
  expect(state.writes[1]!.body.revision).toBe("r2");
  await expect(page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  state.detailError = false; await page.getByRole("button", { name: "Load server copy" }).click();
  await expect(page.getByRole("button", { name: "Delete note", exact: true })).toBeEnabled();
});

test("creation and deletion stay acknowledged when result refresh fails", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3);
  await page.getByRole("button", { name: "Add note", exact: true }).click(); await page.getByLabel("Spanish", { exact: true }).fill("created once");
  state.pageError = true; await page.getByRole("button", { name: "Create note" }).click();
  await expect(page.getByRole("alert")).toContainText("Note 101 saved. Could not refresh");
  await expect(page.getByRole("button", { name: "Create note" })).toHaveCount(0);
  expect(state.writes).toHaveLength(1);
  state.pageError = false; await search(page, ""); await rows(page).first().click();
  await page.getByRole("button", { name: "Delete note", exact: true }).click(); state.pageError = true;
  await page.getByRole("button", { name: "Confirm delete note" }).click();
  await expect(page.getByRole("alert")).toContainText("Note and its cards deleted. Could not refresh");
  await expect(page.getByRole("button", { name: "Confirm delete note" })).toHaveCount(0);
  await expect(page.getByLabel("Example", { exact: true })).toHaveCount(0);
  expect(state.writes).toHaveLength(2);
});

for (const removeMatch of [false, true]) test(`compact Back preserves a delayed committed replay (${removeMatch ? "removed match" : "updated excerpt"})`, async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3);
  state.sparse = true; await search(page, "aprovechar"); await expect(rows(page)).toHaveCount(1);
  await page.getByRole("button", { name: "Load more cards" }).click(); await expect(rows(page)).toHaveCount(3);
  await page.setViewportSize({ width: 390, height: 844 });
  await rows(page).nth(1).click();
  await page.getByLabel("Example", { exact: true }).fill(removeMatch ? "No longer a match." : "aprovechar the committed excerpt");
  const held = barrier(); const before = state.pageReads; state.pageGate = held.promise; state.pageGateAt = before + 2;
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect.poll(() => state.pageReads).toBe(before + 2);
  const back = page.getByRole("button", { name: "Back to results" }); await expect(back).toBeEnabled(); await back.click();
  await expect(rows(page).nth(1)).toBeFocused();
  held.release(); await expect(page.getByRole("status").filter({ hasText: "cards shown" })).toBeVisible();
  expect(state.pageReads - before).toBe(6);
  await expect(rows(page)).toHaveCount(removeMatch ? 2 : 3);
  await expect(page.getByLabel("Search note fields")).toHaveValue("aprovechar");
  if (removeMatch) { await expect(rows(page).last()).toHaveAttribute("data-card", "1002"); await expect(rows(page).last()).toBeFocused(); }
  else { await expect(rows(page).nth(1)).toContainText("the committed excerpt"); await expect(rows(page).nth(1)).toBeFocused(); }
});

test("another selection preserves committed replay but a new query supersedes it", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3);
  await search(page, "aprovechar"); await rows(page).first().click();
  const held = barrier(); state.pageGate = held.promise; const before = state.pageReads;
  await page.getByLabel("Example", { exact: true }).fill("aprovechar updated after selection");
  await page.getByRole("button", { name: "Save changes" }).click(); await expect.poll(() => state.pageReads).toBe(before + 1);
  await expect(rows(page).nth(1)).toBeEnabled(); await rows(page).nth(1).click();
  await expect(page.getByLabel("Spanish", { exact: true })).toHaveValue("aprovechar");
  await expect(page.getByLabel("Spanish", { exact: true })).toBeFocused();
  held.release(); await expect(rows(page).first()).toContainText("updated after selection");
  await expect(page.getByLabel("Spanish", { exact: true })).toBeFocused();
  const obsolete = barrier(); state.pageGate = obsolete.promise;
  await page.getByLabel("Example", { exact: true }).fill("aprovechar another save");
  const next = state.pageReads; await page.getByRole("button", { name: "Save changes" }).click(); await expect.poll(() => state.pageReads).toBe(next + 1);
  await expect(page.getByRole("button", { name: "Search", exact: true })).toBeEnabled();
  state.pageGate = null; await search(page, "oportunidad"); await expect(rows(page)).toHaveCount(1);
  obsolete.release(); await expect.poll(() => state.releasedPages).toBe(2);
  await readSettled(page, "page", next + 1, "aborted");
  await expect(rows(page)).toHaveCount(1); await expect(rows(page).first()).toContainText("la oportunidad");
});

test("skip link stays clipped while scrolled and becomes visible only for keyboard focus", async ({ page, baseURL }) => {
  await page.setViewportSize({ width: 390, height: 844 }); await fixture(page, baseURL!); await page.goto("/browse"); await rows(page).first().click();
  await page.getByLabel("Example", { exact: true }).focus(); await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  const skip = page.getByRole("link", { name: "Skip to content", exact: true });
  await expect(skip).toHaveCSS("clip-path", "inset(50%)");
  await skip.focus(); await expect(skip).toBeInViewport(); await expect(skip).toHaveCSS("clip-path", "none");
  await page.keyboard.press("Enter"); await expect(page.getByRole("main")).toBeFocused();
  await expect(skip).toHaveCSS("clip-path", "inset(50%)");
});

test("an individually unavailable detail keeps its identity and the next card usable", async ({ page, baseURL }) => {
  const state = await fixture(page, baseURL!); await page.goto("/browse"); await expect(rows(page)).toHaveCount(3);
  state.detailUnavailable = true; await rows(page).first().click();
  await expect(page.getByRole("alert")).toContainText("Card 201 · Note 101. Unavailable");
  await expect(page.getByRole("alert")).toBeFocused();
  await expect(page.getByRole("button", { name: "Save changes" })).toHaveCount(0);
  state.detailUnavailable = false; await rows(page).nth(1).click();
  await expect(page.getByLabel("Spanish", { exact: true })).toHaveValue("aprovechar");
});
