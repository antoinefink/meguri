# Rootless media fixture epoch authority

Start: `5a63304a`. Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Authority: `sync/policy#P6`, `#P12.3`, `sync/cloudkit-recovery#V26.1`.

The two media tests failed before their first ACK. Clean-start comparison
`core-test/20261113T112218Z-88211` returned an empty batch for their real pending
MediaName, where each test required one save. The fixture confirmed Active but
never accepted the bound MGEpoch. Qualified epoch crt stayed unknown, so the
production send gate correctly refused the batch.

Only the test helper changes. It now proves valid SDK/root authority, unknown
epoch authority, a pending MediaName and a blocked id-zero batch. An original
Core-issued preparation capability reads the existing Service MGEpoch. Core
accepts the actual encrypted bytes and system fields and reports Ready. Checks
require unchanged adapter/durable state, no publication row, no blob, no new
drain or absence completion, and persistent accepted authority after reopen.
Runtime validation supplies the new date fact; the product's source stays
byte-identical to the starting commit.

Every original assertion remains: one initial save with the expected name,
ACK revision equality, cleared mark, retained envelope, paused media, exact
uploaded bytes, reopen, stale-SDK rejection, both drain modes, false absence,
and delayed deletion. The simulated clock still requires Present at 24 hours
minus one millisecond and Unknown at the exact boundary, followed by re-upload.
This simulated clock is not human trial time.

## Review and verification

Author `cleanroom/20261113T113733Z-36001` exported the setup. Read-only semantic
review `20261113T114050Z-38933` found no P0/P1/P2. Compilation in
`core-test/20261113T114104Z-39427` then rejected a helper imported from the
wrong module; no tests ran. Fixer `cleanroom/20261113T114320Z-42149` corrected
only that path. Independent delta review `20261113T114426Z-43662` found no
P0/P1/P2 and verified the helper preserves actual Service bytes and fields.
The source comparison confirms every other patch byte remains unchanged.

- `core-test/20261113T114439Z-44138`: four tests passed, zero skips, in the
  original media regression binary. Both previously blocked rootless cases
  now exercise their full bodies. The two feed-consumer cases also pass.
- `core-lint/20261113T114547Z-46157`: 3/3 correctness and layering checks pass.
- `private/20261113T114515Z-44726`: the final source file scanned successfully.
- `similarity/20261113T114517Z-44992`: one passed, zero exemptions.
- `static-checks/20261113T114520Z-45276`: 3/3 passed.

The passing test command adds `--test cloud_media_rootless_ack` before the
nextest filter. Cargo builds that integration target; all four of its cases
still run. It avoids compiling unrelated integration targets for this focused
check. No native build is needed for this test-only Rust change.

The final staged privacy receipt is retained in local build evidence and commit
metadata. The old failed receipts remain. This fixes only the two initial media
fixture failures; it does not clear the separate native drain-count failures,
other recorded failures, the full pending batch gate, live peer transfer or
playback checks. No Unit, P24, milestone, trial or release acceptance is claimed.
