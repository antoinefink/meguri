// SPDX-License-Identifier: MIT
import test from "node:test";
import assert from "node:assert/strict";
import { api, ApiError, errorMessage, parseBackupHistory } from "../src/api.js";
test("W3 requests use exact envelopes, same-origin cookies, no-store and no automatic reissue", async t => {
  const cases: [string, object | undefined, unknown][] = [
    ["/api/status", undefined, { setup_required: true }],
    ["/api/me", undefined, { id: "opaque-id", username: "member", role: "member", server_url: "https://sync.test" }],
    ["/api/me/mcp-grants", undefined, { account_id: "opaque-id", grants: [{ id: "grant-id", client_name: "Synthetic client", access: "read", created_at: "2026-11-19T00:00:00Z" }] }],
    ["/api/me/mcp-grants/grant-id/revoke", {}, { account_id: "opaque-id" }],
    ["/api/mcp/authorization/read", { request: "synthetic" }, { username: "member", client_name: "Synthetic client", redirect_host: "callback.test", access_options: ["read", "read_write"] }],
    ["/api/mcp/authorization/decide", { request: "synthetic", decision: "read" }, { redirect_to: "https://callback.test/done" }],
    ["/api/mcp/authorization/decide", { request: "synthetic", decision: "read_write" }, { redirect_to: "https://callback.test/done" }],
    ["/api/mcp/authorization/decide", { request: "synthetic", decision: "deny" }, { redirect_to: "https://callback.test/denied" }],
    ["/api/admin/accounts", undefined, { accounts: [] }],
    ["/api/setup", { token: "synthetic", username: "admin", password: " exact password " }, { id: "opaque-id", username: "admin", role: "admin" }],
    ["/api/invitations/claim", { token: "synthetic", username: "member", password: " exact password " }, { id: "opaque-id", username: "member", role: "member" }],
    ["/api/login", { username: "member", password: "old" }, { id: "opaque-id", username: "member", role: "member" }],
    ["/api/logout", {}, null], ["/api/password", { current_password: "old", new_password: "new password exact" }, null],
    ["/api/admin/invitations", {}, { url: "synthetic", expires_at: "synthetic" }],
    ["/api/admin/accounts/opaque-id/reset", {}, { url: "synthetic", expires_at: "synthetic" }],
    ["/api/admin/accounts/opaque-id/disable", {}, null],
    ["/api/resets/claim", { token: "synthetic", new_password: "new password exact" }, null]
  ];
  for (const [path, body, result] of cases) {
    let calls = 0;
    const stub = t.mock.method(globalThis, "fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls++;
      assert.equal(input, path); assert.equal(init?.credentials, "same-origin"); assert.equal(init?.cache, "no-store"); assert.equal(init?.redirect, "error");
      assert.equal(init?.method, body === undefined ? "GET" : "POST");
      assert.equal(init?.body, body === undefined ? undefined : JSON.stringify(body));
      if (body !== undefined) assert.equal(new Headers(init?.headers).get("content-type"), "application/json");
      return Response.json({ data: result }, { status: path.includes("claim") || path === "/api/setup" || path === "/api/admin/invitations" ? 201 : 200 });
    });
    assert.deepEqual(await api(path, body), result); assert.equal(calls, 1); stub.mock.restore();
  }
  let failures = 0;
  t.mock.method(globalThis, "fetch", async () => { failures++; throw new Error("secret server detail"); });
  await assert.rejects(api("/api/resets/claim", { token: "synthetic" }), error => error instanceof ApiError && error.code === "connection_lost" && !error.message.includes("secret"));
  assert.equal(failures, 1);
});
test("structured field errors stay field linked, raw responses never enter messages", async t => {
  t.mock.method(globalThis, "fetch", async () => Response.json({ error: { code: "validation_failed", field: "new_password", detail: "must never echo" } }, { status: 422 }));
  await assert.rejects(api("/api/password", {}), error => {
    assert.ok(error instanceof ApiError); assert.equal(error.field, "new_password"); assert.equal(error.status, 422);
    assert.ok(!errorMessage(error).includes("must never echo")); return true;
  });
  const message = errorMessage(new ApiError("connection_lost"), true);
  assert.match(message, /may already have completed/); assert.match(message, /Try signing in/);
  assert.ok(!errorMessage(new ApiError("submitted-secret")).includes("submitted-secret"));
  assert.match(errorMessage(new ApiError("sole_administrator")), /sole enabled, ready administrator/);
});
test("malformed envelopes fail closed without a second fetch", async t => {
  let calls = 0;
  t.mock.method(globalThis, "fetch", async () => { calls++; return Response.json({ unexpected: "secret" }); });
  await assert.rejects(api("/api/me"), error => error instanceof ApiError && error.code === "invalid_response");
  assert.equal(calls, 1);
});
test("backup history validates complete wire rows before rendering", () => {
  const row = { id: "opaque /?% identifier", created_at: 0, kind: "daily", size_bytes: 0 };
  const history = (backups: unknown) => ({ account_id: "account-a", backups });
  assert.deepEqual(parseBackupHistory(history([]), "account-a"), []);
  assert.throws(() => parseBackupHistory({ account_id: "account-b", backups: [] }, "account-a"),
    error => error instanceof ApiError && error.code === "stale_authority");
  for (const value of [[], null, {}, { backups: [] }, { account_id: "", backups: [] }]) {
    assert.throws(() => parseBackupHistory(value, "account-a"), error => error instanceof ApiError && error.code === "invalid_response");
  }
  for (const kind of ["daily", "weekly", "monthly", "before_restore"]) {
    for (const created_at of [-8640000000000, 0, 8640000000000]) {
      const value = [{ ...row, kind, created_at, size_bytes: Number.MAX_SAFE_INTEGER }];
      assert.deepEqual(parseBackupHistory(history(value), "account-a"), value);
    }
  }
  for (const value of [null, {}, [null], [1], [row, row],
    ...["", null, 12].map(id => [{ ...row, id }]),
    ...["yearly", "toString", null, 1].map(kind => [{ ...row, kind }]),
    ...[null, "0", 0.5, NaN, Infinity, 8640000000001, -8640000000001].map(created_at => [{ ...row, created_at }]),
    ...[null, "0", -1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1].map(size_bytes => [{ ...row, size_bytes }]),
    [{ id: row.id }],
  ]) assert.throws(() => parseBackupHistory(history(value), "account-a"), error => error instanceof ApiError && error.code === "invalid_response");
});

test("backup requests bind the displayed account while restore keeps its empty JSON body", async t => {
  const calls: string[] = [];
  t.mock.method(globalThis, "fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
    calls.push(String(input));
    assert.equal(new Headers(init?.headers).get("x-meguri-expected-account"), "account-a");
    assert.equal(init?.body, calls.length === 1 ? undefined : "{}");
    return Response.json({ data: calls.length === 1 ? { account_id: "account-a", backups: [] } : { account_id: "account-a" } });
  });
  await api("/api/me/backups", undefined, undefined, "account-a");
  await api("/api/me/backups/point/restore", {}, undefined, "account-a");
  assert.deepEqual(calls, ["/api/me/backups", "/api/me/backups/point/restore"]);
});
