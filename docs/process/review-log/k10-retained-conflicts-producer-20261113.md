# Retained conflict attention producer

Start: `71e68bd5`. Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Authority: `sync/policy#P14.4`, `sync/cloudkit-recovery#V8.1` and
`sync/cloudkit-records#K10`.

Policy now counts finalized retained damaged sources through the existing
TrackingStore API, under State then tracking access. The item is available
without an ordinary binding. WALs and salvage scratch do not count twice.
Metadata errors propagate; source contents are not read. No recovery, retention,
schema, ABI or Swift behavior is changed. The attention-completeness guard
remains false: this producer does not establish a fully clean sync status.

Host tests use actual synthetic SQLite/WAL recovery from a foreign scope.
They preserve one readable conflict and retain a source containing two
unreadable conflicts. Runtime projection and Host status count one kept source,
with and without an ordinary binding, across reopen. Unsafe identity must fail
projection. Retained main/WAL bytes stay unchanged. Complete salvage leaves no
unreadable item and supplies no readiness claim. No Summary or attention map
was injected into the tests.

Filtered author: `cleanroom/20261113T104125Z-70865`.
Independent source review: `cleanroom/20261113T104740Z-76528`, no P0/P1/P2.

- `core-test/20261113T104756Z-76989`: all 39 affected Host policy-driver tests
  passed, zero skips.
- `core-lint/20261113T105116Z-80839`: correctness and layering checks passed 3/3.
- `static-checks/20261113T105114Z-80163`: passed 3/3.
- `similarity/20261113T105115Z-79894`: both Rust files passed, zero exemptions.
- `private/20261113T105133Z-81682`: both source files scanned successfully.
  Earlier `20261113T105113Z-79895` scanned zero files after the sandbox refused
  staging, so it supplies no source coverage. The corrected explicit scan does.

The older producer map now points to this completed projection and marks its
old epoch-crt observation as historical. Its other gaps remain unclaimed.
The new producer is not yet demonstrated in the running signed app. Remaining
K10 producers, recorded broader failures, the full pending gate and cross-device
verification remain due. No Unit, milestone or release acceptance is claimed.
