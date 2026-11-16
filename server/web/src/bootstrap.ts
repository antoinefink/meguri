// SPDX-License-Identifier: MIT
// This entry has no imports: fragment removal precedes loading React or bootstrap I/O.
export function takeFragment(location: Pick<Location, "hash" | "pathname" | "search">, history: Pick<History, "replaceState">): string {
  let fragment = location.hash;
  if (fragment) history.replaceState(null, "", location.pathname + location.search);
  const params = new URLSearchParams(fragment.slice(1));
  fragment = "";
  const tokens = params.getAll("token");
  return tokens.length === 1 && /^[A-Za-z0-9_-]{43}$/.test(tokens[0] ?? "") ? tokens[0]! : "";
}
let token = takeFragment(window.location, window.history);
export function consumeInitialToken(): string { const value = token; token = ""; return value; }
void import("./mount").then(({ mount }) => {
  mount();
}).catch(() => {
  token = "";
  document.getElementById("root")!.textContent = "Meguri Sync could not load. Reload this page. If a link was cleared, reopen the original invitation or reset link.";
});
