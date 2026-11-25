// SPDX-License-Identifier: MIT
import test from "node:test";
import assert from "node:assert/strict";
import { canonicalUsername, passwordError, usernameError, navigation, protectedPath } from "../src/policy.js";
test("username rules match the HTTP account boundary without Unicode trimming", () => {
  assert.equal(canonicalUsername(" \tA.B-c_1\r\n"), "a.b-c_1");
  assert.equal(usernameError("abc"), undefined);
  assert.equal(usernameError("a".repeat(64)), undefined);
  for (const value of ["ab", "a".repeat(65), ".abc", "a/b", "ééé", "a b", "abc\0", "\u00a0abc", "\vabc"]) assert.ok(usernameError(value));
});
test("new passwords count UTF-8 bytes exactly and preserve spaces and normalization", () => {
  for (const value of [" 1234567890 ", "é".repeat(6), "é".repeat(512), "a".repeat(1024), "e\u0301".repeat(4)]) assert.equal(passwordError(value), undefined);
  for (const value of ["12345678901", "é".repeat(513), "abc\0abcdefghijk", "a".repeat(1025), "é".repeat(5)]) assert.ok(passwordError(value));
  assert.equal(passwordError("short"), "Make your password longer.");
  assert.equal(passwordError("a".repeat(1025)), "Make your password shorter.");
  assert.equal(passwordError("abc\0abcdefghijk"), "Remove the unsupported character from your password.");
  const value = "  e\u0301 A long password  ";
  assert.equal(passwordError(value), undefined);
  assert.notEqual(value, value.normalize("NFC"));
});

test("Browse is an exact protected account navigation route", () => {
  assert.ok(navigation.includes("/browse"));
  assert.equal(protectedPath("/browse"), true);
  assert.equal(protectedPath("/browse/"), false);
});
