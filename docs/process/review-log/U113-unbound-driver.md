# U113 unbound publication driver

Status: tests-first contracts are enabled and RED; production remains pending.
Unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Phase start: `af2cee18745c64d2cf36ef1dfbf5c7ede88428b0`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Producer `cleanroom/20261029T073650Z-86717` exports a real unbound child driver,
shared SDK scheduling and generation-specific restore receipts. Its two new
contract files are extracted before production. Independent R1 clears the tests:
unbound discovery draws no keys or baseline; the second failed key draw leaves
no publication; automatic publication does not change the saved preference;
prepared and submitted Active differ durably; retained package rows stay
independent of live rows; restore receipts belong to the actual generation,
including successive restores and missing provenance.

- `core-format/20261029T080043Z-8465`: pass.
- `core-test/20261029T080114Z-8955`: expected compile RED on missing
  `PublicationRig`, `Point` and `replacement_evidence_for`. No case is counted
  as passing. Both new targets are selected.
- `private/20261029T080115Z-9210`: first run fails on custom test identifiers.
  Fixer `cleanroom/20261029T080301Z-10482` changes only a test name and a local
  variable. Independent equivalence review is clear; assertions stay enabled.
- `private/20261029T080415Z-11432`: final two files pass.
- `similarity/20261029T080415Z-11231`: two files, no findings or exemptions.

The original nine publication contracts are unchanged and enabled. Production
review has identified a held transport/close defect to repair before integration.
Adoption, nonempty media and intent execution remain missing in the exported
phase. No delivery, Unit, P24 or integration-gate acceptance is claimed.

## Submission regressions, before the repairs

R1 found two P1 defects: the submission reader stayed held while waiting for a
reply, blocking Close and Restore; and cleanup used an Active read from before a
hold. It also found P2 batch semantics: the shared test adapter sent ordinary
batches atomically, unlike the native adapter.

Fixer `cleanroom/20261029T080643Z-12547` separates queue acceptance from reply
waiting, moves the cleanup hold before the read, and uses non-atomic ordinary
batches. Its partial-result regression also exposed a successful ACK lost on an
early error. The repair persists the actual successful results first.

The new `cloud_publication_submission.rs` is extracted before production.
Independent review clears its real lost-reply close/restart case, two-device
late switch and durable partial-result assertions. The ordinary-adapter case
uses the existing prebound fixture; both publication cases remain unbound.

- `core-format/20261029T081851Z-17027`: pass.
- `core-test/20261029T081909Z-17375`: expected compile RED on missing Rig and
  Point, before production. No executed pass is claimed.
- `private/20261029T081911Z-17856`: first run flags one comment. Fixer
  `cleanroom/20261029T081952Z-18681` removes that comment only; independent
  equivalence review is clear.
- `private/20261029T082138Z-19558`: final file passes.
- `similarity/20261029T082139Z-19787`: one file, no findings or exemptions.

Production R2 clears the lease, queue, ACK and batch repairs. One P1 remains:
multiple cleanup targets still share a read across an earlier deletion reply
wait. A two-zone interleaving and fresh authorization are required before this
source can be integrated. The previous delivery gaps remain open.

## Two-zone cleanup regression, before production

Fixer `cleanroom/20261029T082440Z-21069` retains the first actual Service reply
in the parent and delivers it on resume without a second submission. Each later
deletion reads and checks Active again before its enqueue. Independent R3 finds
no remaining P0-P1 or new P2 in this repair. The previous production files and
committed tests remain unchanged outside the two driver files.

Its new `cloud_publication_cleanup.rs` is independently reviewed and extracted
first. Three unbound hosts create two due zones. While the first deletion reply
is held, another publisher promotes the remaining zone. Assertions require the
first result to persist and the new Active zone and its due mark to survive,
including reopen. Actual Service logs and outcomes constrain the hold.

- `core-format/20261029T082945Z-22987`: pass.
- `core-test/20261029T082957Z-23579`: expected compile RED on missing Rig and
  Point before production. No executed pass is claimed.
- `private/20261029T082956Z-23311`: one file passes.
- `similarity/20261029T082956Z-23484`: one file, no findings or exemptions.

The reviewer also clears a behavior-RED stage with only the fresh-read additions
removed, keeping the actual reply hold and all test assertions. That stage still
needs compilation and execution. Full delivery and integration remain open.
