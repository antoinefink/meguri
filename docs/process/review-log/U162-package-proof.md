# U162 — Offline package proof

Supplemental start: `7b35de76`. Original integration base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Bounded diagnostics

Filtered exports `cleanroom/20261026T084508Z-93203` and
`cleanroom/20261026T085821Z-2515` change only the verifier and its regression.
Independent R1 and R2 are clear. Exact comparisons, bounds, collation refusal,
cleanup and successful outputs stay unchanged. Failures identify fixed schema
positions and reasons. Only mismatched keys gain UTF-8 SHA-256 fingerprints;
raw keys, values, SQL and exception text remain absent.

| Check | Receipt | Result |
|---|---|---|
| Focused Docker regression | `selftest/20261026T084917Z-96389` | Pass |
| Final regression, including key digests and redaction | `selftest/20261026T090114Z-4704` | Pass |
| Final two-file privacy scan | `private/20261026T090114Z-4936` | Pass |

## File proof remains open

Original `package-interop-check/20261026T002734Z-14327` fails before a client opens.
New runs `20261026T085131Z-98116` and `20261026T090245Z-6336` retain that failure.
The first difference is a config-table key at table 11, row 12.

Digest matches against public Meguri constants identify expected `newSpread`
and decoded `localOffset`. The generated baseline omits `localOffset`.
The native day query must add it at the pinned non-UTC clock (day-clock D5).
Fixture preparation needs correction before launch; no comparison is waived.
Further differences remain unproven. This is active offline file compatibility,
not frozen server work. No Unit trailer or full gate acceptance is claimed.

Final tooling similarity `20261026T090115Z-5133` passes for both files.
