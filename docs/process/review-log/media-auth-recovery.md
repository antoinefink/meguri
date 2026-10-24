# Media authentication recovery

Start: `9a3162b49acfeff8e73c0e1dfc5e29372df81b49`.
Fixer session: `s-20261024T174115Z-511e3ad6`.
Contracts: `sync/login-endpoints#L6`, `sync/errors#E8`.

Final sync integration review found one P2. A media request can return an
operation record with `media.failureKind == "auth_failed"` rather than throw.
The coordinator treated it as a generic partial transfer and retained the
rejected key in memory and Keychain. Restart could reload that rejected key.
The partial operation and completed media effects must remain available.

The review found no other P0–P2 issue in the traced app, bridge, FFI, host,
one-way transfer and policy paths. Runtime and full-gate acceptance remain open.

Gate `20261024T182152Z-3548` was cancelled after this finding, during its
canary stage, so the final run can check the repaired source. Its cancellation
receipt is retained; it is not a passing gate. The required base remains
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

Filtered export `cleanroom/20261024T182839Z-31417` passed its boundary and
source consistency checks. The coordinator now handles the exact returned
authentication failure before leaving the media stage. It clears credentials
through the shared invalidation helper, preserves the operation and media
effects, and requests login. History restoration is unchanged, so an old failure
cannot clear a later successful login.

Independent round 2 found the original P2 closed and no new P0–P2 issues.
The regression covers nine failure kinds, retained partial effects, restart
without the rejected key and a later saved login surviving the old history.

Focused verification passed:

- `swift-test/20261024T183057Z-32934`: all 40 tests in the transfer settings and
  collection startup suites, no skips.
- `private/20261024T183059Z-33389`: three files, no private-token finding.
- `similarity/20261024T183101Z-33928`: two source files, no flags or allowlist.
- `static-checks/20261024T183133Z-34580`: passed.

