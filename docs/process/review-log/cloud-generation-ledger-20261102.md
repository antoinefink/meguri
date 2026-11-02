# CloudKit generation ledger foundation

Status: partial U113; no Unit acceptance. Component start: a12118d3.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Integration base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.

Implementer264d authored filtered12689, with tests before implementation.
The new ledger persists local generation facts in SQLite and implements map
composition, claim order and whole joins on retry or loss. This is a storage
and transition foundation, not an operational generation synchronization path.

Independent review found two P2 issues: shared-member joins were missing,
and malformed map input could allocate by a large declared template count.
Fixer4626 repaired both in filtered19572. Review cleared them. Focused core
run22984 passed41/41 with zero skips after formatting.

Scan23744 failed. Patch25488 changed only prose and
local names; independent comparison found no behavior or assertion change.
One comment still matched in29519. Filtered30553 removed its two lines only;
review verified hashes and unchanged executable/test bytes. Privacy checks
then passed, with no guard changes or exceptions. Final core-test33290 again
passes41/41, zero skips. The generation runtime, codec and tracking binaries
are covered; no full Q11 two-device apply/rollback result is claimed.

Core-lint34089 failed two checks: new test formatting and an existing nested
if in workflow_runtime.rs. MG formatting has corrected the test layout.
The smoke-runtime clippy finding remains open for a separate filtered repair.
No assertion, limit, test or checker has been disabled.

Still required before U113 acceptance: writer/WAL operation feed, fetched
manifest and asset intake, whole claim construction and results, completion,
member sending, remote adoption and rollback, generation status, recovery and
cleanup integration, large-member performance evidence and the complete Q11
scenarios. The codec and ledger tests do not substitute for those workflows.
No P24 item, gate or milestone is closed by this foundation commit.
