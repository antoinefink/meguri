// SPDX-License-Identifier: MIT
import test from "node:test";
import assert from "node:assert/strict";
import { ApiError, parseBrowse } from "../src/api.js";
const card = { card_id: "9007199254740993", note_id: "42", title: "<script>literal</script>", field: "Example", excerpt: [{ text: "[a.b]", matched: true }], template: "Forward", deck: "Spanish" };
test("Browse preserves exact IDs and typed inert highlight text", () => {
  const page = parseBrowse("page", { account_id: "account", cards: [card], next_cursor: "next", examined_through: card.card_id }, "account");
  assert.equal(page.cards[0]!.card_id, "9007199254740993");
  assert.equal(page.examined_through, "9007199254740993");
  assert.deepEqual(page.cards[0]!.excerpt, [{ text: "[a.b]", matched: true }]);
  assert.equal(page.cards[0]!.title, "<script>literal</script>");
  assert.throws(() => parseBrowse("page", { ...page, examined_through: 9007199254740993 }, "account"));
  const unavailable = { ...card, unavailable: "Fields were not searched." };
  assert.equal(parseBrowse("page", { ...page, cards: [unavailable] }, "account").cards[0]!.unavailable, unavailable.unavailable);
});
test("Browse rejects mismatched accounts before interpreting collection records", () => {
  for (const kind of ["catalog", "page", "detail", "create", "edit", "delete"] as const) {
    assert.throws(() => parseBrowse(kind, { account_id: "other" }, "account"), (e: unknown) => e instanceof ApiError && e.code === "stale_authority");
  }
  for (const broken of [null, {}, { account_id: "account", cards: [{ ...card, card_id: 9007199254740993 }], next_cursor: null, examined_through: "9007199254740993" },
    { account_id: "account", cards: [{ ...card, excerpt: [{ text: "word", matched: "true" }] }], next_cursor: null, examined_through: "9007199254740993" }]) {
    assert.throws(() => parseBrowse("page", broken, "account"), (e: unknown) => e instanceof ApiError && e.code === "invalid_response");
  }
});
test("Browse detail checks note/card relationship and required field names", () => {
  const detail = { account_id: "account", card, note: { note_id: "42", note_type_id: "10", fields: { Example: "raw <b>markup</b>" }, tags: [], revision: "r" }, field_names: ["Example"], sibling_ids: [card.card_id], confirmation: "c" };
  assert.deepEqual(parseBrowse("detail", detail, "account"), detail);
  assert.throws(() => parseBrowse("detail", { ...detail, field_names: ["Absent"] }, "account"));
  assert.throws(() => parseBrowse("detail", { ...detail, sibling_ids: [] }, "account"));
});

test("Browse transport binds each operation to the displayed account without retries", async t => {
  const { browseApi } = await import("../src/api.js");
  const seen: string[] = [];
  t.mock.method(globalThis, "fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input); seen.push(path);
    assert.equal(init?.method, "POST"); assert.equal(init?.credentials, "same-origin"); assert.equal(init?.cache, "no-store");
    assert.equal(new Headers(init?.headers).get("X-Meguri-Expected-Account"), "account");
    assert.equal(new Headers(init?.headers).get("Content-Type"), "application/json");
    return Response.json({ data: { account_id: "account" } });
  });
  await browseApi("delete", { note_id: "42", revision: "r", confirmation: "c" }, "account");
  assert.deepEqual(seen, ["/api/me/browse/delete"]);
});
