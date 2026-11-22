// SPDX-License-Identifier: MIT
export function canonicalUsername(value: string): string {
  return value.replace(/^[\x09\x0a\x0c\x0d\x20]+|[\x09\x0a\x0c\x0d\x20]+$/g, "").replace(/[A-Z]/g, c => c.toLowerCase());
}
export function usernameError(value: string): string | undefined {
  return /^[a-z0-9][a-z0-9_.-]{2,63}$/.test(canonicalUsername(value)) ? undefined : "Use 3–64 letters, numbers, dots, underscores or hyphens, starting with a letter or number.";
}
export function passwordError(value: string): string | undefined {
  const size = new TextEncoder().encode(value).byteLength;
  if (value.includes("\0")) return "Remove the unsupported character from your password.";
  if (size < 12) return "Make your password longer.";
  if (size > 1024) return "Make your password shorter.";
  return undefined;
}
export const navigation = ["/", "/setup", "/login", "/invite", "/reset", "/connection", "/collection", "/settings", "/accounts", "/oauth/consent", "/mcp/upload"] as const;
export function protectedPath(path: string): boolean { return ["/connection", "/collection", "/settings", "/accounts"].includes(path); }
