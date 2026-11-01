# Media cleanup source delivery

Base: `3f241c19b77db754b71c0d8c433f25fd674903ae`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Seed author: implementer `s-20261025T134357Z-235af9a2`.
Repair author: fixer `s-20261025T162405Z-9dc656e4`.
Authority: K9.5, P10.3, P24 and Q11. No contract changes.

This delivery retains the complete 48-path input and its existing assertions.
It is source for integration, with no acceptance result. The runner performs
no builds, test execution, network access or repository-history operations.
The integrator owns formatting, Rust/native compilation, generated bindings,
focused execution, independent review and the required gates.

The implementation records reference age from live local and acknowledged
versions. ACK and apply writes preserve the earlier materialized payload while
updating tracking. Cleanup computes within one IMMEDIATE transaction; committed
unlink evidence is replayed afterward. A9b remains disabled. Media deletion uses
the original engine batch, delivery admission and callback categories. Durable
Deleted state requires the original deletion result or accepted UnknownItem.
No own-operation delete or substitute pending/ACK result is provided.

R1 repairs:

1. Eligibility reads unresolved media copies across ordinary and recovered
   owners. Each copy uses its owner's stored record key; missing or unreadable
   authority withholds deletion. Foreign owners remain outside ordinary binding
   selection and writes. Resolved copies retain their existing expiry rules.
   The same eligibility function governs computation, batch selection and final
   queue admission.
2. The Driver retains the accepted drain receipt with the exact issued command.
   Runtime consumes that original SDK, backend guard and timestamp once. Clock,
   file and account observations happen before Runtime/Driver locks in this port.
   Delayed or backward observations cannot renew the one-hour window. Failure
   does not advance daily success, and inventory listing does not refresh marks.
   The direct publication helper also propagates clock errors and rejects stale
   receipts. Native RunCleanup remains explicitly unavailable until retained
   inventory integration is implemented in the next checkpoint.
3. Swift forwards the whole original sent receipt to Core before classifying
   continuation. A validated UnknownItem delete remains diagnostic and permits
   remaining media work. Save failures and other delete failures still stop it.
4. Swift computes the reported server delay from both original failure arrays.
   Per-record code, zone, name and delay remain available in its diagnostic.
   A subsequent normal absence callback cannot clear an earlier blocking error.

The existing testkit module import was corrected to use `meguri_sync::cloud`.
A second seed mismatch was repaired: Rust reads failed-record zone metadata,
so its FFI record and the Swift raw mapping now carry zone, native code and
retry delay. The existing Rust result literals were updated consistently.
The affected custom helper identifier and complete comment/prose regions were
rewritten. No privacy search, guard alteration or allowlist exception was used.
These edits do not establish a privacy or compilation pass.

New regression source:

- `cloud/media/cleanup.rs`: ordinary isolated, other-build and actual recovery
  owners; missing decoding authority; eligibility, due clearing and
  resolved-copy behavior. Copy inputs are typed storage fixtures, not UI proof.
- `meguri-ffi/src/cloud_media_tests.rs`: fixture callbacks forward test Service
  records to Core and obtain Driver-issued commands. The source calls
  `Driver::compute_qualified_local_cleanup` for delayed-command, invalidation
  and foreign-copy cases, then checks batch selection and queue admission.
  These assertions have not run here and establish no positive Driver or
  native deletion result.
- `meguri-sync/tests/cloud_media_cleanup.rs`: the source asserts that an
  unresolved copy in another build prevents deletion, then updates the stored
  copy and expects deletion through the test SDK. Its existing helpers issue
  Driver commands but do not call `Driver::compute_qualified_local_cleanup`;
  this source therefore does not establish positive coverage of that port.
- `CoreBridgeTests/CloudMediaTests.swift`: accepted-receipt continuation/error
  classification, deletion-only and mixed delays, real policy delay facts, and
  the raw metadata mapping, and an original fake SDK callback rejected by real Core when it reports a delete
  for a handed save. Successful native deletion continuation still requires an
  integrated positive callback proof; the Rust Service UnknownItem test alone
  does not supply that Swift evidence.

The owner/storage, Driver and Swift regression groups were written before their
corresponding behavior edits. The additional cross-owner Service and FFI stage
cases were added afterward. None has execution evidence in this runner.

Known results remain: the original tests-first Service deletion assertion failed
in core-test070033; the quota cases stopped earlier in drive_budget with
RuntimeIncompleteRootConfirmationAndSending. Root's
`core-test/20261101T073720Z-69112` stopped during compilation on the testkit import.
No result is replaced by the source corrections here. The enabled application
review/media proof remains red; full reviews and mediaCloudKit are unaccepted.
Required next work includes native retained inventory, positive Swift deletion
continuation, full application Service evidence and milestone/final gates.

| Check | Result |
| --- | --- |
| cleanroom/20261101T075956Z-76519 | Source exported |
| core-test/20261101T075807Z-75267 | Compile failed |
| core-format/20261101T080210Z-77185 | Pass |
| core-test/20261101T080235Z-77497 | 69 pass, 11 fail; FFI filter matched none |
| core-test/20261101T080537Z-79006 | 59 pass, 2 fail; FFI media_port selected |
| private/20261101T080500Z-78633 | Notes pass; source scan 080239 had only notes flagged |
| similarity/20261101T075815Z-75899 | Pass before final source edits |

The final source pass adds cleanup_drain: None to STOP commands and changes
test names and comments. Test assertions stay enabled. Native cleanup and the
full app sync tests remain open. No Unit trailer is added.
