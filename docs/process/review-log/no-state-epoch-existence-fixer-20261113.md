# No-state epoch existence fixer

Role: fixer (implementer clean-room rules). Canonical start:
`6a19e8e3b8eb9a64a30b022e25dfb9b6a9070ee3`. Original pending integration base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Session: `s-20261110T214025Z-20fa15fd`.

Contract: sync/policy P3, P6 (Unknown epoch crt), P12.3;
sync/cloudkit-records K12.5, K17.4; sync/cloudkit-recovery V26.1.

The ordinary typed epoch Found path qualified the encrypted body through
epoch_crt_store::fetched, then returned when that qualification failed. Thus a
real MGEpoch without e produced an accepted found diagnostic but never reached
the collection's false drain or decided-zone insertion. The downstream commit
also took epoch_verification for every non-active read, which would incorrectly
clear that fence once existence-only replies reached it.

Restrict the early return to SyncNowEpoch preparation and take the verification
obligation only when fetched_epoch is qualified. Ordinary existence records its
false drain and decided zone under the existing guard/scan and commit scope.
The complete-body qualifier and preparation outcomes are unchanged. Missing
epoch followups, read failures, identity/SDK fences, A7 diagnostics, API-return
timing and native tests are unchanged.

Coverage added in cloud_publication_phase/epoch_crt_gate.rs uses issued reads,
Service projected nil-e typed records, and native-shaped payload variants.
The cases retain verification after an existence reply. They refuse writes
with an unresolved fence or unknown authority. A complete valid response
permits the pending Config update and outbound batch.
Existing complete unequal reply coverage remains. Additional assertions in
support/media_rootless_ack.rs cover preparation's nil-e, malformed,
unsupported, missing-system-fields and publishing bodies: each stays unready,
adds no drain/cache, and preserves the existing successful preparation proof.
The tests deliver replies through issued read handles. The runtime owns the
resulting tracking and authority facts.

The filtered author checked contract alignment and module declarations.
That environment exported source only. The main checkout owns executed
verification and the separate review receipts below.

Follow-up to source review `cleanroom/20261113T115927Z-58239` and root runtime
receipt `core-test/20261113T120009Z-60643`: correct only the two new test
observations. The known-equal case retains the actual Service Active read,
expects no second first-confirmation, and checks SDK validity, confirmed root,
unchanged usable/verified binding and persisted request/epoch/envelope/system
fields. The unknown-authority case observes the Service Config key, mtime and
exact value immediately after the valid batch builder drains incoming work;
a subsequent zero drain must preserve the entire row. All invalid-reply
assertions and subsequent known-equal pending-verification/send checks remain.
Production bytes, guard semantics and native tests are unchanged by this
follow-up. One source consistency check covers the observation accessors and
declarations; root retains compilation, runtime and independent delta review.

Focused Docker selectors for root:

```sh
tools/mg core-test -- --test cloud_publication_phase -E 'package(meguri-host) & (test(epoch_crt_gate::epoch_crt_gate_existence_only) | test(=epoch_crt_gate::epoch_crt_gate_existing_epoch_read_persists_complete_unequal_body_atomically))'
tools/mg core-test -- --test cloud_media_rootless_ack -E 'package(meguri-host) & (test(=rootless_ack::paused_media_name_ack_without_root_survives_open_and_rechecks_delayed_deletion) | test(=rootless_ack::qualified_false_drain_rechecks_rootless_ack_without_completing_absence))'
```

Focused native selector for the original seven assertion records:

```sh
tools/mg swift-test --package MeguriCore -- --filter 'cloudKitNoStateFoundControlsUseLatestOriginIncludingJoinedRepeat|cloudKitNoStateFoundEpochAfterPriorWillLogsWithoutA7OrAbsenceCompletion|cloudKitNoStateFailedEpochReadWaitsForAnotherCallWithoutRetry'
```

## Independent review and executed checks

The filtered author run `cleanroom/20261113T115040Z-50875` made the repair.
Independent R1 `20261113T115927Z-58239` found no P0/P1 and one P2 in a new
test observation. The first Rust run `20261113T120009Z-60643` passed three
selected cases and failed two new observations. One treated an existing root
confirmation as new; the other expected work after the builder had applied it.

Fixer R2 `cleanroom/20261113T120255Z-62887` corrected only those observations
and this handoff. Root compared the patches: production bytes and original
native tests remained unchanged. Independent R2 `20261113T120621Z-65694`
found no P0/P1/P2 in that delta. These reviews were read-only filtered runs.

Root applied the final patch and verified it through MG:

- `core-test/20261113T120636Z-66186`: all 16 selected epoch and media cases
  passed, with no skips.
- `swift-test/20261113T120724Z-66754`: all six unchanged native methods
  passed, including the seven retained drain-count assertions. Error, absent
  record and stale-identity cases passed in the same selection.
- `core-lint/20261113T120750Z-68665`: 3/3 checks passed.
- `static-checks/20261113T120728Z-67676`: 3/3 checks passed.
- `private/20261113T120729Z-67988`: all three source paths passed.
- `similarity/20261113T120729Z-68187`: all three source paths passed,
  with zero exemptions.

Final document and staged privacy receipts are retained with the checkpoint.
These focused checks do not replace the pending batch gate. Synthetic native
callbacks do not prove live peer delivery or playback. Unit and P24 closure,
milestone gates and final acceptance remain open.
