# Browser accounts

- Exact shadcn input URLs/hashes/adaptations live in `upstream-records.json`; retain the MIT notice. Never export acquisition scratch.
- Entry fragment handling must remain import-free and precede React/bootstrap I/O. Never persist credentials or add request retries.
- Consent uses `#request=<43-character base64url handle>`; clearing it must precede bootstrap just like `#token`. A lost bound session requires a fresh client request.
- `tests/browser/mcp.spec.ts` intercepts synthetic API/callback responses over the compiled fixture; it proves UI states, not backend OAuth authority.
- Every file here is pinned in `scripts/audit-assessment.json`: after an edit, refresh that file's `bytes`/`sha256` entry and rebuild with `tools/mg sync-web-image`, or `sync-web-deps --install` fails ("differs from assessment") and the checks fail on a missing `/deps/package-lock.json`.
- Build only through the `sync-web-*` MG jobs. Resolved lock, native compiler package graph, licenses/advisories, browser fixture and client instructions require integration evidence.
- Generated bundles/manifests belong in `build/` and Cargo OUT_DIR, never tracked here or in webassets.
