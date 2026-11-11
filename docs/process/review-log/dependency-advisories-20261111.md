# Dependency gate repairs — 2026-11-11

Role: fixer. Start: `f3ec1b0f`. Integration base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

`core-deny/20261111T161210Z-26228` rejects three advisories. The graph now uses
libsqlite3-sys 0.39.1 through rusqlite 0.41.1, pulldown-cmark 0.13.6 and
rustix 1.1.6. None is an oracle parity pin. Features, parity pins and denial
policy remain unchanged. Historical version measurements remain historical.

RustSec identifies the patched SQLite binding and parser releases in
[RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html) and
[RUSTSEC-2026-0433](https://rustsec.org/advisories/RUSTSEC-2026-0433.html).
The original rustix advisory and its patched range are retained in the failed
MG run's cargo-deny artifact.

Filtered author `cleanroom/20261111T161326Z-27442` changes two manifest
requirements. MG updates `161508Z-28493` and `161601Z-28838` generate the
lockfile. The actual locked rusqlite version is 0.41.1; the manifest permits
it. Exactly four package versions change. Resolution also changes Windows
dependency edges for errno, quinn-udp, rustix, rustls-platform-verifier, tempfile
and winapi-util. No Windows package version is added or removed.

Independent review `161747Z-30968` finds no P0–P2 issues in the final graph.
The host compares the lockfile against the base and corrects two parent names
in the review prompt. The reviewer reads the final files but has no base
history or registry access. Host compilation validates the resolved graph.

Lint `core-lint/20261111T162106Z-35404` finds one redundant iterator conversion
with the new SQLite wrapper. Filtered fixer `162231Z-36697` removes only that
conversion in storage raw queries. Independent review `162526Z-40262` finds
no P0–P2 issues. Column order and owned strings remain unchanged. No assertion,
lint suppression, retry or time limit changes.

Verification:

- Before the API adjustment, `core-test/20261111T161720Z-30684` passes all
  404 storage, text and R1 regression tests.
- Final source `bda5cffd`: `core-test/20261111T163123Z-46523` passes the same
  404 cases. Lint `162909Z-43451` passes.
- Full dependency policy `core-deny/20261111T162109Z-35678` passes. The API
  adjustment leaves its manifest and lockfile inputs unchanged.
- Staged privacy `161651Z-29869` and `162340Z-37597` are retained separately.
  The final evidence scan must pass before commit. Similarity
  `20261111T162705Z-41739` passes for the changed first-party storage file.
- Signed Mac build `20261111T162522Z-39308` passes with all three native Rust
  slices rebuilt. The final iPhone app build and wider integration gate remain
  pending. Gate `161810Z-31484` is a plan, not executed verification.

No unit, milestone or full gate is accepted.
