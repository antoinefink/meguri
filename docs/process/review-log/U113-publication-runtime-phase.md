# U113 publication runtime phase

Status: WIP; focused behavior verified, lint and workflow gaps remain.
Production applied after tests-first commit `a17059d2`.
Original unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The runtime owns discovery, confirmation, independent full-width keys, durable
publication steps, initial unacknowledged rows, real save results and conditional
Active switching. Registered recovery authority guards decision writes. Current
account and file facts are checked before transport submission. No test harness
selects policy outcomes or supplies successful acknowledgements.

Producer `cleanroom/20261029T053520Z-6982`; R1 repairs `33924`; R2 repairs
`47883`. Independent R3 clears the repaired account, parent-acknowledgement,
decision-authority and cancellation paths of P0-P1; the fault-point P2 is fixed.
The tests-first commits and RED receipts are in `U113-cloud-publication.md`.

The first execution compiled 15 cases: 13 passed and two failed before their
intended checks. Independent diagnosis identified invalid fixture setup.
Repair `cleanroom/20261029T065219Z-63939` uses a fixed synthetic key vector
for the parent-ordering test and the engine's controlled replacement seam for
the native-file test. It preserves journal admission and every assertion.
Independent exact-delta review is clear. Three comments and one diagnostic
were also changed for privacy, without behavior changes.

That execution passed 14 cases and exposed a real selection defect: equal-time
isolated bindings could select the old Republished row on reopen. Repair
`cleanroom/20261029T070253Z-68982` selects the operation's exact binding and
validates its scope, account, epoch, record key and publication. Bad references
fail closed. The test retains its clock and IDs and adds the binding-ID check.
Independent narrow review finds no new P0-P2.

Verification on the repaired source:

- `core-test/20261029T070710Z-71005`: all 15 host cases pass, none skipped.
- `core-test/20261029T070002Z-67778`: all 157 engine library, replacement,
  lifecycle, session and durability cases pass. Engine inputs are unchanged by
  the subsequent Runtime selection fix.
- `core-test/20261029T070900Z-72351`: 102 adjacent CloudKit cases pass;
  65 skips are reported under the explicit focused filter.
- `core-format/20261029T070602Z-70271`: pass.
- `private/20261029T070713Z-71268`: all 20 changed files pass.
- `similarity/20261029T070717Z-71541`: 20 files, zero findings or exemptions.
- `core-lint/20261029T071114Z-73224`: RED on eight new Clippy findings:
  two large enum variants and six nested conditions. No lint pass is claimed.

Next: resolve lint without changing behavior, then implement the real unbound
child driver and PublicationRig. The original nine contracts remain enabled;
their missing coordinator dependencies are not counted as passes. Adoption,
incoming data, media recovery, native SDK wiring,
preview/backup provenance, reset recovery with its 37-code tests, signed checks
and full integration remain required. No Unit trailer or P24 closure.
