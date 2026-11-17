# Native own-handoff attribution verification

The original selected native failure is focused-verified on clean
`f1448fc8bec9a2186de9e62342f91517c581e604`. Unit and full-gate acceptance remain incomplete.
Keep failed gate `gate/20261116T005859Z-94440` and integration base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Failure and repair

Retain `swift-test/20261117T135618Z-54731`, clean `42d33adb1`: the original
`actualCollectionReplacementRetiresProviderAndCredentialOwnersBeforeCurrentSettingsAndReturn`
failed. Its refusing guard recorded `startEngineRuntimePreparation`,
`runtimeIdentityUnverified`, `firstTaskCancelled=0`, `providerAdmissionDenied`
and `providerHandoff=present`. The later owned-shutdown cancellation was separate.
Earlier `092016Z-58092`, clean `874b9e3ad`, passed 11 of 12 methods.
Diagnostic receipts `core-test/20261117T133653Z-27011` (5/0/0) and
`swift-test/20261117T135102Z-47566` (43/0/0) remain diagnostic evidence.

Export `cleanroom/20261117T152605Z-55450` became WIP commit `f1448fc8`.
The repair uses process-local closure provenance captured with actual admission.
Only a successful durable handoff sets its revision. At the refusing guard,
the same ordinary closure, matching handoff revision and settled tracking must
all hold. Missing or unreadable tracking remains strict. A foreign Host ticket
returns plain StaleIncarnation. Only issued StartEngine uses scoped preparation;
ConfigureAutomatic retains the original preparation path.

Original errors, result cells, ACK ordering, SyncCoordinator and existing
assertions remain unchanged. Three strict negatives retain sticky genuine
identity failures during owned shutdown, without cancellation and under
background expiry. The repair adds no cancellation-only suppression or delay.

## Review and checks

Independent full source review `lead/reviews/native-repair-fix1.md` reports no
required finding, optional P2 findings only, and complete-content PUBLIC review
of all 12 files. Optional concerns cover the test-only witness lock and raw-row
fixture coupling. Actual execution below resolves the nonzero-test-count concern.
Root records seven reviewed exact public snapshots and passing ordinary checks.
This evidence record does not replace the required provenance or integration checks.

All receipts below use clean `f1448fc8bec9a2186de9e62342f91517c581e604`.
Counts are passed / failed / skipped.

| Receipt | Scope | Result |
|---|---|---|
| `core-test/20261117T155005Z-81091` | Host `own_handoff_attribution`, actual test-hooks feature | 17 / 0 / 0 |
| `core-test/20261117T155138Z-89683` | Sync preparation-refusal and FFI original-error/witness tests | 5 / 0 / 0 |
| `swift-test/20261117T155733Z-19021` | Original unchanged native method alone | 1 / 0 / 0 |
| `swift-test/20261117T161216Z-49095` | Original 12-method cohort and three strict negatives | 15 / 0 / 0 |
| `core-test/20261117T161627Z-87531` | Whole Host `cloud_publication_phase` binary | 155 / 0 / 0 |

The broader Swift run retains all argument cases: Reopen and Resume, four
provider-metadata inputs and both successor-attachment failure cases.

## Remaining verification

The current whole Host publication binary passes all 155 methods with zero
failures or skips. Exact terminal method IDs map all 121 original failures in
`core-test/20261117T091221Z-42310` to current passes, with no missing method.
All 15 earlier passes, including the four restoration/provider-choice cases, also pass.
Earlier `core-test/20261117T130355Z-60781` passed 155 methods
on clean `44b3d527d`; both receipts remain retained, along with every prior failure.
Required integration and final acceptance checks remain incomplete. The affected
checks above grant no Unit or full-gate acceptance.
