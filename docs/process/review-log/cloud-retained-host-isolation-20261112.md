# Retained Host isolation failures — bounded fixer handoff

- Role: fixer, following `docs/process/roles/implementer.md` and applicable AGENTS.
- Requested upstream base: `9f234507`, reported clean master. This filtered
  runner started clean on master at `e5ded0dd`; upstream ancestry is not
  independently established here. No history or private run artifacts read.
- Reported authoritative result: `core-test/20261112T085113Z-82325`, 81/84
  passed. These two failures remain retained; the separately repaired third
  failure supplies no validation for this patch.

## Exact source causes

`first_manual_capture_cannot_send_after_changed_or_deleted_root`: the changed
epoch branch (`deleted = false`) reaches `adapter_did_fetch` →
`accept_root_fetch` → `accept_active_proof`, whose differing-epoch branch
isolates `EpochSwitched`; its existing expected error follows the SDK recheck.
The deletion branch (`deleted = true`) instead takes `adapter_fetched_owned`'s
early return through `adapter_observe_zone_losses`. That function records
`known_loss` and its `after_read = next_identity`, then returns without
isolation. The test immediately expected unusable/unverified state, without a
new identity read or `adapter_accept_zone_losses`. Thus the deletion iteration
reaches the reported assertion with usable/verified state still set.

`native_authority_retire_isolation_precedes_held_readback_and_survives_failure`:
the same-account `adapter_account` event sets the no-state point but never
creates `AcceptedIdentityRead`. The owned missing epoch result records
`collection_loss` and `pending_collection_loss = read.clone()`, returning the
Active followup. `retire_sdk_with_work` requires `original && fresh` before
isolation. `fresh` is false: no begin/complete identity read occurred in this
fixture, so `accepted_identity_read` is absent. Even a cached proof would fail
V2.2 requires `request > read.loss_identity_after`. Without this proof, the runtime
skips the isolation/readback block, clears the pending read and marks the SDK stopping.
It never reaches `SDKIsolationPublished` or consumes the armed readback fault.
The hold helper waits five seconds for entry and five seconds for release;
`observed`, `released` and `unusable` are false. The first, non-fault iteration
already fails the combined assertion; both parameter values lacked the proof.

These are test preparation inconsistencies with approved
`sync/cloudkit-recovery#V2.2` (read the account again before service isolation),
`#V9.1` (same-account event is a no-state point), `#V26.1`, and
`sync/cloudkit-records#K12.5` (root deletion and instance-end loss outcomes).
Actual composition confirms the distinction: Host `cloud_policy_input` calls
Driver handling, whose `DeviceEvent::Account` uses `adapter_account_with_work`,
not identity-read issuance. `CloudWorkflow::complete_identity_read` in the
existing FFI composition completes the issued read and then calls
`adapter_accept_zone_losses` for the matching active account. This is source
evidence, not an executable trace or a finding about unrelated behavior.

## Bounded change

Only the two named tests change. After deleted Active delivery, issue and
complete a fresh read of the existing synthetic account through Runtime's
normal begin/complete calls, assert completion acceptance, and accept the
queued loss. After the missing epoch produces the Active followup, issue and
complete the same fresh account read before retiring the original SDK. No
new SDK or root result is used; proof comes from the existing Runtime API.

Every original assertion, expected result, loop input and timing limit remains.
Both cases keep the held hook, validation
before release, unusability, matching compact authority, and the exact success
or database-failure outcome. Production committed unusability before readback
and compact final validation are untouched. No production/API/FFI/Swift change,
diagnostic export, retry, suppression, flag reset or new cloud behavior.

## Verification limits

One source consistency pass only: scoped diff inspection and `git diff --check`.
No compilation or executable checks run here. All MG formatting/lint, focused
Rust tests (both complete parameter loops and the surrounding Host suite), the
retained 84-test selection, integration/full gates, independent review,
provenance/privacy/similarity checks and commit remain with the integrator.
No signed/device/VM check was run. No passing-run, acceptance, unit-completion
or gate claim; these retained failures require fresh executable evidence.

## Integration evidence

- Actual main-checkout starting commit: `9f234507`.
- Independent review `cleanroom/20261112T102332Z-4021`: no P0-P2 findings. The review checked the account-read boundary, actual Runtime/FFI path and preserved negative coverage.
- `core-test/20261112T102337Z-4374`: the complete retained selection passed, 84 tests, 335 skipped by the explicit selector. Both full parameter loops passed. The original `82325` failed result remains retained.
- `core-format/20261112T102440Z-5315`: passed without source changes. `similarity/20261112T102445Z-5914`: passed.
- App check `swift-test/20261112T102105Z-99936`: fail, four groups, 22 issues. Three groups remain open; their failures are retained. The one passing group is recorded in the original result.
- No production code changed. Full integration, wider recovery and release acceptance remain pending. No Unit trailer is added.

The unchanged commit hook flagged prose and one source comment. A filtered pass changed only that comment; exact source comparison confirmed all executable test text matches the tested source. The log wording was corrected. No guard or exception changed.
