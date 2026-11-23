// SPDX-License-Identifier: MIT
import test from "node:test";
import assert from "node:assert/strict";
import { api, ApiError, errorMessage } from "../src/api.js";
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
